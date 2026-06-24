//! Watch4.me adapter. Uses the existing dashboard API, which is documented as
//! being "for live dashboard + customer integrations" and accepts a Bearer
//! token (SessionOrBearerAuth):
//!
//!   GET {base}/api/v1/dashboard/   (Authorization: Bearer w4m_<token>, Accept: application/json)
//!   -> { monitors: [ { id, name, url, is_up, is_paused, is_stale, latest_check_at } ], ... }
//!
//! Content negotiation: the endpoint returns JSON only when Accept is
//! application/json and does NOT also contain text/html — so we send exactly
//! `Accept: application/json`.

use super::{Monitor, MonitorStatus, Provider, ProviderConfig, ProviderError};

pub struct Watch4Me {
    id: String,
    label: String,
    base: String,
    endpoint: String,
    token: String,
    http: reqwest::Client,
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
            endpoint: format!("{base}/api/v1/dashboard/"),
            base,
            token: secret,
            http,
        })
    }
}

#[derive(serde::Deserialize)]
struct DashboardResp {
    #[serde(default)]
    monitors: Vec<MonitorStats>,
}

#[derive(serde::Deserialize)]
struct MonitorStats {
    id: i64,
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
    latest_check_at: Option<String>,
    /// UUID used in monitor page URLs. Not yet returned by the dashboard API; if
    /// it appears, we deep-link to /monitors/<public_id>/ automatically.
    #[serde(default)]
    public_id: Option<String>,
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
        let resp = self
            .http
            .get(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(super::http_status_error(resp.status()));
        }

        let body: DashboardResp = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;

        Ok(body
            .monitors
            .into_iter()
            .map(|m| {
                let status = map_status(&m);
                // Deep-link to the monitor page when the API gives us a public_id;
                // otherwise fall back to the dashboard.
                let detail_url = m
                    .public_id
                    .as_ref()
                    .map(|pid| format!("{}/monitors/{}/", self.base, pid))
                    .unwrap_or_else(|| format!("{}/dashboard", self.base));
                Monitor {
                    name: m.name.unwrap_or_else(|| format!("monitor {}", m.id)),
                    status,
                    last_checked: m.latest_check_at,
                    url: m.url,
                    detail_url: Some(detail_url),
                    id: m.id.to_string(),
                }
            })
            .collect())
    }
}
