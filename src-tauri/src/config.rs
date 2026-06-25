//! Persistence: non-secret config in `tauri-plugin-store`. API keys are
//! write-only from the UI — stored, never read back out to the frontend.
//!
//! Where keys live depends on the build:
//! - **Release builds** use the OS keychain (macOS Keychain / Windows Credential
//!   Manager) via the `keyring` crate — encrypted at rest, per-app access.
//! - **Debug builds** (`tauri dev`) use a 0600 plaintext file. The keychain
//!   binds each item to the app's code signature, so an unsigned dev build that
//!   is recompiled can't read back what a previous build wrote — keys appeared
//!   "not saved" after every rebuild. The file is reliable across rebuilds.
//!
//! On release, `get_secret` falls back to the file and migrates it into the
//! keychain, so a user upgrading from a file-based build doesn't lose keys.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

use crate::providers::{self, ProviderConfig};
use crate::state::AppState;

const STORE_FILE: &str = "settings.json";
const PROVIDERS_KEY: &str = "providers";
const INTERVAL_KEY: &str = "poll_interval_secs";
const BROWSER_KEY: &str = "browser_app";
const SECRETS_FILE: &str = "secrets.json";

/// Keychain service name (the app identifier); each provider id is an account.
const KEYCHAIN_SERVICE: &str = "app.uptimebar";

/// True when keys should live in the OS keychain (release builds only).
fn use_keychain() -> bool {
    !cfg!(debug_assertions)
}

pub const DEFAULT_INTERVAL_SECS: u64 = 60;

// Secrets live in an owner-only (0600) file in the app data dir, NOT the OS
// keychain. The keychain binds each item to the app's code signature, so an
// unsigned dev build that is recompiled can't read back what a previous build
// wrote — keys appeared "not saved" after every rebuild. A permissioned file is
// reliable across rebuilds (signed or not); a signed release can move these
// back into the keychain. The whole secret API is centralized here.
static SECRETS_LOCK: Mutex<()> = Mutex::new(());

fn secrets_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(SECRETS_FILE))
}

fn read_secrets(app: &AppHandle) -> HashMap<String, String> {
    secrets_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_secrets(app: &AppHandle, map: &HashMap<String, String>) -> Result<(), String> {
    let path = secrets_path(app)?;
    let json = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn keychain_entry(id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, id).map_err(|e| e.to_string())
}

pub fn set_secret(app: &AppHandle, id: &str, secret: &str) -> Result<(), String> {
    let _guard = SECRETS_LOCK.lock().unwrap();
    if use_keychain() {
        return keychain_entry(id)?
            .set_password(secret)
            .map_err(|e| e.to_string());
    }
    let mut map = read_secrets(app);
    map.insert(id.to_string(), secret.to_string());
    write_secrets(app, &map)
}

pub fn get_secret(app: &AppHandle, id: &str) -> Result<String, String> {
    let _guard = SECRETS_LOCK.lock().unwrap();
    if use_keychain() {
        match keychain_entry(id)?.get_password() {
            Ok(s) => return Ok(s),
            Err(keyring::Error::NoEntry) => {
                // Migrate from a prior file-based build: read the file, move it
                // into the keychain, and return it.
                if let Some(s) = read_secrets(app).get(id).cloned() {
                    if let Ok(entry) = keychain_entry(id) {
                        let _ = entry.set_password(&s);
                    }
                    return Ok(s);
                }
                return Ok(String::new());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(read_secrets(app).get(id).cloned().unwrap_or_default())
}

pub fn delete_secret(app: &AppHandle, id: &str) -> Result<(), String> {
    let _guard = SECRETS_LOCK.lock().unwrap();
    if use_keychain() {
        match keychain_entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    // Always clear the file copy too (covers migrated entries / mode switches).
    let mut map = read_secrets(app);
    if map.remove(id).is_some() {
        write_secrets(app, &map)?;
    }
    Ok(())
}

pub fn load_configs(app: &AppHandle) -> Vec<ProviderConfig> {
    let store = match app.store(STORE_FILE) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    store
        .get(PROVIDERS_KEY)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub fn save_configs(app: &AppHandle, configs: &[ProviderConfig]) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        PROVIDERS_KEY,
        serde_json::to_value(configs).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())
}

pub fn poll_interval(app: &AppHandle) -> u64 {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(INTERVAL_KEY))
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_INTERVAL_SECS)
}

/// The app name to open links with (e.g. "Google Chrome"). Empty string means
/// "use the system default browser".
pub fn browser_app(app: &AppHandle) -> String {
    app.store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(BROWSER_KEY))
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

pub fn set_browser_app(app: &AppHandle, value: &str) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(BROWSER_KEY, serde_json::Value::String(value.to_string()));
    store.save().map_err(|e| e.to_string())
}

/// Rebuild the live provider registry from the persisted configs + keychain
/// secrets, then prune stale monitor rows. Call after any config change.
pub fn rebuild_registry(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let configs = state.configs.lock().unwrap().clone();

    let mut providers = Vec::new();
    let mut live_ids = Vec::new();
    for cfg in &configs {
        let secret = get_secret(app, &cfg.id).unwrap_or_default();
        match providers::build(cfg, secret, state.http.clone()) {
            Ok(p) => {
                live_ids.push(cfg.id.clone());
                providers.push(p);
            }
            Err(e) => log::warn!("skipping provider {}: {e}", cfg.label),
        }
    }

    *state.registry.write().unwrap() = providers;
    state.prune_to(&live_ids);
    Ok(())
}
