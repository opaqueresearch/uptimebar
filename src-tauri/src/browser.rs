//! Detect installed browsers so the user can pick which one opens monitor links
//! (instead of always using the system default). macOS only for now — Windows
//! detection is a future enhancement.
//!
//! The detected *name* is what we hand to `tauri_plugin_opener`'s `open_url`
//! second argument: on macOS that maps to `open -a "<name>"`, which resolves the
//! app by its bundle display name.

use serde::Serialize;

/// A browser the user can choose. `app` is empty for "System default".
#[derive(Debug, Clone, Serialize)]
pub struct Browser {
    /// Label shown in the dropdown, e.g. "Google Chrome".
    pub name: String,
    /// The value passed to the opener (the app name). Empty = system default.
    pub app: String,
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Browser;
    use std::path::Path;

    /// Known browsers as (display name, `.app` bundle filename). We probe a few
    /// standard install locations for each bundle; order here is the order shown.
    const KNOWN: &[(&str, &str)] = &[
        ("Safari", "Safari.app"),
        ("Google Chrome", "Google Chrome.app"),
        ("Brave Browser", "Brave Browser.app"),
        ("Firefox", "Firefox.app"),
        ("Microsoft Edge", "Microsoft Edge.app"),
        ("Arc", "Arc.app"),
        ("Vivaldi", "Vivaldi.app"),
        ("Opera", "Opera.app"),
        ("Chromium", "Chromium.app"),
    ];

    fn bundle_exists(bundle: &str) -> bool {
        // System apps live in /Applications; some browsers install per-user.
        let home = std::env::var("HOME").unwrap_or_default();
        let candidates = [
            format!("/Applications/{bundle}"),
            format!("{home}/Applications/{bundle}"),
        ];
        candidates.iter().any(|p| Path::new(p).exists())
    }

    pub fn detect() -> Vec<Browser> {
        let mut out = vec![Browser {
            name: "System default".to_string(),
            app: String::new(),
        }];
        for (name, bundle) in KNOWN {
            if bundle_exists(bundle) {
                out.push(Browser {
                    name: name.to_string(),
                    app: name.to_string(),
                });
            }
        }
        out
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::Browser;
    /// Other platforms: only the system default for now.
    pub fn detect() -> Vec<Browser> {
        vec![Browser {
            name: "System default".to_string(),
            app: String::new(),
        }]
    }
}

/// Installed browsers, "System default" always first.
pub fn detect() -> Vec<Browser> {
    platform::detect()
}
