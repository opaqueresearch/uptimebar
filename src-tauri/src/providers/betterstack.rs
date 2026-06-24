//! BetterStack (Better Uptime) adapter.
//!   GET {base}/api/v2/monitors   (Authorization: Bearer <token>)
//!   -> { data: [ { id, attributes: { url, pronounceable_name, status, ... } } ] }
//!
//! Default base is https://uptime.betterstack.com. Reads the first page of
//! monitors (50); paginating further is a later enhancement.

use super::{Monitor, MonitorStatus, Provider, ProviderConfig, ProviderError};

pub struct BetterStack {
    id: String,
    label: String,
    base: String,
    /// Team URL slug (e.g. "t550046"), supplied by the user — the API doesn't
    /// expose it. Enables per-monitor deep-links when present.
    team: Option<String>,
    endpoint: String,
    token: String,
    http: reqwest::Client,
}

/// Accept either a bare slug ("t550046") or a pasted dashboard URL containing
/// "/team/<slug>/", and return the slug.
fn normalize_team(raw: &str) -> Option<String> {
    let s = raw.trim().trim_matches('/');
    if s.is_empty() {
        return None;
    }
    let slug = match s.find("/team/") {
        Some(i) => s[i + "/team/".len()..].split('/').next().unwrap_or(s),
        None => s.split('/').next().unwrap_or(s),
    };
    (!slug.is_empty()).then(|| slug.to_string())
}

impl BetterStack {
    pub fn new(cfg: &ProviderConfig, secret: String, http: reqwest::Client) -> Self {
        let base = cfg
            .base_url
            .clone()
            .unwrap_or_else(|| "https://uptime.betterstack.com".to_string());
        let base = base.trim_end_matches('/').to_string();
        Self {
            id: cfg.id.clone(),
            label: cfg.label.clone(),
            team: cfg.extra.as_deref().and_then(normalize_team),
            endpoint: format!("{base}/api/v2/monitors"),
            base,
            token: secret,
            http,
        }
    }
}

#[derive(serde::Deserialize)]
struct Resp {
    #[serde(default)]
    data: Vec<Item>,
}

#[derive(serde::Deserialize)]
struct Item {
    id: String,
    #[serde(default)]
    attributes: Attrs,
}

#[derive(serde::Deserialize, Default)]
struct Attrs {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    pronounceable_name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    last_checked_at: Option<String>,
}

/// BetterStack status: up / down / paused / pending / maintenance / validating.
fn map_status(s: &Option<String>) -> MonitorStatus {
    match s.as_deref().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("up") => MonitorStatus::Up,
        Some("down") => MonitorStatus::Down,
        Some("paused") | Some("maintenance") => MonitorStatus::Paused,
        _ => MonitorStatus::Unknown,
    }
}

#[async_trait::async_trait]
impl Provider for BetterStack {
    fn kind(&self) -> &'static str {
        "betterstack"
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

        let body: Resp = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;

        Ok(body
            .data
            .into_iter()
            .map(|item| {
                let name = item
                    .attributes
                    .pronounceable_name
                    .clone()
                    .or_else(|| item.attributes.url.clone())
                    .unwrap_or_else(|| format!("monitor {}", item.id));
                Monitor {
                    status: map_status(&item.attributes.status),
                    last_checked: item.attributes.last_checked_at,
                    url: item.attributes.url,
                    // Per-monitor link needs the team URL slug, which the API
                    // doesn't return — use the one the user supplied if any,
                    // else fall back to the dashboard (avoids a 404).
                    detail_url: Some(match &self.team {
                        Some(team) => format!("{}/team/{}/monitors/{}", self.base, team, item.id),
                        None => format!("{}/", self.base),
                    }),
                    name,
                    id: item.id,
                }
            })
            .collect())
    }
}
