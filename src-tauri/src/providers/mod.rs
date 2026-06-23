//! Provider abstraction: a normalized monitor model plus a trait every backend
//! adapter implements. Adding a provider = one new file here + one match arm in
//! `build()` + one form variant in the settings UI.

pub mod uptimerobot;
pub mod watch4me;
pub mod uptimekuma;
pub mod betterstack;
pub mod healthchecks;

use std::sync::Arc;

/// Normalized status across every provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MonitorStatus {
    Up,
    Down,
    Paused,
    Unknown,
}

/// A single monitored target, normalized from a provider's native response.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Monitor {
    /// Provider-native stable id.
    pub id: String,
    pub name: String,
    pub status: MonitorStatus,
    /// ISO-8601 string of the last check, if the provider reports one.
    pub last_checked: Option<String>,
    /// The monitored URL/target, for display.
    pub url: Option<String>,
    /// A link into the provider's dashboard, for click-through.
    pub detail_url: Option<String>,
}

/// Errors a provider fetch can produce. Each `Display` is written for the end
/// user — it should make clear whether to fix the key, the URL, or the network.
/// Network/timeout errors map to `Unknown` (not `Down`) by the poller so a flaky
/// API never masquerades as an outage.
#[derive(thiserror::Error, Debug)]
pub enum ProviderError {
    #[error("The API key was rejected. Double-check you pasted the full, correct token.")]
    Auth,
    #[error("Rate limited by the provider. Wait a moment and try again.")]
    RateLimited,
    #[error("Couldn't reach the server. Check the Base URL and your internet connection.")]
    Unreachable,
    #[error("The server took too long to respond.")]
    Timeout,
    #[error("No API endpoint at that URL (404). Check the Base URL.")]
    NotFound,
    #[error("The server returned an error (HTTP {0}).")]
    Http(u16),
    #[error("Unexpected response from the server. Is the Base URL and provider type correct?")]
    Decode(String),
    #[error("{0}")]
    Config(String),
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            ProviderError::Timeout
        } else {
            // connect refused, DNS failure, TLS, etc. — all "can't reach it".
            ProviderError::Unreachable
        }
    }
}

/// Map a non-success HTTP status to a user-facing error. Shared by all HTTP
/// adapters so the messages are consistent.
pub fn http_status_error(status: reqwest::StatusCode) -> ProviderError {
    use reqwest::StatusCode;
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::Auth,
        StatusCode::NOT_FOUND => ProviderError::NotFound,
        StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimited,
        s => ProviderError::Http(s.as_u16()),
    }
}

/// Non-secret, persisted provider configuration. The API key lives in the OS
/// keychain, keyed by `id`, never here.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub interval_secs: Option<u64>,
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// Stable kind identifier, e.g. "uptimerobot".
    fn kind(&self) -> &'static str;
    /// The owning config id (used to key monitors and look up secrets).
    fn id(&self) -> &str;
    /// Human label for the UI.
    fn display_name(&self) -> &str;
    /// The clean contract: current monitors with status + last-checked.
    async fn fetch_monitors(&self) -> Result<Vec<Monitor>, ProviderError>;
}

/// Construct a provider from its config plus the secret pulled from the keychain.
pub fn build(
    cfg: &ProviderConfig,
    secret: String,
    http: reqwest::Client,
) -> Result<Arc<dyn Provider>, ProviderError> {
    match cfg.kind.as_str() {
        "uptimerobot" => Ok(Arc::new(uptimerobot::UptimeRobot::new(cfg, secret, http))),
        "watch4me" => Ok(Arc::new(watch4me::Watch4Me::new(cfg, secret, http)?)),
        "uptimekuma" => Ok(Arc::new(uptimekuma::UptimeKuma::new(cfg, secret, http)?)),
        "betterstack" => Ok(Arc::new(betterstack::BetterStack::new(cfg, secret, http))),
        "healthchecks" => Ok(Arc::new(healthchecks::Healthchecks::new(cfg, secret, http))),
        other => Err(ProviderError::Config(format!("unknown provider kind: {other}"))),
    }
}

/// UI-facing description of a provider kind, so the settings form can prefill
/// the label + base URL and show/hide fields. Single source of truth for the UI.
#[derive(Clone, serde::Serialize)]
pub struct ProviderMeta {
    pub kind: String,
    pub name: String,
    /// One-line description shown under the provider picker.
    pub help: String,
    /// Link to where the user gets their API key (provider API docs).
    pub docs_url: Option<String>,
    /// Prefilled into the Base URL field when this kind is chosen.
    pub default_base_url: Option<String>,
    pub base_url_placeholder: String,
    /// Base URL must be supplied by the user (no default that works).
    pub requires_base_url: bool,
    /// This provider needs an API key/token.
    pub requires_secret: bool,
    pub secret_label: String,
}

#[allow(clippy::too_many_arguments)]
fn meta(
    kind: &str,
    name: &str,
    help: &str,
    docs_url: Option<&str>,
    default_base_url: Option<&str>,
    base_url_placeholder: &str,
    requires_base_url: bool,
    requires_secret: bool,
    secret_label: &str,
) -> ProviderMeta {
    ProviderMeta {
        kind: kind.into(),
        name: name.into(),
        help: help.into(),
        docs_url: docs_url.map(Into::into),
        default_base_url: default_base_url.map(Into::into),
        base_url_placeholder: base_url_placeholder.into(),
        requires_base_url,
        requires_secret,
        secret_label: secret_label.into(),
    }
}

/// The provider kinds the UI offers, with prefill + field metadata.
pub fn kinds_meta() -> Vec<ProviderMeta> {
    vec![
        meta(
            "uptimerobot",
            "UptimeRobot",
            "Reads your monitors via the UptimeRobot account API. A read-only key works.",
            Some("https://uptimerobot.com/api/"),
            None,
            "",
            false,
            true,
            "API key",
        ),
        meta(
            "watch4me",
            "Watch4.me",
            "Reads your Watch4.me monitors via the dashboard API using an API token.",
            Some("https://watch4.me/api/"),
            Some("https://watch4.me"),
            "https://watch4.me",
            false,
            true,
            "API token (starts with w4m_)",
        ),
        meta(
            "uptimekuma",
            "Uptime Kuma",
            "Reads a public Uptime Kuma status page. Paste the full status-page URL below.",
            Some("https://github.com/louislam/uptime-kuma/wiki"),
            None,
            "https://kuma.example.com/status/your-slug",
            true,
            false,
            "",
        ),
        meta(
            "betterstack",
            "BetterStack",
            "Reads your BetterStack Uptime monitors using an API token.",
            Some("https://betterstack.com/docs/uptime/api/"),
            Some("https://uptime.betterstack.com"),
            "https://uptime.betterstack.com",
            false,
            true,
            "API token",
        ),
        meta(
            "healthchecks",
            "Healthchecks.io",
            "Reads your Healthchecks.io checks. A read-only key works; supports self-hosted.",
            Some("https://healthchecks.io/docs/api/"),
            Some("https://healthchecks.io"),
            "https://healthchecks.io",
            false,
            true,
            "read-only API key",
        ),
    ]
}
