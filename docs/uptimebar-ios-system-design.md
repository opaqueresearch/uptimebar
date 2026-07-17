# uptimebar (iOS) — System Design & Development Plan

*Open-source, multi-provider uptime status client for iPhone. A DBA of Opaque Research LLC (which operates watch4.me). Extends the existing uptimebar menu-bar project to iOS, adds a minimal server (hel3) for public version tracking and opt-in, privacy-by-construction telemetry.*

**Status:** design draft · **Owner:** Opaque Research LLC · **Not legal advice** — ToS/GDPR/DBA items flagged for counsel review.

---

## 0. What this is (and isn't)

**Is:** a free, open-source iPhone app that shows a user's uptime monitors across multiple providers in one place — the "single pane of glass" for people who stitch coverage together across several services (free tiers included). watch4.me is a first-class provider and the natural funnel destination.

**Supported providers (v1 target):** UptimeRobot, Healthchecks.io, Better Stack, watch4.me.

**Isn't:**
- Not a paid product. No revenue → the DBA filing stays optional and the "commercial repackaging" reading of third-party ToS is far weaker.
- Not a rebranded/white-labeled monitoring service. Each user views *their own* data with *their own* key. uptimebar never repackages a provider's data as its own service.
- Not a server-side aggregator. hel3 never receives authenticated provider data. This sidesteps UptimeRobot Terms §14 *by construction*, not by policy.

---

## 1. Design principles (the foundation every claim rests on)

1. **Own-key, own-data.** Every provider is accessed with the user's own API key, stored in the device Keychain. Authenticated monitor data lives on the device and is never sent to hel3.
2. **Server sees only public + anonymous data.** hel3 handles exactly two things: public provider-version info, and opt-in anonymized telemetry. Nothing else.
3. **Privacy by construction, not by promise.** Where a privacy property can be guaranteed architecturally (IP-blind transport, on-device data), do that instead of a policy pledge.
4. **Verifiable.** The client is open-source, so every privacy claim can be independently audited against the code.
5. **Provider parity is deliberate, not accidental.** watch4.me is first-class (full features); third-party providers are read-first, capped to what their ToS and APIs cleanly allow.
6. **Per-provider ToS is a shipping gate.** No provider adapter ships until its terms are reviewed and, where required, a written integration position is settled.

---

## 2. Architecture overview

```
┌────────────────────────── iPhone (uptimebar) ──────────────────────────┐
│  UI layer (SwiftUI): aggregated list · detail · provider config        │
│  Widgets · Notifications · (later) Watch · (later) 2-way for watch4.me  │
│  ────────────────────────────────────────────────────────────────────  │
│  Provider core (shared with menu-bar app):                             │
│     protocol MonitoringProvider { fetchMonitors(), fetchDetail(), … }  │
│     ├─ UptimeRobotProvider   (v3 / legacy v2)                          │
│     ├─ HealthchecksProvider  (Management API v3; hosted + self-host)   │
│     ├─ BetterStackProvider   (Uptime API v2)                          │
│     └─ Watch4meProvider      (first-class, full feature set)          │
│  Keychain (per-provider API keys)   ·   local cache (App Group)        │
└───────────────┬───────────────────────────────────┬────────────────────┘
                │ authenticated, on-device only       │ public + opt-in only
                │ (never leaves device to hel3)        ▼
                ▼                          ┌──────────── hel3 (server) ───────────┐
   provider APIs (user's own key)         │  A. Version service (public data)     │
   api.uptimerobot.com/v3 …               │  B. Telemetry ingest (opt-in, anon,   │
   healthchecks.io/api/v3 …               │     IP-blind via OHTTP / onion)       │
   uptime.betterstack.com/api/v2 …        │  → aggregate store, short retention   │
                                          └───────────────────────────────────────┘
```

Key invariant: **the two arrows never cross.** Provider-authenticated traffic goes device→provider only. hel3 traffic is public-version and anonymized-telemetry only.

---

## 3. Client design (uptimebar iOS)

### 3.1 Provider abstraction
A single `MonitoringProvider` protocol; one adapter per service. This is the reusable core (share with the existing menu-bar codebase where practical). Adding a provider = writing an adapter, not touching UI.

| Provider | API / version | Auth | Notes |
|----------|---------------|------|-------|
| watch4.me | your own API | your own | First-class; full read + (later) write |
| UptimeRobot | v3 REST (legacy v2 fallback) | HTTP Basic / api_key | Read-only, own-key; §14 gate |
| Healthchecks.io | Management API v3 | project API key | Hosted **and** self-hosted base URLs; version may vary per instance |
| Better Stack | Uptime API v2 | bearer token | Read-first; ToS gate |

### 3.2 Data & storage
- API keys → **Keychain** (per provider). Never synced to hel3.
- Monitor cache → **App Group** container so widgets/Watch read without re-fetching.
- One cheap "summary" fetch per provider drives the aggregated view and widget (mind widget refresh budgets).

### 3.3 UI surfaces
- **Aggregated monitor list** — all providers merged, status-first, "N up / N down" header. This *is* the single-pane value prop.
- **Monitor detail** — status, uptime %, response-time chart (Swift Charts), recent events, "open in provider" link.
- **Provider config** — add/remove providers, paste key, per-instance base URL for self-hosted Healthchecks.
- **Widgets** (Home + Lock Screen) — cross-provider summary + single-monitor.
- **Notifications** — down/recovered, per provider capability.
- **watch4.me first-class treatment** — richer detail, and the only provider that later gets 2-way controls; a subtle, honest "consolidate onto watch4.me" affordance for multi-provider users.

### 3.4 Deferred (post-v1)
Apple Watch complication · Live Activities (incident-in-progress) · 2-way controls (ack/mute/pause) for watch4.me only.

---

## 4. Server-side design (hel3)

hel3 is intentionally tiny and stateless about users. Two jobs:

### 4.1 Version service (public data only)
Tracks the current published API version of each provider so the client can flag deprecation / compatibility. **No universal `GET /version` exists across providers**, so this is a per-provider source table:

| Provider | Current version | How hel3 reads it | Bump detection |
|----------|-----------------|-------------------|----------------|
| UptimeRobot | v3 (legacy v2 live) | cache from public `/api/` docs page | diff cached string on scheduled fetch |
| Healthchecks.io | Management API v3 | public docs page **+** GitHub tags/releases (open-source) | GitHub release webhook or tag poll |
| Better Stack | Uptime API v2 | public docs / base-URL path | diff on scheduled fetch |
| watch4.me | your own | internal — authoritative | direct |

hel3 also serves uptimebar's **own** app-update version (fully first-party, trivial).

### 4.2 Telemetry ingest (opt-in, anonymous, IP-blind)
- **Consent-gated**, default OFF. Layered explanation (short + detailed "how/why/what-protects-you") shown before first send.
- **Payload is bucketed, non-PII** (schema in §5).
- **Transport is IP-blind by design:** primary = **Oblivious HTTP (OHTTP)** (relay sees IP not payload; gateway sees payload not IP); maximalist option = **Tor onion service** for hel3 (no exit node, no IP anywhere). Baseline floor even without those: hel3 and its CDN/proxy **do not log source IP**.
- **Rotating identifier:** fresh-random UUID regenerated each period (e.g., monthly at 00:00 UTC), **no derivation from a persistent secret, no stored mapping** → periods are unlinkable even server-side. Add timing jitter so synchronized rotation can't be relinked.
- **Aggregate store, short raw retention.** Roll up to counts quickly; discard raw events.

---

## 5. Telemetry schema (concrete)

Example opt-in payload (all fields optional, all coarse):

```json
{
  "period_id": "e2b1…random-uuid",        // fresh per period, no mapping
  "period": "2026-07",                     // month bucket
  "app_version": "1.0",
  "os_major": "18",
  "country": "US",                          // coarse; consider dropping if small-population risk
  "providers_configured": {
    "uptimerobot": true,
    "healthchecks": false,
    "betterstack": true,
    "watch4me": true
  },
  "monitor_counts_bucketed": {              // ranges, never exact
    "uptimerobot": "6-20",
    "betterstack": "1-5",
    "watch4me": "1-5"
  },
  "features_enabled": {
    "widget": true, "notifications": true, "watch": false
  }
}
```

**Bucket boundaries:** `0`, `1–5`, `6–20`, `21–50`, `50+`. Coarse enough to prevent fingerprinting from combined dimensions.

**Metrics this unlocks (all anonymous, aggregate):**
- **Multi-provider ratio** — % of users with 2+ providers → validates the "stitcher" thesis.
- **watch4.me attach rate** — % of uptimebar users who include watch4.me → the funnel KPI.
- Provider adoption mix; monitor-count distribution per provider; feature adoption.
- Distinct active installs per period (via `period_id`); **no cross-period retention by design** — a deliberate trade for unlinkability.

---

## 6. Features → claims matrix

Every externally-stated claim maps to a concrete mechanism, so nothing is a bare promise.

| Claim we make | Mechanism that substantiates it | Verifiable how |
|---------------|--------------------------------|----------------|
| "We never receive your monitoring data." | Own-key architecture; authenticated calls are device→provider only | Open-source client audit |
| "We can't see where your data came from." | OHTTP / onion transport; hel3 + CDN don't log IP | Open-source client + infra description |
| "We can't link you across time." | Fresh-random per-period UUID, no mapping | Open-source client audit |
| "We only collect anonymous, coarse data." | Bucketed non-PII schema | Open-source client + published schema |
| "You're in control." | Opt-in default-off; layered consent | In-app UX + open source |
| "We don't repackage providers' services." | No server-side provider data; own-key, own-data; provider clearly attributed | Open-source client audit |
| "Your keys are safe." | Keychain storage, never transmitted to hel3 | Open-source client audit |

The through-line: **open source turns each claim from marketing into an auditable fact.**

---

## 7. Compliance posture (flagged for counsel)

- **Third-party ToS (per provider).** UptimeRobot §14 restricts repackaging authenticated data as a rebranded/standalone service; own-key + on-device + no server handling keeps uptimebar outside that. Non-commercial + open-source further weakens any "commercial repackaging" reading. **Gate:** review each provider's terms before shipping its adapter; if any require a written integration agreement for third-party clients, obtain it first.
- **App Store.** Complete the privacy nutrition label ("Usage Data / Product Interaction, not linked to identity"). First-party, non-tracking, no data-broker sharing generally avoids the ATT prompt — confirm against current policy. If embedding Tor, handle the encryption/export declaration.
- **GDPR.** Legal basis = consent (opt-in). Anonymous + IP-blind + no PII keeps most heavy obligations out of scope; document the anonymization.
- **DBA / entity.** No revenue → DBA filing optional. Apple Developer account shows Opaque Research LLC as producer, which is sufficient public attribution.

---

## 8. Development plan (sprints)

Each sprint lists its **goal**, **exit criteria**, and the **claims/skills it unlocks**. Sprints are sized to ship something demonstrable.

### Sprint 0 — Foundations
- **Goal:** repo, CI, `MonitoringProvider` protocol, Keychain layer, and **watch4.me provider** (your own service — zero third-party ToS risk) rendering a live read-only list on-device.
- **Exit:** watch4.me monitors visible on a real device; provider core unit-tested.
- **Unlocks:** reusable auth + networking modules; proves the abstraction.

### Sprint 1 — Client core UX
- **Goal:** aggregated list + monitor detail + provider-config screens; loading/empty/error/offline states.
- **Exit:** add/remove a provider, view detail, pull-to-refresh; passes offline + expired-key states.
- **Unlocks:** the single-pane UI shell.

### Sprint 2 — UptimeRobot + Healthchecks adapters
- **Goal:** UptimeRobot (v3, v2 fallback) and Healthchecks (Management API v3, hosted + self-hosted base URL) adapters. **ToS gate cleared for each before merge.**
- **Exit:** three providers (watch4.me + these two) in the aggregated view with own-key auth.
- **Unlocks:** "own-key, own-data" claim; multi-provider reality.

### Sprint 3 — Better Stack + aggregated multi-provider view
- **Goal:** Better Stack (Uptime API v2) adapter; polish the merged cross-provider list; surface the multi-provider "stitcher" experience and the honest watch4.me consolidation affordance.
- **Exit:** all four providers live in one pane; ToS gate cleared for Better Stack.
- **Unlocks:** the core value prop; funnel surface.

### Sprint 4 — Widgets + notifications
- **Goal:** Home/Lock Screen widgets (cross-provider summary + single monitor); push/down-recovered per provider capability.
- **Exit:** widget shows correct state incl. placeholder; alerts deep-link into detail.
- **Unlocks:** ambient stickiness; the highest-value user features.

### Sprint 5 — hel3 version service
- **Goal:** stand up hel3; per-provider version-source table (§4.1); client compatibility/deprecation checks; uptimebar self-update endpoint.
- **Exit:** client can detect and surface a provider API-version bump and an app update.
- **Unlocks:** version-awareness; first server component (no user data yet).

### Sprint 6 — Telemetry (opt-in, privacy-by-construction)
- **Goal:** consent UX (layered); bucketed schema; fresh-random per-period UUID; OHTTP transport (onion optional); hel3 ingest + aggregation with no IP logging.
- **Exit:** opt-in flow works; a test payload is anonymous, bucketed, IP-blind end-to-end; multi-provider ratio + watch4.me attach rate computed from aggregates.
- **Unlocks:** every privacy claim in §6; the funnel/stitcher KPIs.

### Sprint 7 — Harden, audit, ship
- **Goal:** privacy self-audit against §6 matrix; open-source repo polish (README, claim-to-code map); App Store privacy labels + submission; TestFlight → review → 1.0.
- **Exit:** 1.0 approved and live; claims independently verifiable from the public repo.
- **Unlocks:** the real goal — shipped, and the transferable release-loop experience.

### Later (post-1.0, gated)
Apple Watch complication · Live Activities · 2-way controls (watch4.me only, hardened write-path auth) · retention analysis (only if you consciously trade away per-period unlinkability) · any **paid aggregator** direction (requires written per-provider integration agreements first — parked).

---

## 9. Open questions / parking lot

- **hel3 scope confirmation** — is it (a) app-update service, (b) public provider-version tracker, (c) telemetry ingest? This design assumes all three; trim if not.
- **Platform** — this doc targets iOS and assumes the provider core is shared with the existing menu-bar app. Confirm whether macOS parity is in-scope now.
- **Country field** — keep or drop, depending on small-population re-identification tolerance.
- **Retention vs unlinkability** — currently chosen: unlinkability (no cross-period retention). Revisit only as a deliberate trade.
- **Per-provider ToS** — UptimeRobot §14 reviewed; Healthchecks (open-source, permissive) and Better Stack still need a formal read before their adapters ship.
- **Aggregator / commercialization** — parked; re-enter only with a specific competitive wedge *and* written integration agreements per provider.

---

Sources referenced during design: UptimeRobot Terms §6/§14 & API docs (v3/legacy v2); Healthchecks.io Management API v3 (open-source); Better Stack Uptime API v2.
