//! Shared application state. The Rust core is the source of truth; the webview
//! UI is an ephemeral view onto this. Transition detection lives here — it is the
//! single gate that decides when a notification fires.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

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
    /// User-chosen left-bar color (hex) from the provider config, if set. The UI
    /// falls back to the kind's default color when this is None.
    pub provider_color: Option<String>,
    /// Provider-native public id (Watch4.me), for keying monitor actions.
    pub public_id: Option<String>,
    /// Whether the monitor's alerts are muted at the provider.
    pub is_muted: bool,
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

/// Cached on-demand provider detail (latency/uptime). The popover re-requests
/// this on every open/focus, but the remote data only changes at the poll
/// cadence — so we serve a recent result and de-dupe concurrent fetches instead
/// of hitting the provider API on every open.
#[derive(Default)]
pub struct DetailCache {
    /// When the last fetch *completed* (success or failure). `None` = never.
    pub fetched_at: Option<Instant>,
    /// Last successful detail payload, replayed on cache hits.
    pub value: Option<serde_json::Value>,
    /// A fetch is currently in flight for this provider.
    pub in_flight: bool,
}

/// What `begin_detail` decided a caller should do.
pub enum DetailDecision {
    /// Serve this cached value (fresh, or a fetch is already in flight).
    Hit(Option<serde_json::Value>),
    /// Caller should fetch; it has been marked in-flight.
    Fetch,
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
    /// When the last full poll completed — gates redundant open-driven polls.
    pub last_poll: Mutex<Option<Instant>>,
    /// Per-provider detail cache, keyed by provider id.
    pub detail_cache: Mutex<HashMap<String, DetailCache>>,
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
            last_poll: Mutex::new(None),
            detail_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Record that a full poll just completed.
    pub fn mark_polled(&self) {
        *self.last_poll.lock().unwrap() = Some(Instant::now());
    }

    /// True if a poll completed within `ttl`. The popover-open path uses this to
    /// skip a redundant immediate poll: the background loop already keeps status
    /// within one interval, so re-polling on open would only hammer the API.
    pub fn poll_is_fresh(&self, ttl: Duration) -> bool {
        self.last_poll
            .lock()
            .unwrap()
            .map(|t| t.elapsed() < ttl)
            .unwrap_or(false)
    }

    /// Decide whether a detail request should hit the network. Returns a cached
    /// value when one was fetched within `ttl` (or a fetch is already in flight,
    /// to de-dupe overlapping opens); otherwise claims the in-flight slot and
    /// tells the caller to `Fetch`. `force` bypasses the freshness check (manual
    /// refresh) but still de-dupes against an in-flight fetch.
    pub fn begin_detail(&self, id: &str, ttl: Duration, force: bool) -> DetailDecision {
        let mut cache = self.detail_cache.lock().unwrap();
        if let Some(e) = cache.get(id) {
            let fresh = !force
                && e.fetched_at.map(|t| t.elapsed() < ttl).unwrap_or(false);
            if fresh || e.in_flight {
                return DetailDecision::Hit(e.value.clone());
            }
        }
        cache.entry(id.to_string()).or_default().in_flight = true;
        DetailDecision::Fetch
    }

    /// Release the in-flight slot and record the outcome. The timestamp is
    /// stamped on success *and* failure so errors back off for `ttl` too; the
    /// value is only replaced on success (a failure keeps the last good detail).
    pub fn finish_detail(&self, id: &str, value: Option<serde_json::Value>) {
        let mut cache = self.detail_cache.lock().unwrap();
        let e = cache.entry(id.to_string()).or_default();
        e.in_flight = false;
        e.fetched_at = Some(Instant::now());
        if value.is_some() {
            e.value = value;
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

    /// Apply the authoritative post-action state to the matching row (by
    /// provider + public_id), so the UI reflects a pause/mute immediately instead
    /// of waiting for the next poll. `is_paused=true` maps the status to Paused;
    /// `false` leaves the last non-paused status for the poller to reconcile.
    pub fn apply_action_outcome(
        &self,
        provider_id: &str,
        public_id: &str,
        is_paused: Option<bool>,
        is_muted: Option<bool>,
    ) {
        let mut rows = self.rows.lock().unwrap();
        for row in rows.values_mut() {
            if row.provider_id == provider_id
                && row.monitor.public_id.as_deref() == Some(public_id)
            {
                if let Some(muted) = is_muted {
                    row.monitor.is_muted = muted;
                }
                if let Some(paused) = is_paused {
                    if paused {
                        row.monitor.status = MonitorStatus::Paused;
                    } else if row.monitor.status == MonitorStatus::Paused {
                        // Resumed: drop back to Unknown until the next poll gives
                        // the real up/down (avoids showing a stale Paused).
                        row.monitor.status = MonitorStatus::Unknown;
                    }
                }
                break;
            }
        }
    }

    /// Remove all rows belonging to providers no longer in the registry.
    pub fn prune_to(&self, live_provider_ids: &[String]) {
        let mut rows = self.rows.lock().unwrap();
        rows.retain(|_, r| live_provider_ids.iter().any(|id| id == &r.provider_id));
        let mut failures = self.failures.lock().unwrap();
        failures.retain(|id, _| live_provider_ids.iter().any(|x| x == id));
        let mut detail = self.detail_cache.lock().unwrap();
        detail.retain(|id, _| live_provider_ids.iter().any(|x| x == id));
    }

    pub fn snapshot_view(&self) -> Vec<MonitorView> {
        let rows = self.rows.lock().unwrap();
        // Per-provider chosen colors, looked up by provider id (cheap: few configs).
        let colors: HashMap<String, Option<String>> = self
            .configs
            .lock()
            .unwrap()
            .iter()
            .map(|c| (c.id.clone(), c.color.clone()))
            .collect();
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
                provider_color: colors.get(&r.provider_id).cloned().flatten(),
                public_id: r.monitor.public_id.clone(),
                is_muted: r.monitor.is_muted,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        AppState::new(reqwest::Client::new(), Vec::new())
    }

    #[test]
    fn poll_freshness_tracks_last_poll() {
        let s = state();
        // Never polled ⇒ never fresh, so an open would force a poll.
        assert!(!s.poll_is_fresh(Duration::from_secs(60)));
        s.mark_polled();
        assert!(s.poll_is_fresh(Duration::from_secs(60)));
        // A zero TTL is always stale, even right after polling.
        assert!(!s.poll_is_fresh(Duration::ZERO));
    }

    #[test]
    fn first_detail_request_fetches_then_serves_cache() {
        let s = state();
        let ttl = Duration::from_secs(60);
        // Cold cache ⇒ the caller is told to fetch.
        assert!(matches!(s.begin_detail("p", ttl, false), DetailDecision::Fetch));
        s.finish_detail("p", Some(serde_json::json!({"v": 1})));
        // Within TTL the next open is served from cache — no network.
        match s.begin_detail("p", ttl, false) {
            DetailDecision::Hit(Some(v)) => assert_eq!(v["v"], 1),
            other => panic!("expected cache hit, got {:?}", matches!(other, DetailDecision::Fetch)),
        }
    }

    #[test]
    fn force_bypasses_freshness_but_cache_replays_value() {
        let s = state();
        let ttl = Duration::from_secs(60);
        s.begin_detail("p", ttl, false);
        s.finish_detail("p", Some(serde_json::json!({"v": 1})));
        // Forced refresh ignores the fresh cache and re-fetches.
        assert!(matches!(s.begin_detail("p", ttl, true), DetailDecision::Fetch));
    }

    #[test]
    fn in_flight_request_is_not_duplicated() {
        let s = state();
        let ttl = Duration::from_secs(60);
        // First caller claims the in-flight slot (told to fetch)…
        assert!(matches!(s.begin_detail("p", ttl, false), DetailDecision::Fetch));
        // …a concurrent open gets a cache hit (stale/None) instead of a 2nd GET,
        // even with force set.
        assert!(matches!(s.begin_detail("p", ttl, true), DetailDecision::Hit(_)));
        // Once the fetch finishes, the slot is released and a stale TTL re-fetches.
        s.finish_detail("p", None);
        assert!(matches!(s.begin_detail("p", Duration::ZERO, false), DetailDecision::Fetch));
    }
}
