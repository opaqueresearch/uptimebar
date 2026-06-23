//! Persistence: non-secret config in `tauri-plugin-store`, API keys in the OS
//! keychain via the `keyring` crate. Secrets are write-only from the UI — they
//! go into the keychain and are never read back out to the frontend.

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
const SECRETS_FILE: &str = "secrets.json";

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

pub fn set_secret(app: &AppHandle, id: &str, secret: &str) -> Result<(), String> {
    let _guard = SECRETS_LOCK.lock().unwrap();
    let mut map = read_secrets(app);
    map.insert(id.to_string(), secret.to_string());
    write_secrets(app, &map)
}

pub fn get_secret(app: &AppHandle, id: &str) -> Result<String, String> {
    let _guard = SECRETS_LOCK.lock().unwrap();
    Ok(read_secrets(app).get(id).cloned().unwrap_or_default())
}

pub fn delete_secret(app: &AppHandle, id: &str) -> Result<(), String> {
    let _guard = SECRETS_LOCK.lock().unwrap();
    let mut map = read_secrets(app);
    map.remove(id);
    write_secrets(app, &map)
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
