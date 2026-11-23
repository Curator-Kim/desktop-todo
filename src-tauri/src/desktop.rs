use std::{cell::Cell, sync::Mutex, time::SystemTime};
use tauri::{Emitter, Manager};

#[derive(Default)]
pub struct DesktopState(pub Mutex<Option<Box<Saved>>>);
#[derive(Default)]
pub struct Saved {
    #[cfg(windows)]
    renderer: Option<crate::desktop_rendering::RenderScale>,
    style: isize,
    ex_style: isize,
    nc_policy: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    min_size: Option<tauri::LogicalSize<f64>>,
    placement: Cell<Option<[i32; 4]>>,
    repairing: Cell<bool>,
    refresh: Cell<bool>,
    last_tick: Cell<Option<SystemTime>>,
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::ptr::null_mut;
    use windows_sys::Win32::{
        Foundation::{GetLastError, SetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{
            Dwm::{
                DwmGetWindowAttribute, DwmSetWindowAttribute, DWMNCRP_DISABLED,
                DWMWA_NCRENDERING_POLICY,
            },
            Gdi::{MapWindowPoints, RedrawWindow, RDW_ALLCHILDREN, RDW_FRAME, RDW_INVALIDATE},
        },
        UI::HiDpi::{
            GetWindowDpiAwarenessContext, PhysicalToLogicalPointForPerMonitorDPI,
            SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        },
        UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        UI::WindowsAndMessaging::*,
    };
    const PROGMAN: &[u16] = &[80, 114, 111, 103, 109, 97, 110, 0];
    const WORKER: &[u16] = &[87, 111, 114, 107, 101, 114, 87, 0];
    const ICONS: &[u16] = &[
        83, 72, 69, 76, 76, 68, 76, 76, 95, 68, 101, 102, 86, 105, 101, 119, 0,
    ];
    const SUBCLASS_ID: usize = 0x44544f44;
    const REPAIR_TIMER: usize = 0x44544f45;
    const FRAME_STYLE: u32 = WS_CAPTION | WS_THICKFRAME | WS_BORDER | WS_DLGFRAME;
    const FRAME_EX_STYLE: u32 =
        WS_EX_CLIENTEDGE | WS_EX_WINDOWEDGE | WS_EX_DLGMODALFRAME | WS_EX_STATICEDGE;

    fn desktop_style(style: isize) -> isize {
        (style & !((FRAME_STYLE | WS_POPUP) as isize)) | WS_CHILD as isize
    }

    fn desktop_ex_style(style: isize) -> isize {
        style & !(FRAME_EX_STYLE as isize)
    }

    // Installed and removed on the window's owning thread. The Saved value is
    // boxed before installation and outlives the subclass and its timer.
    unsafe extern "system" fn desktop_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        data: usize,
    ) -> LRESULT {
        let saved = &*(data as *const Saved);
        match message {
            // A desktop child has no non-client area, even when DWM is rebuilt
            // after sleep. Prevent Tao/DefWindowProc from reserving frame pixels.
            WM_NCCALCSIZE | WM_NCPAINT => return 0,
            WM_STYLECHANGING if lparam != 0 => {
                let styles = &mut *(lparam as *mut STYLESTRUCT);
                if wparam as i32 == GWL_STYLE {
                    styles.styleNew = desktop_style(styles.styleNew as isize) as u32;
                } else if wparam as i32 == GWL_EXSTYLE {
                    styles.styleNew = desktop_ex_style(styles.styleNew as isize) as u32;
                }
            }
            WM_DPICHANGED => {
                // SetParent can change the effective DPI. Keep the captured
                // physical rectangle instead of Tao's suggested logical resize.
                saved.refresh.set(true);
                return 0;
            }
            WM_GETMINMAXINFO if lparam != 0 => {
                let limits = &mut *(lparam as *mut MINMAXINFO);
                limits.ptMinTrackSize.x = 1;
                limits.ptMinTrackSize.y = 1;
                return 0;
            }
            WM_WINDOWPOSCHANGING if !saved.repairing.get() && lparam != 0 => {
                if let Some([x, y, width, height]) = saved.placement.get() {
                    let position = &mut *(lparam as *mut WINDOWPOS);
                    position.x = x;
                    position.y = y;
                    position.cx = width;
                    position.cy = height;
                    position.flags &= !(SWP_NOMOVE | SWP_NOSIZE);
                }
            }
            WM_DISPLAYCHANGE | WM_POWERBROADCAST | WM_DWMCOMPOSITIONCHANGED | WM_THEMECHANGED => {
                saved.refresh.set(true);
            }
            WM_TIMER if wparam == REPAIR_TIMER => {
                let now = SystemTime::now();
                let resumed = saved
                    .last_tick
                    .replace(Some(now))
                    .and_then(|previous| now.duration_since(previous).ok())
                    .is_some_and(|elapsed| elapsed.as_secs() >= 4);
                let refresh = saved.refresh.replace(false) || resumed;
                let _ = repair(hwnd, saved, refresh);
                return 0;
            }
            WM_NCDESTROY => {
                KillTimer(hwnd, REPAIR_TIMER);
                RemoveWindowSubclass(hwnd, Some(desktop_proc), SUBCLASS_ID);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }

    unsafe fn install_hook(hwnd: HWND, saved: &Saved) -> Result<(), String> {
        if SetWindowSubclass(
            hwnd,
            Some(desktop_proc),
            SUBCLASS_ID,
            saved as *const Saved as usize,
        ) == 0
        {
            return Err("无法启用桌面窗口保护。".into());
        }
        if SetTimer(hwnd, REPAIR_TIMER, 1000, None) == 0 {
            RemoveWindowSubclass(hwnd, Some(desktop_proc), SUBCLASS_ID);
            return Err("无法启用合盖恢复保护。".into());
        }
        saved.last_tick.set(Some(SystemTime::now()));
        Ok(())
    }

    unsafe fn remove_hook(hwnd: HWND) {
        KillTimer(hwnd, REPAIR_TIMER);
        RemoveWindowSubclass(hwnd, Some(desktop_proc), SUBCLASS_ID);
    }

    unsafe fn strip_frame(hwnd: HWND) -> bool {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let changed = style != desktop_style(style) || ex_style != desktop_ex_style(ex_style);
        if changed {
            SetWindowLongPtrW(hwnd, GWL_STYLE, desktop_style(style));
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desktop_ex_style(ex_style));
        }
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY as u32,
            &DWMNCRP_DISABLED as *const i32 as _,
            std::mem::size_of::<i32>() as u32,
        );
        changed
    }

    struct RepairGuard<'a>(&'a Cell<bool>);
    impl Drop for RepairGuard<'_> {
        fn drop(&mut self) {
            self.0.set(false);
        }
    }

    unsafe fn repair(hwnd: HWND, saved: &Saved, refresh: bool) -> Result<(), String> {
        if saved.repairing.replace(true) {
            return Ok(());
        }
        let _guard = RepairGuard(&saved.repairing);
        let _physical = PhysicalCoordinates::enter()?;
        let frame_changed = strip_frame(hwnd);
        let mut actual: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut actual) == 0 {
            return Err("无法读取桌面窗口尺寸。".into());
        }
        if refresh || frame_changed || !matches_target(saved, &actual) {
            place_on_desktop(hwnd, saved)?;
            RedrawWindow(
                hwnd,
                std::ptr::null(),
                null_mut(),
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME,
            );
        }
        Ok(())
    }

    fn matches_target(saved: &Saved, rect: &RECT) -> bool {
        (rect.right - rect.left - saved.width).abs() <= 1
            && (rect.bottom - rect.top - saved.height).abs() <= 1
            && (rect.left - saved.x).abs() <= 1
            && (rect.top - saved.y).abs() <= 1
    }

    // Shell parenting may change DPI contexts. Capture and place in physical
    // coordinates consistently, and always restore the caller's context.
    struct PhysicalCoordinates(DPI_AWARENESS_CONTEXT);
    impl PhysicalCoordinates {
        unsafe fn enter() -> Result<Self, String> {
            let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            if previous.is_null() {
                return Err("无法使用物理像素保持窗口尺寸。".into());
            }
            Ok(Self(previous))
        }
    }
    impl Drop for PhysicalCoordinates {
        fn drop(&mut self) {
            unsafe {
                SetThreadDpiAwarenessContext(self.0);
            }
        }
    }

    #[derive(Debug, PartialEq)]
    struct Placement {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    }

    impl Placement {
        fn compensate(&mut self, target: &Saved, actual: &RECT) -> Result<bool, String> {
            let width = actual.right - actual.left;
            let height = actual.bottom - actual.top;
            if width <= 0 || height <= 0 {
                return Err("无法读取融入桌面后的窗口尺寸。".into());
            }
            if (width - target.width).abs() <= 1
                && (height - target.height).abs() <= 1
                && (actual.left - target.x).abs() <= 1
                && (actual.top - target.y).abs() <= 1
            {
                return Ok(true);
            }
            // The host's logical units can differ from the child's actual
            // physical pixels after SetParent. Measure that ratio rather than
            // assuming both windows adopted the same monitor's DPI.
            let scale_x = self.width as f64 / width as f64;
            let scale_y = self.height as f64 / height as f64;
            self.x += ((target.x - actual.left) as f64 * scale_x).round() as i32;
            self.y += ((target.y - actual.top) as f64 * scale_y).round() as i32;
            self.width = (target.width as f64 * scale_x).round().max(1.0) as i32;
            self.height = (target.height as f64 * scale_y).round().max(1.0) as i32;
            Ok(false)
        }
    }

    // The desktop host can use a different DPI context from the monitor on
    // which the note was placed. Convert BOTH corners into the host's units.
    unsafe fn place_on_desktop(hwnd: HWND, original: &Saved) -> Result<(), String> {
        let host = GetParent(hwnd);
        if host.is_null() {
            return Err("桌面层已失效，请从托盘恢复便签。".into());
        }
        let mut corners = [
            windows_sys::Win32::Foundation::POINT {
                x: original.x,
                y: original.y,
            },
            windows_sys::Win32::Foundation::POINT {
                x: original.x + original.width,
                y: original.y + original.height,
            },
        ];
        for point in &mut corners {
            if PhysicalToLogicalPointForPerMonitorDPI(host, point) == 0 {
                return Err("无法换算当前屏幕的桌面坐标。".into());
            }
        }
        let context = GetWindowDpiAwarenessContext(host);
        let previous = SetThreadDpiAwarenessContext(context);
        if previous.is_null() {
            return Err("无法切换桌面坐标系。".into());
        }
        let _host_coordinates = PhysicalCoordinates(previous);
        for point in &mut corners {
            MapWindowPoints(null_mut(), host, point, 1);
        }
        let mut placement = Placement {
            x: corners[0].x,
            y: corners[0].y,
            width: corners[1].x - corners[0].x,
            height: corners[1].y - corners[0].y,
        };
        for _ in 0..4 {
            if SetWindowPos(
                hwnd,
                HWND_BOTTOM,
                placement.x,
                placement.y,
                placement.width,
                placement.height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
            ) == 0
            {
                return Err("无法保持双屏下的窗口位置和尺寸。".into());
            }
            let mut actual: RECT = std::mem::zeroed();
            {
                let _physical = PhysicalCoordinates::enter()?;
                if GetWindowRect(hwnd, &mut actual) == 0 {
                    return Err("无法校验融入桌面后的窗口尺寸。".into());
                }
            }
            if placement.compensate(original, &actual)? {
                original.placement.set(Some([
                    placement.x,
                    placement.y,
                    placement.width,
                    placement.height,
                ]));
                return Ok(());
            }
        }
        Err("无法保持原始窗口尺寸，已取消融入桌面。".into())
    }

    unsafe extern "system" fn find_host(window: HWND, context: LPARAM) -> i32 {
        if !FindWindowExW(window, null_mut(), ICONS.as_ptr(), std::ptr::null()).is_null() {
            let worker = FindWindowExW(null_mut(), window, WORKER.as_ptr(), std::ptr::null());
            if !worker.is_null() {
                *(context as *mut HWND) = worker;
                return 0;
            }
        }
        1
    }

    pub fn embed(
        window: &tauri::WebviewWindow,
        state: &DesktopState,
        controller: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
    ) -> Result<(), String> {
        let mut saved = state.0.lock().map_err(|e| e.to_string())?;
        if saved.is_some() {
            return Ok(());
        }
        let render_lock = crate::desktop_rendering::RenderScale::freeze(controller)?;
        unsafe {
            let _coordinates = PhysicalCoordinates::enter()?;
            let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as HWND;
            // Capture before asking Explorer to prepare its desktop layer.
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut rect) == 0 {
                return Err("无法读取窗口位置。".into());
            }
            let mut nc_policy = 0i32;
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_NCRENDERING_POLICY as u32,
                &mut nc_policy as *mut i32 as _,
                std::mem::size_of::<i32>() as u32,
            );
            let original = Box::new(Saved {
                renderer: Some(render_lock.snapshot),
                style: GetWindowLongPtrW(hwnd, GWL_STYLE),
                ex_style: GetWindowLongPtrW(hwnd, GWL_EXSTYLE),
                nc_policy,
                x: rect.left,
                y: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
                min_size: window
                    .app_handle()
                    .config()
                    .app
                    .windows
                    .iter()
                    .find(|config| config.label == "main")
                    .and_then(|config| config.min_width.zip(config.min_height))
                    .map(|(width, height)| tauri::LogicalSize::new(width, height)),
                ..Saved::default()
            });
            let progman = FindWindowW(PROGMAN.as_ptr(), std::ptr::null());
            if progman.is_null() {
                return Err("找不到 Windows 桌面。".into());
            }
            let mut result = 0;
            SendMessageTimeoutW(progman, 0x052c, 0xd, 1, SMTO_ABORTIFHUNG, 1000, &mut result);
            let mut host = FindWindowExW(progman, null_mut(), WORKER.as_ptr(), std::ptr::null());
            if host.is_null() {
                SendMessageTimeoutW(progman, 0x052c, 0, 0, SMTO_ABORTIFHUNG, 1000, &mut result);
                EnumWindows(Some(find_host), &mut host as *mut HWND as LPARAM);
            }
            if host.is_null() {
                return Err("当前桌面不支持嵌入模式，便签仍保留在原处。".into());
            }
            // Tao converts logical minWidth/minHeight using the new host DPI
            // during WM_GETMINMAXINFO. Small list windows can otherwise be
            // enlarged even though the same geometry works for a large calendar.
            window
                .set_min_size(None::<tauri::LogicalSize<f64>>)
                .map_err(|e| e.to_string())?;
            window.set_ignore_cursor_events(true).map_err(|e| {
                let _ = window.set_min_size(original.min_size);
                e.to_string()
            })?;
            if let Err(error) = install_hook(hwnd, &original) {
                let _ = window.set_ignore_cursor_events(false);
                let _ = window.set_min_size(original.min_size);
                return Err(error);
            }
            strip_frame(hwnd);
            SetLastError(0);
            let parent = SetParent(hwnd, host);
            if parent.is_null() && GetLastError() != 0 {
                remove_hook(hwnd);
                SetWindowLongPtrW(hwnd, GWL_STYLE, original.style);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, original.ex_style);
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_NCRENDERING_POLICY as u32,
                    &original.nc_policy as *const i32 as _,
                    std::mem::size_of::<i32>() as u32,
                );
                let _ = window.set_ignore_cursor_events(false);
                let _ = window.set_min_size(original.min_size);
                return Err("Windows 拒绝嵌入桌面。".into());
            }
            if let Err(error) = repair(hwnd, &original, true) {
                remove_hook(hwnd);
                SetParent(hwnd, null_mut());
                SetWindowLongPtrW(hwnd, GWL_STYLE, original.style);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, original.ex_style);
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_NCRENDERING_POLICY as u32,
                    &original.nc_policy as *const i32 as _,
                    std::mem::size_of::<i32>() as u32,
                );
                SetWindowPos(
                    hwnd,
                    HWND_TOP,
                    original.x,
                    original.y,
                    original.width,
                    original.height,
                    SWP_FRAMECHANGED | SWP_SHOWWINDOW,
                );
                let _ = window.set_ignore_cursor_events(false);
                let _ = window.set_min_size(original.min_size);
                return Err(error);
            }
            render_lock.persist();
            *saved = Some(original);
        }
        Ok(())
    }

    pub fn restore(
        window: &tauri::WebviewWindow,
        state: &DesktopState,
        controller: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
    ) -> Result<(), String> {
        let mut saved = state.0.lock().map_err(|e| e.to_string())?;
        if let Some(original) = saved.as_ref() {
            unsafe {
                let _coordinates = PhysicalCoordinates::enter()?;
                let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as HWND;
                remove_hook(hwnd);
                SetLastError(0);
                let parent = SetParent(hwnd, null_mut());
                if parent.is_null() && GetLastError() != 0 {
                    let _ = install_hook(hwnd, original);
                    return Err("无法恢复便签窗口，请退出程序后重新打开。".into());
                }
                SetWindowLongPtrW(hwnd, GWL_STYLE, original.style);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, original.ex_style);
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_NCRENDERING_POLICY as u32,
                    &original.nc_policy as *const i32 as _,
                    std::mem::size_of::<i32>() as u32,
                );
                window
                    .set_ignore_cursor_events(false)
                    .map_err(|e| e.to_string())?;
                if SetWindowPos(
                    hwnd,
                    HWND_TOP,
                    original.x,
                    original.y,
                    original.width,
                    original.height,
                    SWP_FRAMECHANGED | SWP_SHOWWINDOW,
                ) == 0
                {
                    return Err("无法恢复便签位置。".into());
                }
                window
                    .set_min_size(original.min_size)
                    .map_err(|e| e.to_string())?;
            }
            if let Some(renderer) = original.renderer {
                renderer.restore(controller)?;
            }
            *saved = None;
        }
        Ok(())
    }

    pub fn preserve_size(window: &tauri::Window, state: &DesktopState) {
        // SetParent and Wry may deliver a delayed DPI resize. Ignore synchronous
        // transition callbacks (the state is locked), correct later resizes.
        let Ok(saved) = state.0.try_lock() else {
            return;
        };
        let Some(original) = saved.as_ref() else {
            return;
        };
        unsafe {
            let Ok(handle) = window.hwnd() else { return };
            let _ = repair(handle.0 as HWND, original, false);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        struct TestWindows {
            host: HWND,
            note: HWND,
        }
        impl TestWindows {
            unsafe fn new() -> Self {
                // Hidden standard Windows controls exercise real HWND messages
                // without touching the user's desktop host or running app.
                let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
                let host = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    std::ptr::null(),
                    WS_POPUP,
                    0,
                    0,
                    3000,
                    2000,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    std::ptr::null(),
                );
                assert!(!host.is_null());
                let note = CreateWindowExW(
                    WS_EX_CLIENTEDGE,
                    class.as_ptr(),
                    std::ptr::null(),
                    WS_POPUP | WS_THICKFRAME | WS_BORDER,
                    120,
                    90,
                    380,
                    520,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    std::ptr::null(),
                );
                if note.is_null() {
                    DestroyWindow(host);
                    panic!("Cannot create test note");
                }
                Self { host, note }
            }
        }
        impl Drop for TestWindows {
            fn drop(&mut self) {
                unsafe {
                    DestroyWindow(self.note);
                    DestroyWindow(self.host);
                }
            }
        }
        struct TestHook(HWND);
        impl Drop for TestHook {
            fn drop(&mut self) {
                unsafe {
                    remove_hook(self.0);
                }
            }
        }

        #[test]
        fn native_desktop_keeps_size_and_client_area_through_dpi_and_resume() {
            unsafe {
                let _physical = PhysicalCoordinates::enter().unwrap();
                let windows = TestWindows::new();
                let mut before: RECT = std::mem::zeroed();
                assert_ne!(GetWindowRect(windows.note, &mut before), 0);
                let saved = Box::new(Saved {
                    style: GetWindowLongPtrW(windows.note, GWL_STYLE),
                    ex_style: GetWindowLongPtrW(windows.note, GWL_EXSTYLE),
                    x: before.left,
                    y: before.top,
                    width: before.right - before.left,
                    height: before.bottom - before.top,
                    ..Saved::default()
                });
                install_hook(windows.note, &saved).unwrap();
                let hook = TestHook(windows.note);
                strip_frame(windows.note);
                SetParent(windows.note, windows.host);
                repair(windows.note, &saved, true).unwrap();

                // Windows/Tao may request a different rectangle after a DPI
                // transition. Both the DPI message and delayed resize are tested.
                let mut suggested = RECT {
                    left: 200,
                    top: 160,
                    right: 960,
                    bottom: 1200,
                };
                SendMessageW(
                    windows.note,
                    WM_DPICHANGED,
                    144 | (144 << 16),
                    &mut suggested as *mut RECT as LPARAM,
                );
                assert_ne!(
                    SetWindowPos(
                        windows.note,
                        null_mut(),
                        200,
                        160,
                        760,
                        1040,
                        SWP_NOZORDER | SWP_NOACTIVATE
                    ),
                    0
                );

                // Reproduce frame styles being restored by system/framework
                // work while our normal hook is temporarily absent.
                remove_hook(windows.note);
                SetWindowLongPtrW(
                    windows.note,
                    GWL_STYLE,
                    GetWindowLongPtrW(windows.note, GWL_STYLE) | WS_THICKFRAME as isize,
                );
                SetWindowLongPtrW(
                    windows.note,
                    GWL_EXSTYLE,
                    GetWindowLongPtrW(windows.note, GWL_EXSTYLE) | WS_EX_CLIENTEDGE as isize,
                );
                install_hook(windows.note, &saved).unwrap();
                saved
                    .last_tick
                    .set(Some(SystemTime::now() - std::time::Duration::from_secs(60)));
                SendMessageW(windows.note, WM_DWMCOMPOSITIONCHANGED, 0, 0);
                SendMessageW(windows.note, WM_TIMER, REPAIR_TIMER, 0);

                let mut after: RECT = std::mem::zeroed();
                assert_ne!(GetWindowRect(windows.note, &mut after), 0);
                assert!(matches_target(&saved, &after));
                let mut client: RECT = std::mem::zeroed();
                assert_ne!(GetClientRect(windows.note, &mut client), 0);
                assert_eq!(
                    client.right,
                    after.right - after.left,
                    "right-side frame gap"
                );
                assert_eq!(client.bottom, after.bottom - after.top, "bottom frame gap");
                assert_eq!(
                    GetWindowLongPtrW(windows.note, GWL_STYLE) & FRAME_STYLE as isize,
                    0
                );
                assert_eq!(
                    GetWindowLongPtrW(windows.note, GWL_EXSTYLE) & FRAME_EX_STYLE as isize,
                    0
                );

                // Leaving desktop mode removes the size guard and allows normal
                // movement/resizing again.
                drop(hook);
                SetParent(windows.note, null_mut());
                SetWindowLongPtrW(windows.note, GWL_STYLE, saved.style);
                SetWindowLongPtrW(windows.note, GWL_EXSTYLE, saved.ex_style);
                assert_ne!(
                    SetWindowPos(
                        windows.note,
                        null_mut(),
                        160,
                        110,
                        480,
                        620,
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED
                    ),
                    0
                );
                assert_ne!(GetWindowRect(windows.note, &mut after), 0);
                assert_eq!(after.right - after.left, 480);
                assert_eq!(after.bottom - after.top, 620);
            }
        }

        #[test]
        fn preserves_list_and_calendar_rectangles_across_monitor_scales() {
            for (width, height) in [(300, 300), (380, 520), (1327, 967)] {
                for scale in [1.0, 1.25, 1.5, 2.0] {
                    for screen_x in [-1920, 0, 2560] {
                        let target = Saved {
                            style: 0,
                            x: screen_x + 120,
                            y: 90,
                            width,
                            height,
                            min_size: None,
                            ..Saved::default()
                        };
                        let mut placement = Placement {
                            x: 120,
                            y: 90,
                            width,
                            height,
                        };
                        let mut matched = false;
                        for _ in 0..4 {
                            // Simulate the desktop host applying its scale to
                            // a child window on either side of the main screen.
                            let left = screen_x + (placement.x as f64 * scale).round() as i32;
                            let top = (placement.y as f64 * scale).round() as i32;
                            let actual = RECT {
                                left,
                                top,
                                right: left + (placement.width as f64 * scale).round() as i32,
                                bottom: top + (placement.height as f64 * scale).round() as i32,
                            };
                            if placement.compensate(&target, &actual).unwrap() {
                                matched = true;
                                break;
                            }
                        }
                        assert!(
                            matched,
                            "{width}x{height}, scale {scale}, monitor {screen_x}"
                        );
                    }
                }
            }
        }

        #[test]
        fn rejects_empty_physical_bounds() {
            let target = Saved {
                style: 0,
                x: 0,
                y: 0,
                width: 380,
                height: 520,
                min_size: None,
                ..Saved::default()
            };
            let mut placement = Placement {
                x: 0,
                y: 0,
                width: 380,
                height: 520,
            };
            let actual = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            assert!(placement.compensate(&target, &actual).is_err());
        }
    }
}

pub fn preserve_size(window: &tauri::Window) {
    #[cfg(windows)]
    native::preserve_size(window, &window.state::<DesktopState>());
}

pub async fn restore(window: &tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    {
        let (sender, mut receiver) = tauri::async_runtime::channel(1);
        let ui_window = window.clone();
        window
            .with_webview(move |webview| {
                let result = native::restore(
                    &ui_window,
                    &ui_window.state::<DesktopState>(),
                    &webview.controller(),
                );
                // Do not emit JS events or dispatch window actions from this
                // callback: its runtime dispatcher can still hold its lock.
                let _ = sender.try_send(result);
            })
            .map_err(|e| e.to_string())?;
        receiver
            .recv()
            .await
            .ok_or_else(|| "桌面窗口已关闭。".to_string())??;
    }
    // Always continue on the async worker after the WebView callback returns.
    // This path is shared by startup and tray clicks, with or without listeners.
    window
        .emit("desktop-mode", false)
        .map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.unminimize().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn embed_desktop(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("只能将便签放入桌面。".into());
    }
    #[cfg(not(windows))]
    return Err("仅支持 Windows。".into());
    #[cfg(windows)]
    {
        // Comctl32 subclasses and window timers must run on the HWND owner.
        let (sender, mut receiver) = tauri::async_runtime::channel(1);
        let ui_window = window.clone();
        window
            .with_webview(move |webview| {
                let result = native::embed(
                    &ui_window,
                    &ui_window.state::<DesktopState>(),
                    &webview.controller(),
                );
                let _ = sender.try_send(result);
            })
            .map_err(|error| error.to_string())?;
        receiver
            .recv()
            .await
            .ok_or_else(|| "桌面窗口已关闭。".to_string())??;
        window
            .emit("desktop-mode", true)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
