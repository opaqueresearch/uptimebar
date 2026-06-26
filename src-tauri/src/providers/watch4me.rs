//! Watch4.me adapter.
//!
//! Polls the purpose-built status endpoint with conditional caching (ETag/304):
//!
//!   GET {base}/api/v1/monitors/status   (Authorization: Bearer w4m_<token>)
//!   -> 200 { monitors: [...] } + ETag header  (full list)
//!   -> 304 (empty body)                        (nothing changed — reuse cache)
//!
//! The steady-state response is 304, so we cache the last ETag + monitor list
//! and send `If-None-Match`. A 304 MUST be handled before `.json()` (no body).
//! Cache lives in a Mutex because `fetch_monitors(&self)` is immutable and the
//! adapter is shared as `Arc<dyn Provider>`.
//!
//! Two-tier model: this status endpoint is the cheap always-on tier. Latency /
//! uptime / sparklines come from `/api/v1/dashboard/`, fetched on demand when
//! the popover opens (see `fetch_detail`).

use std::sync::Mutex;

use super::{Monitor, MonitorStatus, Provider, ProviderConfig, ProviderError};

pub struct Watch4Me {
    id: String,
    label: String,
    base: String,
    status_endpoint: String,
    dashboard_endpoint: String,
    token: String,
    http: reqwest::Client,
    /// Conditional-request cache: last ETag + the list it corresponds to.
    cache: Mutex<StatusCache>,
}

#[derive(Default)]
struct StatusCache {
    etag: Option<String>,
    monitors: Vec<Monitor>,
}

impl Watch4Me {
    pub fn new(
        cfg: &ProviderConfig,
        secret: String,
        http: reqwest::Client,
    ) -> Result<Self, ProviderError> {
        let base = cfg
            .base_url
            .clone()
            .unwrap_or_else(|| "https://watch4.me".to_string());
        let base = base.trim_end_matches('/').to_string();
        Ok(Self {
            id: cfg.id.clone(),
            label: cfg.label.clone(),
            status_endpoint: format!("{base}/api/v1/monitors/status"),
            dashboard_endpoint: format!("{base}/api/v1/dashboard/"),
            base,
            token: secret,
            http,
            cache: Mutex::new(StatusCache::default()),
        })
    }

    fn deep_link(&self, public_id: Option<&str>) -> String {
        match public_id {
            Some(pid) => format!("{}/monitors/{}/", self.base, pid),
            None => format!("{}/dashboard", self.base),
        }
    }
}

#[derive(serde::Deserialize)]
struct StatusResp {
    #[serde(default)]
    monitors: Vec<MonitorStats>,
}

#[derive(serde::Deserialize)]
struct MonitorStats {
    id: i64,
    #[serde(default)]
    public_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    is_up: bool,
    #[serde(default)]
    is_paused: bool,
    #[serde(default)]
    is_stale: bool,
    #[serde(default)]
    state_since: Option<String>,
    #[serde(default)]
    latest_check_at: Option<String>,
}

fn map_status(m: &MonitorStats) -> MonitorStatus {
    if m.is_paused {
        MonitorStatus::Paused
    } else if m.is_stale {
        // Data too old to trust — surface as Unknown rather than guess up/down.
        MonitorStatus::Unknown
    } else if m.is_up {
        MonitorStatus::Up
    } else {
        MonitorStatus::Down
    }
}

impl MonitorStats {
    fn into_monitor(self, base_deep_link: impl Fn(Option<&str>) -> String) -> Monitor {
        let status = map_status(&self);
        let detail_url = base_deep_link(self.public_id.as_deref());
        Monitor {
            name: self.name.unwrap_or_else(|| format!("monitor {}", self.id)),
            status,
            last_checked: self.latest_check_at,
            url: self.url,
            detail_url: Some(detail_url),
            state_since: self.state_since,
            id: self.id.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Provider for Watch4Me {
    fn kind(&self) -> &'static str {
        "watch4me"
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.label
    }

    async fn fetch_monitors(&self) -> Result<Vec<Monitor>, ProviderError> {
        // Send the cached ETag for a conditional request (steady state -> 304).
        let prev_etag = self.cache.lock().unwrap().etag.clone();

        let mut req = self
            .http
            .get(&self.status_endpoint)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/json");
        if let Some(etag) = &prev_etag {
            req = req.header(reqwest::header::IF_NONE_MATCH, etag);
        }

        let resp = req.send().await?;
        let status = resp.status();

        // 304: nothing changed. Reuse the cached list WITHOUT decoding a body
        // (a 304 has none — a blind .json() would error). No transition results,
        // so no spurious notification — which is correct.
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(self.cache.lock().unwrap().monitors.clone());
        }

        if !status.is_success() {
            return Err(super::http_status_error(status));
        }

        // Capture the new ETag before consuming the response for its body.
        let new_etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let body: StatusResp = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;

        let monitors: Vec<Monitor> = body
            .monitors
            .into_iter()
            .map(|m| m.into_monitor(|pid| self.deep_link(pid)))
            .collect();

        // Update the cache so the next poll can go conditional.
        {
            let mut cache = self.cache.lock().unwrap();
            cache.etag = new_etag;
            cache.monitors = monitors.clone();
        }

        Ok(monitors)
    }

    async fn fetch_detail(&self) -> Result<Option<serde_json::Value>, ProviderError> {
        // Rich detail tier: the dashboard endpoint carries latency / uptime %.
        // Fetched on demand (popover open), not on every background poll.
        let resp = self
            .http
            .get(&self.dashboard_endpoint)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/json")
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(super::http_status_error(resp.status()));
        }
        let value = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;
        Ok(Some(value))
    }
}
