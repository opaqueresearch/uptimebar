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

use super::{
    ActionOutcome, Monitor, MonitorAction, MonitorStatus, Provider, ProviderConfig, ProviderError,
};

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
    is_muted: bool,
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
            // Retain public_id — Watch4.me's action endpoints key on it.
            public_id: self.public_id,
            is_muted: self.is_muted,
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
            log::debug!("watch4me {}: 304 Not Modified (cache hit)", self.label);
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

    async fn monitor_action(
        &self,
        public_id: &str,
        action: MonitorAction,
    ) -> Result<ActionOutcome, ProviderError> {
        // POST /api/v1/monitors/{public_id}/{pause|resume|mute|unmute}. No body;
        // mute takes ?duration_seconds=N (omit = indefinite). All idempotent.
        let (verb, duration) = match action {
            MonitorAction::Pause => ("pause", None),
            MonitorAction::Resume => ("resume", None),
            MonitorAction::Mute { duration_secs } => ("mute", duration_secs),
            MonitorAction::Unmute => ("unmute", None),
        };
        let mut url = format!("{}/api/v1/monitors/{}/{}", self.base, public_id, verb);
        if let Some(secs) = duration {
            url = format!("{url}?duration_seconds={secs}");
        }

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/json")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            // Parse the uniform error envelope ({"error":{"code":...}}) to map the
            // scope/plan-limit cases to specific errors before the generic fallback.
            let body = resp.text().await.unwrap_or_default();
            let code = serde_json::from_str::<ErrorEnvelope>(&body)
                .ok()
                .map(|e| e.error);
            if let Some(err) = code {
                match err.code.as_str() {
                    "insufficient_scope" => return Err(ProviderError::InsufficientScope),
                    "plan_limit" => return Err(ProviderError::PlanLimit(err.message)),
                    _ => {}
                }
            }
            return Err(super::http_status_error(status));
        }

        let result: ActionResult = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;
        Ok(ActionOutcome {
            is_paused: result.is_paused,
            is_muted: result.is_muted,
            changed: result.changed,
        })
    }
}

/// Watch4.me action success body (union of pause/resume + mute/unmute shapes).
#[derive(serde::Deserialize)]
struct ActionResult {
    #[serde(default)]
    is_paused: Option<bool>,
    #[serde(default)]
    is_muted: Option<bool>,
    #[serde(default)]
    changed: bool,
}

/// The uniform error envelope from Watch4.me's #728 exception handler.
#[derive(serde::Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(serde::Deserialize)]
struct ErrorBody {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}
