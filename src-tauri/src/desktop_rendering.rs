//! SetParent changes WebView2's effective monitor independently of the outer HWND.
//! Preserve both physical bounds and renderer DPI; preserving only the HWND still
//! shrinks text and task rows on mixed-DPI desktops.
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Controller, ICoreWebView2Controller3, COREWEBVIEW2_BOUNDS_MODE,
    COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS,
};
use windows::core::{Interface, BOOL};

#[derive(Clone, Copy, Debug)]
pub struct RenderScale {
    scale: f64,
    automatic: bool,
    bounds_mode: COREWEBVIEW2_BOUNDS_MODE,
}

impl RenderScale {
    pub fn freeze(controller: &ICoreWebView2Controller) -> Result<ScaleLock, String> {
        let controller: ICoreWebView2Controller3 = controller
            .cast()
            .map_err(|e| format!("无法读取界面缩放设置：{e}"))?;
        unsafe {
            let mut scale = 0.0;
            let mut automatic = BOOL::default();
            let mut bounds_mode = COREWEBVIEW2_BOUNDS_MODE::default();
            controller
                .RasterizationScale(&mut scale)
                .map_err(render_error)?;
            controller
                .ShouldDetectMonitorScaleChanges(&mut automatic)
                .map_err(render_error)?;
            controller
                .BoundsMode(&mut bounds_mode)
                .map_err(render_error)?;
            if !scale.is_finite() || scale <= 0.0 {
                return Err("界面缩放比例无效，已取消融入桌面。".into());
            }
            // The guard rolls back even a partially applied freeze. No COM object
            // is stored in shared state or accessed from a non-owning thread.
            let lock = ScaleLock {
                controller,
                snapshot: Self {
                    scale,
                    automatic: automatic.as_bool(),
                    bounds_mode,
                },
                restore_on_drop: true,
            };
            lock.pin()?;
            Ok(lock)
        }
    }

    pub fn restore(self, controller: &ICoreWebView2Controller) -> Result<(), String> {
        let controller = controller
            .cast::<ICoreWebView2Controller3>()
            .map_err(render_error)?;
        self.restore_inner(&controller)
    }

    fn restore_inner(self, controller: &ICoreWebView2Controller3) -> Result<(), String> {
        unsafe {
            // Restore after reparenting back to the original monitor. Re-enable
            // normal monitor tracking last, so dragging to other screens works.
            controller
                .SetRasterizationScale(self.scale)
                .map_err(render_error)?;
            controller
                .SetBoundsMode(self.bounds_mode)
                .map_err(render_error)?;
            controller
                .SetShouldDetectMonitorScaleChanges(self.automatic)
                .map_err(render_error)
        }
    }
}

pub struct ScaleLock {
    controller: ICoreWebView2Controller3,
    pub snapshot: RenderScale,
    restore_on_drop: bool,
}

impl ScaleLock {
    fn pin(&self) -> Result<(), String> {
        unsafe {
            // Must precede SetParent, otherwise WebView2 may already have
            // replaced the side monitor's scale with the shell host's scale.
            self.controller
                .SetShouldDetectMonitorScaleChanges(false)
                .map_err(render_error)?;
            self.controller
                .SetBoundsMode(COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS)
                .map_err(render_error)?;
            self.controller
                .SetRasterizationScale(self.snapshot.scale)
                .map_err(render_error)
        }
    }

    pub fn persist(mut self) {
        self.restore_on_drop = false;
    }
}

impl Drop for ScaleLock {
    fn drop(&mut self) {
        if self.restore_on_drop {
            let _ = self.snapshot.restore_inner(&self.controller);
        }
    }
}

fn render_error(error: windows::core::Error) -> String {
    format!("无法保持界面显示比例：{error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        ptr::null_mut,
        sync::mpsc,
        time::{Duration, Instant},
    };
    use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
    use windows::{
        core::{w, PCWSTR},
        Win32::{
            Foundation::HWND,
            System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
        },
    };
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    };

    struct ComApartment;
    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    struct TestWindows {
        note: windows_sys::Win32::Foundation::HWND,
        host: windows_sys::Win32::Foundation::HWND,
    }
    impl Drop for TestWindows {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.note);
                DestroyWindow(self.host);
            }
        }
    }
    struct Controller(ICoreWebView2Controller);
    impl Drop for Controller {
        fn drop(&mut self) {
            unsafe {
                let _ = self.0.Close();
            }
        }
    }

    fn eval(webview: &ICoreWebView2, script: &str) -> serde_json::Value {
        let webview = webview.clone();
        let script = script.to_string();
        let (sender, receiver) = mpsc::channel();
        ExecuteScriptCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                let script = CoTaskMemPWSTR::from(script.as_str());
                webview
                    .ExecuteScript(*script.as_ref().as_pcwstr(), &handler)
                    .map_err(Error::WindowsError)
            }),
            Box::new(move |error, result| {
                error?;
                sender.send(result).unwrap();
                Ok(())
            }),
        )
        .unwrap();
        serde_json::from_str(&receiver.recv().unwrap()).unwrap()
    }

    fn measure(webview: &ICoreWebView2, expected_scale: f64) -> serde_json::Value {
        let start = Instant::now();
        loop {
            let result = eval(
                webview,
                r#"(() => {
                const row = document.querySelector('#task');
                return { dpr: devicePixelRatio, width: innerWidth * devicePixelRatio,
                    row: row.getBoundingClientRect().height * devicePixelRatio,
                    font: parseFloat(getComputedStyle(row).fontSize) * devicePixelRatio,
                    icon: document.querySelector('#icon').getBoundingClientRect().width * devicePixelRatio };
            })()"#,
            );
            if (result["dpr"].as_f64().unwrap() - expected_scale).abs() < 0.001
                && (result["width"].as_f64().unwrap() - 900.0).abs() <= 2.0
            {
                return result;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "Renderer scale did not settle: {result}"
            );
        }
    }

    #[test]
    fn real_webview_preserves_viewport_text_and_task_rows_when_reparented() {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap();
        }
        let _com = ComApartment;
        // Isolated hidden windows and temporary browser data: never attach to
        // Explorer or touch the user's app, tasks, profile or monitor settings.
        let windows = unsafe {
            let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let note = CreateWindowExW(
                0,
                w!("STATIC").0,
                std::ptr::null(),
                WS_POPUP,
                100,
                100,
                900,
                600,
                null_mut(),
                null_mut(),
                null_mut(),
                std::ptr::null(),
            );
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
            let host = CreateWindowExW(
                0,
                w!("STATIC").0,
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
            SetThreadDpiAwarenessContext(previous);
            assert!(!note.is_null() && !host.is_null());
            TestWindows { note, host }
        };
        let profile = tempfile::tempdir().unwrap();
        let path: Vec<u16> = profile
            .path()
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let (sender, receiver) = mpsc::channel();
        CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                CreateCoreWebView2EnvironmentWithOptions(
                    PCWSTR::null(),
                    PCWSTR(path.as_ptr()),
                    None,
                    &handler,
                )
                .map_err(Error::WindowsError)
            }),
            Box::new(move |error, environment| {
                error?;
                sender.send(environment.unwrap()).unwrap();
                Ok(())
            }),
        )
        .unwrap();
        let environment = receiver.recv().unwrap();
        let (sender, receiver) = mpsc::channel();
        let parent = HWND(windows.note);
        CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                environment
                    .CreateCoreWebView2Controller(parent, &handler)
                    .map_err(Error::WindowsError)
            }),
            Box::new(move |error, controller| {
                error?;
                sender.send(controller.unwrap()).unwrap();
                Ok(())
            }),
        )
        .unwrap();
        let controller = Controller(receiver.recv().unwrap());
        let view3: ICoreWebView2Controller3 = controller.0.cast().unwrap();
        let webview = unsafe {
            controller
                .0
                .SetBounds(windows::Win32::Foundation::RECT {
                    left: 0,
                    top: 0,
                    right: 900,
                    bottom: 600,
                })
                .unwrap();
            controller.0.SetIsVisible(true).unwrap();
            controller.0.CoreWebView2().unwrap()
        };
        eval(
            &webview,
            r#"document.documentElement.innerHTML = '<head><style>body{margin:0}#task{box-sizing:border-box;height:54px;font-size:16px}#icon{display:inline-block;width:18px;height:18px}</style></head><body><div id="task"><i id="icon"></i>工具学习</div></body>'; true"#,
        );
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            unsafe {
                view3.SetShouldDetectMonitorScaleChanges(false).unwrap();
                view3.SetRasterizationScale(scale).unwrap();
            }
            let before = measure(&webview, scale);
            let lock = RenderScale::freeze(&controller.0).unwrap();
            let snapshot = lock.snapshot;
            lock.persist();
            unsafe {
                SetWindowLongPtrW(windows.note, GWL_STYLE, WS_CHILD as isize);
                SetParent(windows.note, windows.host);
                controller.0.NotifyParentWindowPositionChanged().unwrap();
                // Deliver delayed native DPI/resume messages after reparenting.
                SendMessageW(windows.note, WM_DPICHANGED_AFTERPARENT, 0, 0);
                SendMessageW(windows.note, WM_DISPLAYCHANGE, 0, 0);
            }
            let after = measure(&webview, scale);
            assert_eq!(before, after, "UI changed when embedding at {scale}x");
            assert!((after["row"].as_f64().unwrap() - 54.0 * scale).abs() < 0.01);
            assert!((after["width"].as_f64().unwrap() - 900.0).abs() <= 2.0);
            let mut automatic = BOOL::default();
            unsafe {
                view3
                    .ShouldDetectMonitorScaleChanges(&mut automatic)
                    .unwrap();
            }
            assert!(!automatic.as_bool());
            unsafe {
                SetParent(windows.note, null_mut());
                SetWindowLongPtrW(windows.note, GWL_STYLE, WS_POPUP as isize);
                controller.0.NotifyParentWindowPositionChanged().unwrap();
            }
            snapshot.restore(&controller.0).unwrap();
            assert_eq!(
                before,
                measure(&webview, scale),
                "UI changed on returning at {scale}x"
            );
            let mut bounds = RECT::default();
            unsafe {
                GetClientRect(windows.note, &mut bounds);
            }
            assert_eq!((bounds.right, bounds.bottom), (900, 600));
            eprintln!("{scale}x: {after}");
        }
        // Failed embedding rolls back settings; a successful tray return also
        // restores automatic DPI tracking instead of leaving the app frozen.
        unsafe {
            view3.SetShouldDetectMonitorScaleChanges(true).unwrap();
        }
        {
            let _rollback = RenderScale::freeze(&controller.0).unwrap();
        }
        let mut automatic = BOOL::default();
        unsafe {
            view3
                .ShouldDetectMonitorScaleChanges(&mut automatic)
                .unwrap();
        }
        assert!(automatic.as_bool());
        let lock = RenderScale::freeze(&controller.0).unwrap();
        let snapshot = lock.snapshot;
        unsafe {
            view3
                .ShouldDetectMonitorScaleChanges(&mut automatic)
                .unwrap();
        }
        assert!(
            !automatic.as_bool(),
            "Automatic DPI tracking must stop before SetParent"
        );
        let before = measure(&webview, snapshot.scale);
        lock.persist();
        unsafe {
            SetWindowLongPtrW(windows.note, GWL_STYLE, WS_CHILD as isize);
            SetParent(windows.note, windows.host);
            controller.0.NotifyParentWindowPositionChanged().unwrap();
        }
        assert_eq!(before, measure(&webview, snapshot.scale));
        unsafe {
            SetParent(windows.note, null_mut());
            SetWindowLongPtrW(windows.note, GWL_STYLE, WS_POPUP as isize);
            controller.0.NotifyParentWindowPositionChanged().unwrap();
        }
        snapshot.restore(&controller.0).unwrap();
        unsafe {
            view3
                .ShouldDetectMonitorScaleChanges(&mut automatic)
                .unwrap();
        }
        assert!(automatic.as_bool());
    }
}
