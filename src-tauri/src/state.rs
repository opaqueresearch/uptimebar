//! Shared application state. The Rust core is the source of truth; the webview
//! UI is an ephemeral view onto this. Transition detection lives here — it is the
//! single gate that decides when a notification fires.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use tokio::sync::Notify;

use crate::providers::{Monitor, MonitorStatus, Provider, ProviderConfig};

/// Number of consecutive provider fetch failures before its monitors are shown
/// as Unknown (rather than letting a flaky API masquerade as an outage).
pub const FAILURE_THRESHOLD: u32 = 2;

pub struct MonitorRow {
    pub provider_id: String,
    pub provider_label: String,
    pub provider_kind: String,
    pub monitor: Monitor,
    /// Last *solid* (Up/Down) status observed — the basis for transition edges.
    pub last_solid: Option<MonitorStatus>,
}

/// A serializable view of a monitor row for the frontend.
#[derive(Clone, serde::Serialize)]
pub struct MonitorView {
    pub key: String,
    pub provider_label: String,
    pub provider_kind: String,
    pub name: String,
    pub status: MonitorStatus,
    pub last_checked: Option<String>,
    pub url: Option<String>,
    pub detail_url: Option<String>,
    /// ISO-8601 when the current status began, if known. The UI computes
    /// "down for Xm" / "up since…" from this against its own clock.
    pub state_since: Option<String>,
}

/// Aggregate counts used to drive the tray icon + tooltip.
#[derive(Clone, Copy, Default, serde::Serialize)]
pub struct Aggregate {
    pub up: usize,
    pub down: usize,
    pub paused: usize,
    pub unknown: usize,
}

impl Aggregate {
    pub fn total(&self) -> usize {
        self.up + self.down + self.paused + self.unknown
    }
}

/// A state edge worth notifying the user about.
pub struct Transition {
    pub monitor_name: String,
    pub provider_label: String,
    pub new_status: MonitorStatus,
}

pub struct AppState {
    pub http: reqwest::Client,
    /// Live provider adapters, rebuilt whenever config changes.
    pub registry: RwLock<Vec<Arc<dyn Provider>>>,
    /// Persisted, non-secret provider configs (mirror of the store).
    pub configs: Mutex<Vec<ProviderConfig>>,
    /// Current monitor rows keyed by "{provider_id}:{monitor_id}".
    pub rows: Mutex<HashMap<String, MonitorRow>>,
    /// Per-provider consecutive failure counters.
    pub failures: Mutex<HashMap<String, u32>>,
    /// Signalled to trigger an immediate poll.
    pub refresh: Notify,
}

fn key(provider_id: &str, monitor_id: &str) -> String {
    format!("{provider_id}:{monitor_id}")
}

fn is_solid(s: MonitorStatus) -> bool {
    matches!(s, MonitorStatus::Up | MonitorStatus::Down)
}

impl AppState {
    pub fn new(http: reqwest::Client, configs: Vec<ProviderConfig>) -> Self {
        Self {
            http,
            registry: RwLock::new(Vec::new()),
            configs: Mutex::new(configs),
            rows: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
            refresh: Notify::new(),
        }
    }

    /// Apply a successful provider fetch. Returns any Up↔Down transitions.
    /// First observation of a monitor sets a silent baseline (no notification).
    pub fn apply_success(
        &self,
        provider_id: &str,
        provider_label: &str,
        provider_kind: &str,
        monitors: Vec<Monitor>,
    ) -> Vec<Transition> {
        self.failures.lock().unwrap().insert(provider_id.to_string(), 0);

        let mut transitions = Vec::new();
        let mut rows = self.rows.lock().unwrap();
        let mut seen = Vec::with_capacity(monitors.len());

        for m in monitors {
            let k = key(provider_id, &m.id);
            seen.push(k.clone());
            let new_status = m.status;

            match rows.get_mut(&k) {
                Some(row) => {
                    if is_solid(new_status) {
                        if let Some(prev) = row.last_solid {
                            if prev != new_status {
                                transitions.push(Transition {
                                    monitor_name: m.name.clone(),
                                    provider_label: provider_label.to_string(),
                                    new_status,
                                });
                            }
                        }
                        row.last_solid = Some(new_status);
                    }
                    row.monitor = m;
                }
                None => {
                    rows.insert(
                        k,
                        MonitorRow {
                            provider_id: provider_id.to_string(),
                            provider_label: provider_label.to_string(),
                            provider_kind: provider_kind.to_string(),
                            last_solid: is_solid(new_status).then_some(new_status),
                            monitor: m,
                        },
                    );
                }
            }
        }

        // Drop rows for monitors that vanished from this provider.
        rows.retain(|_, r| r.provider_id != provider_id || seen.iter().any(|k| k == &key(provider_id, &r.monitor.id)));

        transitions
    }

    /// Record a provider fetch failure; once past the threshold, its monitors
    /// are shown as Unknown. Going Unknown never notifies.
    pub fn apply_failure(&self, provider_id: &str) {
        let mut failures = self.failures.lock().unwrap();
        let count = failures.entry(provider_id.to_string()).or_insert(0);
        *count += 1;
        if *count >= FAILURE_THRESHOLD {
            let mut rows = self.rows.lock().unwrap();
            for row in rows.values_mut() {
                if row.provider_id == provider_id {
                    row.monitor.status = MonitorStatus::Unknown;
                }
            }
        }
    }

    /// Remove all rows belonging to providers no longer in the registry.
    pub fn prune_to(&self, live_provider_ids: &[String]) {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|_, r| live_provider_ids.iter().any(|id| id == &r.provider_id));
        let mut failures = self.failures.lock().unwrap();
        failures.retain(|id, _| live_provider_ids.iter().any(|x| x == id));
    }

    pub fn snapshot_view(&self) -> Vec<MonitorView> {
        let rows = self.rows.lock().unwrap();
        let mut out: Vec<MonitorView> = rows
            .iter()
            .map(|(k, r)| MonitorView {
                key: k.clone(),
                provider_label: r.provider_label.clone(),
                provider_kind: r.provider_kind.clone(),
                name: r.monitor.name.clone(),
                status: r.monitor.status,
                last_checked: r.monitor.last_checked.clone(),
                url: r.monitor.url.clone(),
                detail_url: r.monitor.detail_url.clone(),
                state_since: r.monitor.state_since.clone(),
            })
            .collect();
        // Down first, then unknown, paused, up; alphabetical within.
        out.sort_by(|a, b| {
            rank(a.status)
                .cmp(&rank(b.status))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }

    pub fn aggregate(&self) -> Aggregate {
        let rows = self.rows.lock().unwrap();
        let mut agg = Aggregate::default();
        for r in rows.values() {
            match r.monitor.status {
                MonitorStatus::Up => agg.up += 1,
                MonitorStatus::Down => agg.down += 1,
                MonitorStatus::Paused => agg.paused += 1,
                MonitorStatus::Unknown => agg.unknown += 1,
            }
        }
        agg
    }
}

fn rank(s: MonitorStatus) -> u8 {
    match s {
        MonitorStatus::Down => 0,
        MonitorStatus::Unknown => 1,
        MonitorStatus::Paused => 2,
        MonitorStatus::Up => 3,
    }
}
