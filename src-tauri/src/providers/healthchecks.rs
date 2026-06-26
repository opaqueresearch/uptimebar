//! Healthchecks.io adapter (works against the hosted service or a self-hosted
//! instance via base_url).
//!   GET {base}/api/v3/checks/   (header: X-Api-Key: <key>)
//!   -> { checks: [ { name, status, last_ping, uuid|unique_key, ... } ] }
//!
//! A read-only API key is sufficient. With a read-only key checks carry a
//! `unique_key` instead of a `uuid`; we accept either as the stable id.

use super::{Monitor, MonitorStatus, Provider, ProviderConfig, ProviderError};

pub struct Healthchecks {
    id: String,
    label: String,
    base: String,
    endpoint: String,
    api_key: String,
    http: reqwest::Client,
}

impl Healthchecks {
    pub fn new(cfg: &ProviderConfig, secret: String, http: reqwest::Client) -> Self {
        let base = cfg
            .base_url
            .clone()
            .unwrap_or_else(|| "https://healthchecks.io".to_string());
        let base = base.trim_end_matches('/').to_string();
        Self {
            id: cfg.id.clone(),
            label: cfg.label.clone(),
            endpoint: format!("{base}/api/v3/checks/"),
            base,
            api_key: secret,
            http,
        }
    }
}

#[derive(serde::Deserialize)]
struct Resp {
    #[serde(default)]
    checks: Vec<Check>,
}

#[derive(serde::Deserialize)]
struct Check {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    last_ping: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    unique_key: Option<String>,
    #[serde(default)]
    ping_url: Option<String>,
    // Embeds the uuid; a fallback source when the top-level uuid is absent but
    // these URLs are present. (Read-only keys redact all of them — then no
    // per-check deep-link is possible; HC has no slug-based detail route.)
    #[serde(default)]
    update_url: Option<String>,
}

/// Pull a v4 UUID out of a Healthchecks URL (ping_url / update_url embed it),
/// for keys that omit the top-level `uuid` but expose those URLs.
fn uuid_from_url(url: Option<&str>) -> Option<String> {
    let url = url?;
    // Match the last path segment that looks like a UUID (8-4-4-4-12 hex).
    url.rsplit('/')
        .map(|seg| seg.trim_end_matches(|c: char| !c.is_ascii_hexdigit() && c != '-'))
        .find(|seg| {
            let parts: Vec<&str> = seg.split('-').collect();
            parts.len() == 5
                && [8, 4, 4, 4, 12]
                    .iter()
                    .zip(&parts)
                    .all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_hexdigit()))
        })
        .map(String::from)
}

/// Healthchecks status: new / up / grace / down / paused.
/// "grace" = a ping is overdue but not yet failed — surface as Unknown (amber).
fn map_status(s: &Option<String>) -> MonitorStatus {
    match s.as_deref().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("up") => MonitorStatus::Up,
        Some("down") => MonitorStatus::Down,
        Some("paused") => MonitorStatus::Paused,
        _ => MonitorStatus::Unknown,
    }
}

#[async_trait::async_trait]
impl Provider for Healthchecks {
    fn kind(&self) -> &'static str {
        "healthchecks"
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
            .header("X-Api-Key", &self.api_key)
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
            .checks
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let id = c
                    .uuid
                    .clone()
                    .or_else(|| c.unique_key.clone())
                    .unwrap_or_else(|| format!("check-{i}"));
                // Deep-link to /checks/<uuid>/details/. The uuid comes from the
                // top-level field (read-write keys) or, failing that, embedded in
                // ping_url/update_url. Read-only keys redact ALL of these, so no
                // per-check link is possible — fall back to the site root then
                // (only the uuid route exists; /checks/<slug>/ is a 404).
                let detail_id = c
                    .uuid
                    .clone()
                    .or_else(|| uuid_from_url(c.ping_url.as_deref()))
                    .or_else(|| uuid_from_url(c.update_url.as_deref()));
                let detail_url = match detail_id {
                    Some(uuid) => format!("{}/checks/{}/details/", self.base, uuid),
                    None => format!("{}/", self.base),
                };
                Monitor {
                    name: c.name.unwrap_or_else(|| id.clone()),
                    status: map_status(&c.status),
                    last_checked: c.last_ping,
                    url: c.ping_url,
                    detail_url: Some(detail_url),
                    id,
                    state_since: None,
                }
            })
            .collect())
    }
}
