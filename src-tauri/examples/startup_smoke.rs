//! Run separately: real Tauri event-loop regression, isolated from user data.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[path = "../src/desktop.rs"]
#[allow(dead_code)]
mod desktop;
#[cfg(windows)]
#[path = "../src/desktop_rendering.rs"]
#[allow(dead_code)]
mod desktop_rendering;

use std::{
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use tauri::Manager;

#[derive(Default)]
struct SmokeState {
    startup_visible: AtomicBool,
    started: AtomicBool,
    delivered: AtomicUsize,
}

#[tauri::command]
fn smoke_ack(state: tauri::State<SmokeState>, value: bool) {
    assert!(!value);
    state.delivered.fetch_add(1, Ordering::SeqCst);
}

#[tauri::command]
fn smoke_failed(app: tauri::AppHandle, error: String) {
    eprintln!("FAIL: {error}");
    app.exit(1);
}

#[tauri::command]
fn smoke_ready(window: tauri::WebviewWindow) {
    if window
        .state::<SmokeState>()
        .started
        .swap(true, Ordering::SeqCst)
    {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let result: Result<(), String> = async {
            let start = Instant::now();
            while !window
                .state::<SmokeState>()
                .startup_visible
                .load(Ordering::SeqCst)
            {
                if start.elapsed() > Duration::from_secs(5) {
                    return Err("Startup did not show the note".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(window.is_visible().unwrap());
            eprintln!("startup: note visible, JavaScript listener registered");
            for cycle in 1..=10 {
                let previous = window
                    .state::<SmokeState>()
                    .delivered
                    .load(Ordering::SeqCst);
                window.hide().unwrap();
                assert!(!window.is_visible().unwrap());
                desktop::restore(&window).await?;
                assert!(window.is_visible().unwrap());
                let start = Instant::now();
                while window
                    .state::<SmokeState>()
                    .delivered
                    .load(Ordering::SeqCst)
                    <= previous
                {
                    if start.elapsed() > Duration::from_secs(5) {
                        return Err(format!("JS event not delivered in cycle {cycle}"));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                eprintln!("restore cycle {cycle}: visible and JS event delivered");
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => window.app_handle().exit(0),
            Err(error) => {
                eprintln!("FAIL: {error}");
                window.app_handle().exit(1);
            }
        }
    });
}

fn main() {
    let profile = tempfile::tempdir().unwrap();
    let mut context = tauri::generate_context!();
    let config = context.config_mut();
    config.identifier = "com.desktop.todo.startup-regression".into();
    config.app.windows.retain(|window| window.label == "main");
    config.app.windows[0].data_directory = Some(profile.path().to_path_buf());
    let app = tauri::Builder::default()
        .manage(desktop::DesktopState::default())
        .manage(SmokeState::default())
        .invoke_handler(tauri::generate_handler![smoke_ack, smoke_ready, smoke_failed])
        .on_page_load(|webview, payload| {
            if webview.label() == "main" && payload.event() == tauri::webview::PageLoadEvent::Finished {
                // Crucial regression: the real app has JS listeners. Without
                // one, emit() can skip the dispatcher and hide the deadlock.
                webview.eval(r#"(async () => {
                    const api = window.__TAURI_INTERNALS__;
                    await api.invoke('plugin:event|listen', {
                        event: 'desktop-mode', target: {kind: 'Any'},
                        handler: api.transformCallback(event => api.invoke('smoke_ack', {value: event.payload}))
                    });
                    await api.invoke('smoke_ready');
                })().catch(error => window.__TAURI_INTERNALS__.invoke('smoke_failed', {error: String(error)}));"#).unwrap();
            }
        })
        .setup(|app| {
            // Match the production setup: show only the note, after creating
            // the initially hidden windows; settings/history stay hidden.
            let window = app.get_webview_window("main").unwrap();
            tauri::async_runtime::spawn(async move {
                match desktop::restore(&window).await {
                    Ok(()) => window.state::<SmokeState>().startup_visible.store(true, Ordering::SeqCst),
                    Err(error) => { eprintln!("FAIL: {error}"); window.app_handle().exit(1); }
                }
            });
            Ok(())
        })
        .build(context)
        .unwrap();
    app.run(|_, _| {});
}
