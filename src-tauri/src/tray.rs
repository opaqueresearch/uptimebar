//! System-tray / menubar surface. Aggregate status is shown by swapping a
//! programmatically-drawn colored dot (no icon asset files needed); the monitor
//! list is a frameless webview popover opened on left-click; a small native menu
//! (right-click) holds the app-control verbs.

use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow,
};
use tauri_plugin_autostart::ManagerExt;

use crate::state::{Aggregate, AppState};

const GREEN: (u8, u8, u8) = (0x30, 0xA4, 0x6C);
const RED: (u8, u8, u8) = (0xE5, 0x48, 0x4D);
const GRAY: (u8, u8, u8) = (0x8B, 0x8D, 0x98);

const ICON_SIZE: u32 = 32;
const POPOVER_W: f64 = 340.0;
const POPOVER_H: f64 = 440.0;

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart =
        CheckMenuItem::with_id(app, "autostart", "Launch at login", true, autostart_on, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit UptimeBar", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&refresh, &settings, &autostart, &sep, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(circle_icon(GRAY))
        .icon_as_template(false)
        .tooltip("UptimeBar")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "refresh" => app.state::<AppState>().refresh.notify_one(),
            "settings" => open_settings(app),
            "autostart" => {
                let mgr = app.autolaunch();
                let enabled = mgr.is_enabled().unwrap_or(false);
                let _ = if enabled { mgr.disable() } else { mgr.enable() };
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                toggle_popover(tray.app_handle(), position);
            }
        })
        .build(app)?;
    Ok(())
}

pub fn apply_aggregate(app: &AppHandle, agg: Aggregate) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let _ = tray.set_icon(Some(circle_icon(status_color(agg))));
    let _ = tray.set_icon_as_template(false);
    let tip = if agg.total() == 0 {
        "UptimeBar — no monitors configured".to_string()
    } else {
        format!("{} up · {} down · {} unknown", agg.up, agg.down, agg.unknown)
    };
    let _ = tray.set_tooltip(Some(&tip));
}

fn status_color(agg: Aggregate) -> (u8, u8, u8) {
    if agg.down > 0 {
        RED
    } else if agg.total() == 0 || (agg.up == 0 && agg.unknown > 0) {
        GRAY
    } else {
        GREEN
    }
}

/// Draw a solid colored circle on a transparent square as an RGBA image.
fn circle_icon(color: (u8, u8, u8)) -> Image<'static> {
    let s = ICON_SIZE;
    let mut buf = vec![0u8; (s * s * 4) as usize];
    let center = (s as f32 - 1.0) / 2.0;
    let radius = center - 2.0;
    for y in 0..s {
        for x in 0..s {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            if dx * dx + dy * dy <= radius * radius {
                let i = ((y * s + x) * 4) as usize;
                buf[i] = color.0;
                buf[i + 1] = color.1;
                buf[i + 2] = color.2;
                buf[i + 3] = 255;
            }
        }
    }
    Image::new_owned(buf, s, s)
}

fn open_settings(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

fn toggle_popover(app: &AppHandle, cursor: PhysicalPosition<f64>) {
    let Some(win) = app.get_webview_window("popover") else {
        return;
    };
    if win.is_visible().unwrap_or(false) {
        let _ = win.hide();
        return;
    }
    position_popover(&win, cursor);
    let _ = win.show();
    let _ = win.set_focus();

    // Push the latest snapshot to the popover right away (event delivery to a
    // hidden window is unreliable), and kick a fresh poll for good measure.
    let state = app.state::<AppState>();
    let _ = app.emit("monitors:updated", state.snapshot_view());
    state.refresh.notify_one();
}

fn position_popover(win: &WebviewWindow, cursor: PhysicalPosition<f64>) {
    let size = win
        .outer_size()
        .map(|s| (s.width as f64, s.height as f64))
        .unwrap_or((POPOVER_W, POPOVER_H));
    let w = size.0;
    let (mon_x, mon_w) = win
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| (m.position().x as f64, m.size().width as f64))
        .unwrap_or((0.0, 1920.0));

    let mut x = cursor.x - w / 2.0;
    let min_x = mon_x + 8.0;
    let max_x = mon_x + mon_w - w - 8.0;
    x = x.clamp(min_x, max_x.max(min_x));

    // macOS tray is at the top, Windows at the bottom.
    #[cfg(target_os = "macos")]
    let y = cursor.y + 6.0;
    #[cfg(not(target_os = "macos"))]
    let y = cursor.y - size.1 - 6.0;

    let _ = win.set_position(PhysicalPosition::new(x, y));
}
