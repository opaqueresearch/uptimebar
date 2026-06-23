//! Native OS notifications. Fired only from Rust, only on Up↔Down transitions
//! (the gate lives in `state.rs`). The webview is usually closed, so this never
//! depends on a window being open.

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::providers::MonitorStatus;
use crate::state::Transition;

pub fn fire(app: &AppHandle, t: &Transition) {
    let (title, body) = match t.new_status {
        MonitorStatus::Down => (
            format!("🔴 {} is DOWN", t.monitor_name),
            format!("via {}", t.provider_label),
        ),
        MonitorStatus::Up => (
            format!("🟢 {} recovered", t.monitor_name),
            format!("via {}", t.provider_label),
        ),
        // Transitions are only ever Up/Down (see state.rs).
        _ => return,
    };

    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("notification failed: {e}");
    }
}
