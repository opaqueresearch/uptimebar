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

// --- Detail-tier response shapes (live-verified 2026-06-28) ---

/// GET /monitors/{id}/response-times — latency series, split BY REGION. Each
/// point's `response_time` is in SECONDS (e.g. 0.546 = 546 ms).
#[derive(serde::Deserialize)]
struct RtResp {
    data: RtData,
}
#[derive(serde::Deserialize)]
struct RtData {
    attributes: RtAttrs,
}
#[derive(serde::Deserialize, Default)]
struct RtAttrs {
    #[serde(default)]
    regions: Vec<RtRegion>,
}
#[derive(serde::Deserialize)]
struct RtRegion {
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    response_times: Vec<RtPoint>,
}
#[derive(serde::Deserialize)]
struct RtPoint {
    // Points are already chronological within a region, so we don't need `at`.
    #[serde(default)]
    response_time: f64,
}

/// GET /monitors/{id}/sla — `availability` is already a percentage (e.g. 99.74).
#[derive(serde::Deserialize)]
struct SlaResp {
    data: SlaData,
}
#[derive(serde::Deserialize)]
struct SlaData {
    attributes: SlaAttrs,
}
#[derive(serde::Deserialize, Default)]
struct SlaAttrs {
    #[serde(default)]
    availability: Option<f64>,
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
                    state_since: None,
                }
            })
            .collect())
    }

    /// On-demand detail tier: current latency + uptime % + a latency sparkline,
    /// for every monitor. BetterStack has NO fleet-aggregation endpoint — latency
    /// and SLA are one call PER MONITOR each (the audit's N+1). We accept that here
    /// because the detail tier runs only on popover-open (not every poll), and we
    /// bound concurrency so a large fleet doesn't burst the rate limit. Latency is
    /// reported per region in SECONDS; we average regions per timestamp and convert
    /// to ms. (Live-verified against a real account 2026-06-28.)
    async fn fetch_detail(&self) -> Result<Option<serde_json::Value>, ProviderError> {
        // Re-fetch the list to get monitor ids (fetch_detail has no cached state).
        let ids = self.monitor_ids().await?;

        // Fan out, but at most 6 monitors in flight, to stay polite to the
        // undocumented rate limit. Each monitor = 2 calls (response-times + sla).
        use futures::stream::{self, StreamExt};
        let results: Vec<serde_json::Value> = stream::iter(ids)
            .map(|id| async move { self.detail_for(&id).await })
            .buffer_unordered(6)
            .filter_map(|r| async move { r })
            .collect()
            .await;

        Ok(Some(serde_json::json!({ "monitors": results })))
    }
}

impl BetterStack {
    /// API host base (e.g. "https://uptime.betterstack.com/api/v2"), derived from
    /// the monitors endpoint, for building the per-monitor detail URLs.
    fn api_v2(&self) -> &str {
        self.endpoint.trim_end_matches("/monitors")
    }

    /// Just the monitor ids (used by fetch_detail to know what to enrich).
    async fn monitor_ids(&self) -> Result<Vec<String>, ProviderError> {
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
        Ok(body.data.into_iter().map(|i| i.id).collect())
    }

    /// Fetch one monitor's normalized detail object, or None on any error (the
    /// status tier already populated the row — detail is best-effort enrichment).
    async fn detail_for(&self, id: &str) -> Option<serde_json::Value> {
        let mut obj = serde_json::Map::new();
        obj.insert("id".into(), id.to_string().into());

        // Latency series (per region, seconds) → average regions per timestamp → ms.
        if let Ok(resp) = self
            .http
            .get(format!("{}/monitors/{}/response-times", self.api_v2(), id))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/json")
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(rt) = resp.json::<RtResp>().await {
                    let series = representative_region_ms(&rt.data.attributes.regions);
                    if let Some(last) = series.last() {
                        obj.insert("latest_response_time_ms".into(), (*last).into());
                    }
                    if series.len() >= 2 {
                        let history: Vec<serde_json::Value> = series
                            .iter()
                            .map(|ms| serde_json::json!({ "avg_ms": ms, "failures": 0 }))
                            .collect();
                        obj.insert("response_history".into(), history.into());
                    }
                }
            }
        }

        // Uptime % (availability is already a percentage).
        if let Ok(resp) = self
            .http
            .get(format!("{}/monitors/{}/sla", self.api_v2(), id))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/json")
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(sla) = resp.json::<SlaResp>().await {
                    if let Some(pct) = sla.data.attributes.availability {
                        obj.insert("uptime_pct".into(), pct.into());
                    }
                }
            }
        }

        // Only return something if we actually enriched beyond the id.
        (obj.len() > 1).then(|| serde_json::Value::Object(obj))
    }
}

/// Pick ONE representative region's latency series (in ms). BetterStack checks
/// each region on its own schedule, so the regions' timestamps don't align —
/// averaging across them would just produce a jagged line jumping between wildly
/// different regional latencies (e.g. EU ~80ms vs AU ~1300ms). A single region is
/// cleaner and honest. Prefer us → eu → as → au, falling back to whatever exists.
/// `response_time` is in seconds; we ×1000 for ms.
fn representative_region_ms(regions: &[RtRegion]) -> Vec<f64> {
    const PREF: [&str; 4] = ["us", "eu", "as", "au"];
    let pick = PREF
        .iter()
        .find_map(|want| {
            regions
                .iter()
                .find(|r| r.region.as_deref() == Some(want) && !r.response_times.is_empty())
        })
        .or_else(|| regions.iter().find(|r| !r.response_times.is_empty()));

    pick.map(|r| r.response_times.iter().map(|p| p.response_time * 1000.0).collect())
        .unwrap_or_default()
}
