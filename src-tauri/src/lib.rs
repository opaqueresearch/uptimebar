mod browser;
mod commands;
mod config;
mod notify;
mod poller;
mod providers;
mod state;
mod tray;

use tauri::{Manager, WindowEvent};

use state::AppState;

/// HTTP User-Agent sent to every provider, e.g. "UptimeBar/0.4.0 (macOS; aarch64)".
/// The OS/arch suffix lets Watch4.me segment funnel traffic by platform (issue #3).
fn user_agent() -> String {
    // std::env::consts::OS is lowercase ("macos"/"windows"/"linux"); map to nicer
    // display names. ARCH ("aarch64"/"x86_64") is already presentable.
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    format!(
        "UptimeBar/{} ({}; {})",
        env!("CARGO_PKG_VERSION"),
        os,
        std::env::consts::ARCH,
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Logs to stderr (visible in `tauri dev`). Override with RUST_LOG=debug.
    let _ = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("uptimebar_lib=info"),
    )
    .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_monitors,
            commands::get_provider_detail,
            commands::monitor_action,
            commands::get_providers,
            commands::get_provider_kinds,
            commands::provider_has_secret,
            commands::refresh_now,
            commands::open_settings,
            commands::close_popover,
            commands::resize_popover,
            commands::set_pointer_inside,
            commands::open_url,
            commands::get_browsers,
            commands::get_browser_app,
            commands::set_browser_app,
            commands::test_provider,
            commands::upsert_provider,
            commands::delete_provider,
        ])
        .setup(|app| {
            // Tray-only app: no Dock icon on macOS.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();

            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent(user_agent())
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());

            let configs = config::load_configs(&handle);
            app.manage(AppState::new(http, configs));

            let _ = config::rebuild_registry(&handle);
            tray::build_tray(&handle)?;

            // Background polling loop, independent of any window.
            tauri::async_runtime::spawn(poller::run(handle.clone()));
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Dismiss the popover when it loses focus — UNLESS the pointer is still
            // inside it. Dragging the popover's own scrollbar briefly drops window
            // focus on macOS; without this guard that would dismiss the popover.
            WindowEvent::Focused(false) if window.label() == "popover" => {
                if crate::tray::pointer_inside() {
                    log::debug!("popover Focused(false) ignored (pointer inside)");
                } else {
                    let _ = window.hide();
                }
            }
            // Keep the settings window alive (hide instead of destroy) so it can reopen.
            WindowEvent::CloseRequested { api, .. } if window.label() == "settings" => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
