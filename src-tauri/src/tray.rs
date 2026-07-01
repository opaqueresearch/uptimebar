//! System-tray / menubar surface. Aggregate status is shown by re-drawing the
//! brand signal-mark in the status color (green/amber/red) — see `signal_icon`;
//! no icon asset files needed. The monitor list is a frameless webview popover
//! opened on left-click; a small native menu (right-click) holds the app verbs.

use tauri::{
    image::Image,
    menu::{AboutMetadata, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow,
};
use tauri_plugin_autostart::ManagerExt;

use crate::config;
use crate::state::{Aggregate, AppState};

// Muted/pastel status tints for the tray tile — desaturated so the menu-bar icon
// reads softer than a saturated badge. (Full-strength brand colors still used for
// the popover dots; these are tray-only.)
const GREEN: (u8, u8, u8) = (0x6F, 0xC1, 0x99);
const RED: (u8, u8, u8) = (0xE0, 0x8A, 0x8A);
const AMBER: (u8, u8, u8) = (0xE8, 0xC0, 0x77); // degraded/unknown — warmer than gray
const GRAY: (u8, u8, u8) = (0xAF, 0xB1, 0xB8); // idle: no monitors configured
// The signal-mark drawn on top of the tile: brand dark navy. Dark-on-pastel is
// high-contrast on all three status colors (white washed out on pastel-red).
const MARK: (u8, u8, u8) = (0x04, 0x0A, 0x16);

const ICON_SIZE: u32 = 32;
const POPOVER_W: f64 = 340.0;
/// Vertical bounds for the content-fitted popover. MIN keeps the header/toolbar
/// usable even with one monitor; MAX caps it so a large fleet stays on-screen and
/// the list scrolls beyond this (the scroll the wider color band replaced sticky
/// for). The window opens at MAX, then the webview shrinks it to fit its content.
const POPOVER_MIN_H: f64 = 120.0;
const POPOVER_MAX_H: f64 = 560.0;
const POPOVER_H: f64 = POPOVER_MAX_H;

/// Last cursor position the popover was anchored to (set on open). The webview
/// resizes the window after its content renders; we re-anchor to this so the
/// popover stays put under the tray icon instead of drifting as it grows/shrinks.
static LAST_ANCHOR: std::sync::Mutex<Option<(f64, f64)>> = std::sync::Mutex::new(None);

/// Whether the pointer is currently inside the popover (reported by the webview).
/// Guards the focus-loss auto-hide: dragging the popover's own scrollbar briefly
/// drops window focus on macOS, and we must NOT dismiss in that case.
static POINTER_INSIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_pointer_inside(inside: bool) {
    POINTER_INSIDE.store(inside, std::sync::atomic::Ordering::Relaxed);
}

pub fn pointer_inside() -> bool {
    POINTER_INSIDE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    // Accelerators follow macOS muscle memory: ⌘R refresh, ⌘, settings, ⌘Q quit.
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, Some("CmdOrCtrl+R"))?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart =
        CheckMenuItem::with_id(app, "autostart", "Launch at login", true, autostart_on, None::<&str>)?;
    // Native About panel (Apple HIG: About <App> shows version).
    let about = PredefinedMenuItem::about(
        app,
        Some("About UptimeBar"),
        Some(AboutMetadata {
            name: Some("UptimeBar".into()),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            comments: Some("Menu-bar uptime notifier".into()),
            ..Default::default()
        }),
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit UptimeBar", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[&refresh, &settings, &autostart, &sep, &about, &sep2, &quit],
    )?;

    TrayIconBuilder::with_id("main")
        .icon(signal_icon(GRAY))
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
    let _ = tray.set_icon(Some(signal_icon(status_color(agg))));
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
    } else if agg.total() == 0 {
        GRAY // nothing configured yet — neutral
    } else if agg.up == 0 && agg.unknown > 0 {
        AMBER // can't reach anything / all degraded
    } else {
        GREEN
    }
}

/// Draw the tray status icon: a **muted pastel rounded-square (squircle) tile
/// filled in the status color** (green/amber/red) with the brand **signal-mark in
/// dark navy** on top. The filled tile gives the color real visual mass at
/// menu-bar size (thin colored strokes alone were nearly invisible); the dark
/// mark reads high-contrast on all three pastels (white washed out on pastel-red).
/// The squircle echoes the app/DMG icon.
///
/// Mark geometry mirrors assets/icons/uptimebar-template.svg (512 viewBox): center
/// dot r=38, one arc per side at r=150, stroke ~40 — scaled to ICON_SIZE. Rendered
/// with 3× supersampling for crisp anti-aliasing at small size.
fn signal_icon(color: (u8, u8, u8)) -> Image<'static> {
    let s = ICON_SIZE;
    let mut buf = vec![0u8; (s * s * 4) as usize];

    let scale = s as f32 / 512.0;
    let cx = (s as f32 - 1.0) / 2.0;
    let cy = (s as f32 - 1.0) / 2.0;

    // Squircle tile: nearly the full icon, small inset so it doesn't touch edges.
    let tile_half = (s as f32) / 2.0 - 1.5;
    let tile_radius = tile_half * 0.42; // corner rounding

    // Mark, slightly smaller than the SVG so it sits comfortably inside the tile.
    let mark_scale = scale * 0.82;
    // Heavier strokes than the SVG's 40 so the white mark has presence on the tile
    // (thin arcs read as ~1px and disappear); a bigger dot balances it.
    let dot_r = 50.0 * mark_scale;
    let arc_r = 150.0 * mark_scale;
    let half_stroke = (58.0 * mark_scale) / 2.0;
    let arc_half_angle = (116.0_f32).atan2(104.0);

    const SS: u32 = 3;
    let inv_ss = 1.0 / SS as f32;
    let samples = (SS * SS) as f32;

    // Signed-distance to a rounded rectangle centered at (cx,cy); <=0 is inside.
    let sd_squircle = |px: f32, py: f32| -> f32 {
        let qx = (px - cx).abs() - (tile_half - tile_radius);
        let qy = (py - cy).abs() - (tile_half - tile_radius);
        let ox = qx.max(0.0);
        let oy = qy.max(0.0);
        (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - tile_radius
    };

    for y in 0..s {
        for x in 0..s {
            let mut tile_cov = 0.0f32;
            let mut mark_cov = 0.0f32;
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = x as f32 + (sx as f32 + 0.5) * inv_ss - 0.5;
                    let py = y as f32 + (sy as f32 + 0.5) * inv_ss - 0.5;

                    if sd_squircle(px, py) <= 0.0 {
                        tile_cov += 1.0;
                    }

                    let dx = px - cx;
                    let dy = py - cy;
                    let r = (dx * dx + dy * dy).sqrt();
                    let inside = r <= dot_r;
                    let on_arc = if (r - arc_r).abs() <= half_stroke {
                        let ang = dy.atan2(dx);
                        let on_right = ang.abs() <= arc_half_angle;
                        let left_ang = (std::f32::consts::PI - ang.abs()).abs();
                        let on_left = left_ang <= arc_half_angle;
                        on_right || on_left
                    } else {
                        false
                    };
                    if inside || on_arc {
                        mark_cov += 1.0;
                    }
                }
            }

            let tile_a = tile_cov / samples;
            if tile_a <= 0.0 {
                continue; // outside the tile — fully transparent
            }
            let mark_a = mark_cov / samples;
            // Composite the dark navy mark over the colored tile.
            let (r, g, b) = (
                color.0 as f32 * (1.0 - mark_a) + MARK.0 as f32 * mark_a,
                color.1 as f32 * (1.0 - mark_a) + MARK.1 as f32 * mark_a,
                color.2 as f32 * (1.0 - mark_a) + MARK.2 as f32 * mark_a,
            );
            let i = ((y * s + x) * 4) as usize;
            buf[i] = r.round() as u8;
            buf[i + 1] = g.round() as u8;
            buf[i + 2] = b.round() as u8;
            buf[i + 3] = (tile_a * 255.0).round() as u8;
        }
    }
    Image::new_owned(buf, s, s)
}

/// Show the settings window centered on the monitor under the cursor, so it
/// appears on the same screen the user is acting on — not always the primary.
pub fn open_settings(app: &AppHandle) {
    let Some(win) = app.get_webview_window("settings") else {
        return;
    };

    if let Ok(cursor) = app.cursor_position() {
        let mon = app
            .monitor_from_point(cursor.x, cursor.y)
            .ok()
            .flatten()
            .or_else(|| win.current_monitor().ok().flatten());
        if let Some(m) = mon {
            let size = win
                .outer_size()
                .map(|s| (s.width as f64, s.height as f64))
                .unwrap_or((480.0, 600.0));
            let mx = m.position().x as f64;
            let my = m.position().y as f64;
            let mw = m.size().width as f64;
            let mh = m.size().height as f64;
            let x = mx + (mw - size.0) / 2.0;
            let y = my + (mh - size.1) / 2.0;
            let _ = win.set_position(PhysicalPosition::new(x, y));
        }
    }

    let _ = win.show();
    let _ = win.set_focus();
}

fn toggle_popover(app: &AppHandle, cursor: PhysicalPosition<f64>) {
    let Some(win) = app.get_webview_window("popover") else {
        return;
    };
    if win.is_visible().unwrap_or(false) {
        let _ = win.hide();
        return;
    }
    // Remember where we anchored so a later content-fit resize can re-anchor.
    *LAST_ANCHOR.lock().unwrap() = Some((cursor.x, cursor.y));
    position_popover(app, &win, cursor);
    let _ = win.show();
    let _ = win.set_focus();

    // Push the latest snapshot to the popover right away (event delivery to a
    // hidden window is unreliable) so it shows last-known status instantly.
    let state = app.state::<AppState>();
    let _ = app.emit("monitors:updated", state.snapshot_view());

    // Only kick a fresh poll if the last one is stale. The background loop
    // already keeps status within one interval, and the remote monitors don't
    // produce new data faster than that — so re-polling on every open would just
    // hammer provider APIs (open/close/open/close ⇒ a burst of pointless GETs).
    if !state.poll_is_fresh(config::effective_interval(app)) {
        state.refresh.notify_one();
    }
}

fn position_popover(app: &AppHandle, win: &WebviewWindow, cursor: PhysicalPosition<f64>) {
    let size = win
        .outer_size()
        .map(|s| (s.width as f64, s.height as f64))
        .unwrap_or((POPOVER_W, POPOVER_H));
    let (w, h) = size;

    // Clamp against the monitor the CLICK happened on — not the window's current
    // monitor, which is wherever it was last shown (often the primary). Using the
    // wrong monitor's bounds is what pushed the popover onto the other screen.
    // Fall back to the window's current monitor, then a sane default.
    let mon = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| win.current_monitor().ok().flatten());
    let (mon_x, mon_y, mon_w, mon_h) = mon
        .map(|m| {
            (
                m.position().x as f64,
                m.position().y as f64,
                m.size().width as f64,
                m.size().height as f64,
            )
        })
        .unwrap_or((0.0, 0.0, 1920.0, 1080.0));

    let mut x = cursor.x - w / 2.0;
    let min_x = mon_x + 8.0;
    let max_x = mon_x + mon_w - w - 8.0;
    x = x.clamp(min_x, max_x.max(min_x));

    // macOS tray is at the top, Windows at the bottom.
    #[cfg(target_os = "macos")]
    let mut y = cursor.y + 6.0;
    #[cfg(not(target_os = "macos"))]
    let mut y = cursor.y - h - 6.0;

    // Keep it on the click's monitor vertically too.
    let min_y = mon_y + 8.0;
    let max_y = mon_y + mon_h - h - 8.0;
    y = y.clamp(min_y, max_y.max(min_y));

    let _ = win.set_position(PhysicalPosition::new(x, y));
}

/// Resize the popover to fit its content height (clamped), then re-anchor it to
/// the tray icon. Called by the webview after it renders, so short lists get a
/// compact window (no dead space — the reason `position: sticky` looked broken)
/// and long lists cap at MAX and scroll. Width is fixed.
pub fn resize_popover(app: &AppHandle, content_h: f64) {
    let Some(win) = app.get_webview_window("popover") else {
        return;
    };
    let h = content_h.clamp(POPOVER_MIN_H, POPOVER_MAX_H);
    let _ = win.set_size(tauri::LogicalSize::new(POPOVER_W, h));

    // Re-anchor using the cursor we opened at, so growing/shrinking doesn't push
    // the popover off the tray icon. If we never recorded one, leave it in place.
    if let Some((cx, cy)) = *LAST_ANCHOR.lock().unwrap() {
        position_popover(app, &win, PhysicalPosition::new(cx, cy));
    }
}
