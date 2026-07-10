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

#[derive(serde::Deserialize, Default)]
struct ApiError {
    #[serde(default, rename = "type")]
    error_type: String,
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
    /// Current/average latency, ms. UptimeRobot returns this as a STRING
    /// (e.g. "409.531"), not a number — parse it. Present on the detail fetch.
    #[serde(default)]
    average_response_time: Option<String>,
    /// Uptime ratio for the windows requested via `custom_uptime_ratios`. With a
    /// single window ("30") this is one number as a string, e.g. "99.973".
    #[serde(default)]
    custom_uptime_ratio: Option<String>,
    /// Response-time series (newest-first), present with `response_times=1`. Free
    /// tier returns the full retained window (~24h, ~5-min buckets) — enough for a
    /// real sparkline. `value` is integer ms.
    #[serde(default)]
    response_times: Vec<ResponseTime>,
}

#[derive(serde::Deserialize)]
struct ResponseTime {
    #[serde(default)]
    value: f64,
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
                // Per-monitor deep-link. UptimeRobot exposes no dashboard-URL
                // field in the API, but the dashboard route is stable and
                // verified: /monitors/<numeric id> opens that monitor directly.
                detail_url: Some(format!(
                    "https://dashboard.uptimerobot.com/monitors/{}",
                    m.id
                )),
                state_since: None,
                public_id: None,
                is_muted: false,
            })
            .collect())
    }

    /// On-demand detail tier: current latency, 30-day uptime %, and a latency
    /// sparkline. UptimeRobot returns all three in ONE `getMonitors` call with the
    /// extra params (not N+1). The free tier returns the full retained
    /// response-time series (~24h, ~5-min buckets), so a real sparkline IS viable
    /// here — `average_response_time` and uptime arrive as STRINGS, so we parse.
    async fn fetch_detail(&self) -> Result<Option<serde_json::Value>, ProviderError> {
        let resp = self
            .http
            .post(API_URL)
            .form(&[
                ("api_key", self.api_key.as_str()),
                ("format", "json"),
                ("response_times", "1"),
                ("custom_uptime_ratios", "30"),
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
            if msg.to_lowercase().contains("api_key") {
                return Err(ProviderError::Auth);
            }
            return Err(ProviderError::Decode(format!("api error: {msg}")));
        }

        // Emit the normalized detail shape the popover reads: { monitors: [{ id,
        // latest_response_time_ms, uptime_pct }] }. Omit a field when the provider
        // didn't supply it, so the UI leaves it blank rather than showing a zero.
        let monitors: Vec<serde_json::Value> = body
            .monitors
            .into_iter()
            .map(|m| {
                let mut obj = serde_json::Map::new();
                obj.insert("id".into(), m.id.to_string().into());
                // average_response_time is a STRING ("409.531") — parse to a float.
                if let Some(ms) = m
                    .average_response_time
                    .as_deref()
                    .and_then(|s| s.trim().parse::<f64>().ok())
                {
                    obj.insert("latest_response_time_ms".into(), ms.into());
                }
                if let Some(pct) = m.custom_uptime_ratio.as_deref().and_then(parse_pct) {
                    obj.insert("uptime_pct".into(), pct.into());
                }
                // Sparkline: the series is newest-first, so reverse to chronological.
                // UptimeRobot's series carries no per-bucket failure flag, so mark
                // none (a value of 0 ms would be the only down signal, but the API
                // simply omits failed checks from response_times).
                if m.response_times.len() >= 2 {
                    let history: Vec<serde_json::Value> = m
                        .response_times
                        .iter()
                        .rev()
                        .map(|rt| serde_json::json!({ "avg_ms": rt.value, "failures": 0 }))
                        .collect();
                    obj.insert("response_history".into(), history.into());
                }
                serde_json::Value::Object(obj)
            })
            .collect();

        Ok(Some(serde_json::json!({ "monitors": monitors })))
    }

    fn capabilities(&self) -> super::ActionCaps {
        // Pause/resume via editMonitor. No mute.
        super::ActionCaps { pause: true, mute: false }
    }

    async fn monitor_action(
        &self,
        id: &str,
        action: super::MonitorAction,
    ) -> Result<super::ActionOutcome, ProviderError> {
        use super::MonitorAction;
        // editMonitor status: 0 = paused, 1 = active (resume). Mute unsupported.
        let status = match action {
            MonitorAction::Pause => "0",
            MonitorAction::Resume => "1",
            MonitorAction::Mute { .. } | MonitorAction::Unmute => {
                return Err(ProviderError::Config("UptimeRobot doesn't support mute.".into()));
            }
        };
        let resp = self
            .http
            .post("https://api.uptimerobot.com/v2/editMonitor")
            .form(&[
                ("api_key", self.api_key.as_str()),
                ("format", "json"),
                ("id", id),
                ("status", status),
            ])
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ProviderError::RateLimited);
        }
        let body: EditResp = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;
        if body.stat != "ok" {
            let err = body.error.unwrap_or_default();
            // A read-only key can't edit — UptimeRobot returns type "not_authorized"
            // ("You are not allowed to perform this request"). Live-verified.
            if err.error_type == "not_authorized" || err.message.to_lowercase().contains("not allowed")
            {
                return Err(ProviderError::InsufficientScope);
            }
            return Err(ProviderError::Decode(format!("edit failed: {}", err.message)));
        }
        Ok(super::ActionOutcome {
            is_paused: Some(status == "0"),
            is_muted: None,
            changed: true,
        })
    }
}

#[derive(serde::Deserialize)]
struct EditResp {
    stat: String,
    #[serde(default)]
    error: Option<ApiError>,
}

/// `custom_uptime_ratio` is a string; with one requested window it's a single
/// number ("99.973"). Multiple windows come dash-joined — take the first.
fn parse_pct(raw: &str) -> Option<f64> {
    raw.split('-').next()?.trim().parse::<f64>().ok()
}
