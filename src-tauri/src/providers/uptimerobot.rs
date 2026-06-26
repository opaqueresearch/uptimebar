//! UptimeRobot adapter. POSTs to the v2 getMonitors endpoint with an account
//! (read-only is enough) API key. Simplest, most stable of the three — used as
//! the end-to-end proving ground.

use super::{Monitor, MonitorStatus, Provider, ProviderConfig, ProviderError};

const API_URL: &str = "https://api.uptimerobot.com/v2/getMonitors";

pub struct UptimeRobot {
    id: String,
    label: String,
    api_key: String,
    http: reqwest::Client,
}

impl UptimeRobot {
    pub fn new(cfg: &ProviderConfig, secret: String, http: reqwest::Client) -> Self {
        Self {
            id: cfg.id.clone(),
            label: cfg.label.clone(),
            api_key: secret,
            http,
        }
    }
}

#[derive(serde::Deserialize)]
struct Resp {
    stat: String,
    #[serde(default)]
    monitors: Vec<RawMonitor>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(serde::Deserialize)]
struct ApiError {
    #[serde(default)]
    message: String,
}

#[derive(serde::Deserialize)]
struct RawMonitor {
    id: u64,
    friendly_name: String,
    #[serde(default)]
    url: Option<String>,
    status: i64,
}

/// UptimeRobot status codes: 0 paused, 1 not-checked-yet, 2 up, 8 seems-down, 9 down.
fn map_status(code: i64) -> MonitorStatus {
    match code {
        2 => MonitorStatus::Up,
        8 | 9 => MonitorStatus::Down,
        0 => MonitorStatus::Paused,
        _ => MonitorStatus::Unknown,
    }
}

#[async_trait::async_trait]
impl Provider for UptimeRobot {
    fn kind(&self) -> &'static str {
        "uptimerobot"
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
            .post(API_URL)
            .form(&[
                ("api_key", self.api_key.as_str()),
                ("format", "json"),
            ])
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimited);
        }

        let body: Resp = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;

        if body.stat != "ok" {
            let msg = body.error.map(|e| e.message).unwrap_or_default();
            // UptimeRobot reports a bad key as a normal "fail" response.
            if msg.to_lowercase().contains("api_key") {
                return Err(ProviderError::Auth);
            }
            return Err(ProviderError::Decode(format!("api error: {msg}")));
        }

        Ok(body
            .monitors
            .into_iter()
            .map(|m| Monitor {
                id: m.id.to_string(),
                name: m.friendly_name,
                status: map_status(m.status),
                last_checked: None,
                url: m.url,
                detail_url: Some("https://dashboard.uptimerobot.com/monitors".to_string()),
                state_since: None,
            })
            .collect())
    }
}
