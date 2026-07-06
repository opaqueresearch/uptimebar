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
    /// ISO-8601 timestamp the *current* status began, if the provider reports it.
    /// Drives "down for Xm" / "up since…" in the UI (computed client-side).
    #[serde(default)]
    pub state_since: Option<String>,
    /// Provider-native *public* id, distinct from `id` (which may be an internal
    /// numeric id). Watch4.me's action endpoints key on this. `None` for providers
    /// that don't expose one.
    #[serde(default)]
    pub public_id: Option<String>,
    /// Whether the monitor's alerts are muted at the provider (Watch4.me). Orthogonal
    /// to up/down; drives the mute/unmute toggle and notification suppression.
    #[serde(default)]
    pub is_muted: bool,
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
    /// A write/action endpoint was called with a read-only token (403
    /// insufficient_scope). Distinct from `Auth` so the UI can prompt for a
    /// read+write token rather than "the key was rejected".
    #[error("This API token is read-only. Use a read+write token to control monitors.")]
    InsufficientScope,
    /// The provider refused an action against a plan limit (e.g. resuming past the
    /// active-monitor cap). Carries the provider's human message.
    #[error("{0}")]
    PlanLimit(String),
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
    /// Optional provider-specific value (e.g. BetterStack team slug). Interpreted
    /// by the adapter; surfaced in the UI only when the provider's meta asks for it.
    #[serde(default)]
    pub extra: Option<String>,
    /// User-chosen color for this provider's left bar in the popover (a hex string
    /// from the fixed palette). `None` falls back to the kind's default color.
    #[serde(default)]
    pub color: Option<String>,
    /// Server-derived token scope: `"read"` | `"write"` | `None` (unknown). Set by
    /// the scope probe / a 403 demotion, NOT by the settings form — `upsert_provider`
    /// preserves the stored value. Drives whether monitor-action buttons render.
    #[serde(default)]
    pub scope: Option<String>,
    /// Default mute duration in seconds for this provider's monitors (`None` =
    /// indefinite). Chosen once in settings; applied by the popover mute button.
    #[serde(default)]
    pub mute_default_secs: Option<u64>,
}

/// A write action a provider may support against one monitor. `Mute` carries an
/// optional duration in seconds (`None` = indefinite).
#[derive(Clone, Copy, Debug)]
pub enum MonitorAction {
    Pause,
    Resume,
    Mute { duration_secs: Option<u64> },
    Unmute,
}

/// A token's capability scope, as best the app can determine it.
// Read/Write aren't constructed until the real scope probe lands (watch4.me#732);
// the stub `probe_scope` only ever returns Unknown today.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenScope {
    Read,
    Write,
    Unknown,
}

impl TokenScope {
    /// The persisted string form (`ProviderConfig.scope`).
    pub fn as_config(self) -> Option<String> {
        match self {
            TokenScope::Read => Some("read".into()),
            TokenScope::Write => Some("write".into()),
            TokenScope::Unknown => None,
        }
    }
}

/// The authoritative post-action state a provider reports, so the caller can
/// update local state without waiting for the next poll. Fields are `None` when
/// the action doesn't affect them.
#[derive(Clone, Debug, Default)]
pub struct ActionOutcome {
    pub is_paused: Option<bool>,
    pub is_muted: Option<bool>,
    /// False when the monitor was already in the requested state (idempotent no-op).
    pub changed: bool,
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

    /// Optional rich detail (latency, uptime %, etc.) fetched on demand when the
    /// popover opens — NOT on every background poll. Returns provider-native JSON
    /// the frontend renders opportunistically; `None` means "no detail tier".
    async fn fetch_detail(&self) -> Result<Option<serde_json::Value>, ProviderError> {
        Ok(None)
    }

    /// Perform a write action against one monitor (identified by its provider-native
    /// public id). Returns the authoritative post-action state. Defaults to
    /// unsupported so read-only adapters compile unchanged; only providers with an
    /// action API (Watch4.me) override this.
    async fn monitor_action(
        &self,
        _public_id: &str,
        _action: MonitorAction,
    ) -> Result<ActionOutcome, ProviderError> {
        Err(ProviderError::Config(
            "This provider doesn't support monitor actions.".into(),
        ))
    }

    /// Determine the token's scope up front (so the UI can gate write actions
    /// before the user tries one). Defaults to `Unknown` — providers without a
    /// scope-introspection endpoint rely on the runtime 403 instead. Watch4.me will
    /// override this once its `GET /api/v1/token` ships (watch4.me#732); until then
    /// it too returns `Unknown`.
    async fn probe_scope(&self) -> Result<TokenScope, ProviderError> {
        Ok(TokenScope::Unknown)
    }
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
    /// When set, the form shows an extra optional text field with this label.
    pub extra_label: Option<String>,
    pub extra_placeholder: String,
    pub extra_help: String,
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
        extra_label: None,
        extra_placeholder: String::new(),
        extra_help: String::new(),
    }
}

/// The provider kinds the UI offers, with prefill + field metadata.
pub fn kinds_meta() -> Vec<ProviderMeta> {
    let mut kinds = vec![
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
        // Uptime Kuma intentionally NOT offered in the Add picker (paused — see
        // PROVIDER-CAPABILITIES.md). The adapter is retained so existing configs
        // keep working and we can re-enable it later, but it's a low-yield funnel
        // segment already well served by purpose-built third-party menu-bar apps.
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
    ];

    // BetterStack monitor pages need a team URL slug the API doesn't expose, so
    // let the user supply it once (optional — empty just means no deep-links).
    if let Some(bs) = kinds.iter_mut().find(|m| m.kind == "betterstack") {
        bs.extra_label = Some("Team".to_string());
        bs.extra_placeholder = "t550046".to_string();
        bs.extra_help =
            "Optional, for click-through. The t… segment from your BetterStack URL: \
             uptime.betterstack.com/team/<this>/…"
                .to_string();
    }

    kinds
}
