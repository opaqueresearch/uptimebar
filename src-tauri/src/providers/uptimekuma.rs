//! Uptime Kuma adapter. Kuma has no stable public REST API — its live data flows
//! over Socket.IO. The pragmatic MVP surface is the public *status page* JSON,
//! which is plain HTTP:
//!   GET {base}/api/status-page/{slug}            -> monitor names/groups
//!   GET {base}/api/status-page/heartbeat/{slug}  -> latest heartbeat per monitor
//!
//! Config: put the full status-page URL (".../status/{slug}") in `base_url`.
//! Only monitors published on that status page are visible. Full coverage via
//! Socket.IO auth is a Phase 2 enhancement.

use std::collections::HashMap;

use super::{Monitor, MonitorStatus, Provider, ProviderConfig, ProviderError};

pub struct UptimeKuma {
    id: String,
    label: String,
    base: String,
    slug: String,
    http: reqwest::Client,
}

impl UptimeKuma {
    pub fn new(
        cfg: &ProviderConfig,
        _secret: String,
        http: reqwest::Client,
    ) -> Result<Self, ProviderError> {
        let url = cfg.base_url.clone().ok_or_else(|| {
            ProviderError::Config("set base_url to the status-page URL (.../status/{slug})".into())
        })?;
        let (base, slug) = parse_status_url(&url).ok_or_else(|| {
            ProviderError::Config(format!("could not parse a /status/{{slug}} from: {url}"))
        })?;
        Ok(Self {
            id: cfg.id.clone(),
            label: cfg.label.clone(),
            base,
            slug,
            http,
        })
    }
}

/// Split "https://kuma.example.com/status/prod" into ("https://kuma.example.com", "prod").
fn parse_status_url(url: &str) -> Option<(String, String)> {
    let trimmed = url.trim_end_matches('/');
    let idx = trimmed.find("/status/")?;
    let base = trimmed[..idx].to_string();
    let slug = trimmed[idx + "/status/".len()..].to_string();
    if base.is_empty() || slug.is_empty() {
        return None;
    }
    Some((base, slug))
}

#[derive(serde::Deserialize)]
struct StatusPage {
    #[serde(default, rename = "publicGroupList")]
    public_group_list: Vec<Group>,
}

#[derive(serde::Deserialize)]
struct Group {
    #[serde(default, rename = "monitorList")]
    monitor_list: Vec<KumaMonitor>,
}

#[derive(serde::Deserialize)]
struct KumaMonitor {
    id: i64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(serde::Deserialize)]
struct HeartbeatResp {
    #[serde(default, rename = "heartbeatList")]
    heartbeat_list: HashMap<String, Vec<Heartbeat>>,
}

#[derive(serde::Deserialize)]
struct Heartbeat {
    #[serde(default)]
    status: i64,
    #[serde(default)]
    time: Option<String>,
}

/// Kuma heartbeat status: 0 down, 1 up, 2 pending, 3 maintenance.
fn map_status(code: i64) -> MonitorStatus {
    match code {
        1 => MonitorStatus::Up,
        0 => MonitorStatus::Down,
        3 => MonitorStatus::Paused,
        _ => MonitorStatus::Unknown,
    }
}

#[async_trait::async_trait]
impl Provider for UptimeKuma {
    fn kind(&self) -> &'static str {
        "uptimekuma"
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.label
    }

    async fn fetch_monitors(&self) -> Result<Vec<Monitor>, ProviderError> {
        let page_url = format!("{}/api/status-page/{}", self.base, self.slug);
        let hb_url = format!("{}/api/status-page/heartbeat/{}", self.base, self.slug);

        let page_resp = self.http.get(&page_url).send().await?;
        if !page_resp.status().is_success() {
            return Err(super::http_status_error(page_resp.status()));
        }
        let page: StatusPage = page_resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;

        let hb_resp = self.http.get(&hb_url).send().await?;
        if !hb_resp.status().is_success() {
            return Err(super::http_status_error(hb_resp.status()));
        }
        let hb: HeartbeatResp = hb_resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;

        let mut out = Vec::new();
        for group in page.public_group_list {
            for m in group.monitor_list {
                let last = hb
                    .heartbeat_list
                    .get(&m.id.to_string())
                    .and_then(|v| v.last());
                out.push(Monitor {
                    id: m.id.to_string(),
                    name: m.name.unwrap_or_else(|| format!("monitor {}", m.id)),
                    status: last.map(|h| map_status(h.status)).unwrap_or(MonitorStatus::Unknown),
                    last_checked: last.and_then(|h| h.time.clone()),
                    url: m.url,
                    detail_url: Some(page_url.clone()),
                    state_since: None,
                    public_id: None,
                    is_muted: false,
                });
            }
        }
        Ok(out)
    }
}
