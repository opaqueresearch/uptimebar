//! The `#[tauri::command]` surface invoked from the popover + settings UIs. All
//! mutations go through here so Rust owns the invariants (persist → rebuild
//! registry → trigger an immediate poll). Secrets are write-only.

use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::config;
use crate::providers::{self, ProviderConfig};
use crate::state::{AppState, MonitorView};

/// Open a URL in the user's chosen browser (monitor + docs links). Falls back to
/// the system default when no browser is configured, or if launching the chosen
/// one fails (e.g. it was uninstalled since being selected).
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    let chosen = config::browser_app(&app);
    if !chosen.is_empty() {
        if app.opener().open_url(&url, Some(chosen.as_str())).is_ok() {
            return Ok(());
        }
        log::warn!("opening in '{chosen}' failed; falling back to system default");
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Browsers detected on this machine, for the settings dropdown.
#[tauri::command]
pub fn get_browsers() -> Vec<crate::browser::Browser> {
    crate::browser::detect()
}

/// The currently chosen browser app name ("" = system default).
#[tauri::command]
pub fn get_browser_app(app: AppHandle) -> String {
    config::browser_app(&app)
}

#[tauri::command]
pub fn set_browser_app(app: AppHandle, value: String) -> Result<(), String> {
    config::set_browser_app(&app, &value)
}

#[tauri::command]
pub fn get_monitors(state: State<AppState>) -> Vec<MonitorView> {
    state.snapshot_view()
}

/// On-demand rich detail (latency, uptime %, …) for one provider — called when
/// the popover opens, not on every poll. Returns provider-native JSON or null.
#[tauri::command]
pub async fn get_provider_detail(
    app: AppHandle,
    provider_id: String,
) -> Result<Option<serde_json::Value>, String> {
    // Clone the Arc out and drop the lock guard before awaiting.
    let provider = {
        let state = app.state::<AppState>();
        let reg = state.registry.read().unwrap();
        reg.iter().find(|p| p.id() == provider_id).cloned()
    };
    match provider {
        Some(p) => p.fetch_detail().await.map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

#[tauri::command]
pub fn get_providers(state: State<AppState>) -> Vec<ProviderConfig> {
    state.configs.lock().unwrap().clone()
}

#[tauri::command]
pub fn get_provider_kinds() -> Vec<providers::ProviderMeta> {
    providers::kinds_meta()
}

#[tauri::command]
pub fn provider_has_secret(app: AppHandle, id: String) -> bool {
    config::get_secret(&app, &id)
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

#[tauri::command]
pub fn refresh_now(state: State<AppState>) {
    state.refresh.notify_one();
}

#[tauri::command]
pub fn open_settings(app: AppHandle) {
    // Single source of truth: center on the active monitor (see tray.rs).
    crate::tray::open_settings(&app);
}

/// Dismiss the popover (hide, not destroy) — used by Esc.
#[tauri::command]
pub fn close_popover(app: AppHandle) {
    if let Some(win) = app.get_webview_window("popover") {
        let _ = win.hide();
    }
}

/// Fit the popover window to its rendered content height (clamped), called by the
/// webview after each draw. Keeps short lists compact and long ones scrollable.
#[tauri::command]
pub fn resize_popover(app: AppHandle, height: f64) {
    crate::tray::resize_popover(&app, height);
}

/// The webview reports whether the pointer is currently inside the popover. Used
/// to suppress the focus-loss auto-hide while the user is interacting with the
/// window's own scrollbar (which briefly drops focus on macOS — a scrollbar drag
/// would otherwise dismiss the popover).
#[tauri::command]
pub fn set_pointer_inside(app: AppHandle, inside: bool) {
    crate::tray::set_pointer_inside(inside);
    // If the pointer leaves while the popover is NOT focused, the user has moved
    // on (e.g. dragged the scrollbar, then left to another app) and the earlier
    // Focused(false) was suppressed — so honor the dismiss now.
    if !inside {
        if let Some(win) = app.get_webview_window("popover") {
            if !win.is_focused().unwrap_or(true) {
                let _ = win.hide();
            }
        }
    }
}

/// Result of a "Test connection": how many monitors, plus an optional advisory
/// note (e.g. a Healthchecks read-only key that disables per-check deep-links).
#[derive(serde::Serialize)]
pub struct TestResult {
    pub count: usize,
    pub note: Option<String>,
}

/// Build a provider ad-hoc and run a single fetch — used by the "Test
/// connection" button before committing config.
#[tauri::command]
pub async fn test_provider(
    app: AppHandle,
    mut config: ProviderConfig,
    secret: String,
) -> Result<TestResult, String> {
    let http = app.state::<AppState>().http.clone();

    // The key field is blank when editing an existing provider (secrets are
    // write-only). Fall back to the stored keychain key in that case.
    let secret = if secret.is_empty() && !config.id.is_empty() {
        config::get_secret(&app, &config.id).unwrap_or_default()
    } else {
        secret
    };

    if config.id.is_empty() {
        config.id = "__test__".to_string();
    }
    let provider = providers::build(&config, secret, http).map_err(|e| e.to_string())?;
    let monitors = provider.fetch_monitors().await.map_err(|e| e.to_string())?;

    // Healthchecks read-only keys redact the uuid (and ping_url/update_url), so
    // no monitor gets a per-check deep-link — every detail_url is just the base.
    // Detect that and advise the user to use the read-write key for links.
    let note = if config.kind == "healthchecks" && !monitors.is_empty() {
        let base = config
            .base_url
            .as_deref()
            .unwrap_or("https://healthchecks.io")
            .trim_end_matches('/');
        let none_deeplink = monitors
            .iter()
            .all(|m| !m.detail_url.as_deref().is_some_and(|u| u.contains("/checks/")));
        if none_deeplink {
            Some(format!(
                "Read-only key detected — per-check links go to the dashboard. \
                 Use the read-write API key ({base}) for per-check deep-links."
            ))
        } else {
            None
        }
    } else {
        None
    };

    Ok(TestResult {
        count: monitors.len(),
        note,
    })
}

#[tauri::command]
pub fn upsert_provider(
    app: AppHandle,
    mut config: ProviderConfig,
    secret: Option<String>,
) -> Result<ProviderConfig, String> {
    if config.id.is_empty() {
        config.id = uuid::Uuid::new_v4().to_string();
    }
    if let Some(sec) = secret {
        if !sec.is_empty() {
            config::set_secret(&app, &config.id, &sec)?;
        }
    }
    {
        let state = app.state::<AppState>();
        let mut configs = state.configs.lock().unwrap();
        if let Some(existing) = configs.iter_mut().find(|c| c.id == config.id) {
            *existing = config.clone();
        } else {
            configs.push(config.clone());
        }
        config::save_configs(&app, configs.as_slice())?;
    }
    config::rebuild_registry(&app)?;
    app.state::<AppState>().refresh.notify_one();
    Ok(config)
}

#[tauri::command]
pub fn delete_provider(app: AppHandle, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut configs = state.configs.lock().unwrap();
        configs.retain(|c| c.id != id);
        config::save_configs(&app, configs.as_slice())?;
    }
    config::delete_secret(&app, &id)?;
    config::rebuild_registry(&app)?;
    app.state::<AppState>().refresh.notify_one();
    Ok(())
}
