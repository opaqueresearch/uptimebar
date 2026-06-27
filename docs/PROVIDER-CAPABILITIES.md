# Provider Capability Matrix

What each supported provider's API actually exposes, audited 2026-06-27 for
UptimeBar's funnel strategy (issue #5) and to decide which feature-contrasts with
Watch4.me are *genuine* (issue #8). The honesty guardrail: only surface a
capability gap in the UI if it's real — this doc is the source of truth.

**Legend**
- ✅ **Confirmed** — proven from our own adapter/live data, or unambiguous in docs.
- 📄 **Documented** — stated in the provider's API docs; not yet exercised by us.
- ⚠️ **Partial / caveated** — works, but with a cost (extra calls, key type, plan, window).
- ❌ **Not supported** — no API path to it.
- ❓ **Needs account/instance to verify** — we have no account; confirm before relying on it.

Baseline = **Watch4.me** (we have a live token; all ✅).

---

## The matrix

| Capability | Watch4.me | UptimeRobot | BetterStack | Uptime Kuma (status-page) | Healthchecks.io |
|---|---|---|---|---|---|
| **Current latency** | ✅ `latest_response_time_ms` | ✅ `average_response_time` | ⚠️ per-monitor call | ✅ heartbeat `ping` | ⚠️ run-time only |
| **Latency history (sparkline)** | ✅ `response_history[]` (1 call) | ⚠️ `response_times=1`; free=24h | ⚠️ `/response-times` 24h, **N+1** | ✅ `heartbeatList` (≤100 pts) | ⚠️ `/pings/` run-time, RW key |
| **Uptime %** | ✅ `uptime_pct` | ✅ `custom_uptime_ratios` | ✅ `/sla` (N+1) | ⚠️ 24h only (`uptimeList`) | ❌ none (compute from flips) |
| **Status-change time ("down for Xm")** | ✅ `state_since` (1 call) | ✅ `logs=1` | ⚠️ via `/incidents` v3 (N+1) | ✅ derive from heartbeats | ✅ `/flips/` (even read-only) |
| **Per-monitor deep-link** | ✅ `public_id` | ⚠️ build from `id` (unofficial) | ❌ team slug not in API | ❌ status page only | ⚠️ uuid → RW key only |
| **Remote pause/resume** | ❓ (#710 pending) | ✅ `editMonitor status` | ✅ `PATCH paused` | ❌ (needs auth Socket.IO) | ✅ `/pause` `/resume` |
| **Remote mute/ack** | ❓ (#710 pending) | ❌ | ⚠️ incident ack (not mute) | ❌ | ❌ |
| **Conditional polling (ETag/304)** | ✅ | ❌ | ❌ | ❌ (1-min server cache) | ❌ |
| **All data in ONE call** | ✅ status endpoint | ⚠️ one big getMonitors w/ params | ❌ N+1 fan-out | ⚠️ 2 calls (page+heartbeat) | ⚠️ list + per-check for detail |
| **Auth model fit (read-only viewer)** | ✅ Bearer | ✅ read-only key | ⚠️ no read-only scope | ✅ public, no key | ⚠️ RW key needed for detail/links |
| **Self-hostable** | ⚠️ (SaaS; self-host TBD) | ❌ SaaS only | ❌ SaaS only | ✅ self-hosted by design | ✅ open-source, self-hostable |
| **Custom base URL in UptimeBar today** | ✅ base_url | ❌ hardcoded (SaaS, fine) | ✅ base_url (vestigial — SaaS) | ✅ **required** (paste instance URL) | ✅ base_url ("supports self-hosted") |

---

## The honest strategic read

**Most per-monitor *capabilities* are NOT unique to Watch4.me.** Latency, uptime,
and status-timing exist almost everywhere in some form. If we framed "sparklines
are a Watch4.me thing," it would be **false** — UptimeRobot, Uptime Kuma, and
BetterStack can all feed a sparkline. Do **not** build that contrast.

**Watch4.me's genuine, defensible differentiators are architectural, not feature-checkboxes:**

1. **Everything in one cheap call.** Watch4.me returns status + latency + uptime +
   state_since + deep-link id from a single endpoint. The others force **N+1
   fan-out** (BetterStack: a call per monitor for latency, another for SLA,
   another for incident timing) or split across calls. For an always-on menu-bar
   poller, that efficiency is real and felt.
2. **Conditional polling (ETag/304) — unique to Watch4.me.** Every other provider
   is full-fetch-every-poll. This is a true, verifiable "only Watch4.me" trait.
3. **Deep-links that just work.** Watch4.me `public_id` → specific monitor page,
   no caveats. BetterStack **can't** (slug not in API). Healthchecks needs a
   read-write key. UptimeRobot has no official dashboard URL. Uptime Kuma has no
   per-monitor page at all. **This is the cleanest honest contrast.**
4. **Read-only-friendly full fidelity.** Watch4.me gives everything to a simple
   Bearer token; Healthchecks cripples read-only keys, BetterStack has no
   read-only scope.

**Per-provider honest one-liners (safe to use in UI/marketing):**
- **UptimeRobot** — capable API, but no official per-monitor deep-link and free tier is 10 req/min + 24h response-time history.
- **BetterStack** — rich data, but spread across N+1 calls and **no per-monitor deep-link** (team slug isn't in the API).
- **Uptime Kuma** — great self-hosted data, but the status-page path has **no per-monitor links** and **no remote control**; 24h uptime only.
- **Healthchecks.io** — cron/heartbeat focused: **no uptime %**, "latency" is job run-time, and deep-links/detail need the **read-write** key.

---

## Self-hosting & custom URLs

Two of the four supported providers are **self-hostable**, and self-hosted users
are a *strategically prime* migration pool — the "self-hosting-fatigued" the
strategy (#5) explicitly targets. Our app must accept a custom base URL for them.

**Current state (already mostly handled):**
- **Uptime Kuma** — `requires_base_url: true`. The user pastes their instance's
  full status-page URL; the adapter parses `{base}` + `{slug}`. ✅ Fully
  self-host-aware today.
- **Healthchecks.io** — has a `base_url` field (default `https://healthchecks.io`);
  the help text already says "supports self-hosted." A self-hoster points it at
  their instance. ✅ Handled.
- **Watch4.me** — `base_url` field present (default `https://watch4.me`); ready if
  Watch4.me ever offers self-host/on-prem. ⚠️ SaaS today.
- **UptimeRobot / BetterStack** — SaaS only; no self-host. UptimeRobot's adapter
  hardcodes the API host (fine). BetterStack's base_url is vestigial.

**Gaps / action items:**
- The **deep-link base** must follow the custom URL for self-hosted instances.
  Watch4.me and Healthchecks already build links off the configured `base`. ✅
  Verify Healthchecks self-hosted detail URLs resolve as `{base}/checks/<uuid>/details/`.
- **Uptime Kuma self-hosted has no per-monitor link** regardless of base (status
  page only) — consistent with the matrix; not a base-URL issue.
- **Discoverability:** the base-URL field exists but isn't obviously "for
  self-hosters." Minor UX win: label/help that says "Self-hosted? Enter your
  instance URL" on Kuma + Healthchecks. (Candidate for the Help/About pane.)
- ❓ **Needs an instance to verify:** Healthchecks self-hosted API parity, and
  Kuma per-version status-page JSON (see Next Steps).

## Per-provider detail (evidence)

### UptimeRobot — `POST /v2/getMonitors`
- Latency: `response_times=1` → `{datetime,value}` series; `response_times_average` downsamples. **Free ≈ last 24h; Pro up to 12 mo (7-day windows).** Current: `average_response_time`.
- Uptime: `custom_uptime_ratios=7-30-45`, `all_time_uptime_ratio`.
- Status-change: `logs=1` → entries `{type(1=down,2=up,99=paused), datetime, duration}`. Most recent down = outage start.
- Deep-link: stable numeric `id` + target `url`, but **no documented dashboard URL field** — link format unofficial.
- Actions: `editMonitor` pause/resume (`status` 0/1, needs RW key). **No mute, no ack.**
- Polling: **no ETag/304**; outbound alert webhooks only. Rate limit **free 10/min**, Pro up to 5000/min, 429 + `Retry-After`.
- v3 API exists but param/field parity **unverified (needs account)**.

### BetterStack — `/api/v2/monitors` (+ v3 incidents)
- Latency: `GET /monitors/{id}/response-times` (per-region series, **24h, ~30s, no range params**). **N+1.**
- Uptime/SLA: `GET /monitors/{id}/sla` → `availability`, `total_downtime`, incident stats; `from`/`to`. **N+1.**
- Status-change: monitor has no `status_changed_at`; use `GET /api/v3/incidents?monitor_id=` → `started_at`. **N+1.**
- Deep-link: `team_name` present but **no team URL slug, no monitor URL** → can't build `/team/<slug>/monitors/<id>` from API. **Confirmed blocker.**
- Actions: `PATCH /monitors/{id} {paused}`; incident acknowledge/resolve/reopen. No mute.
- Polling: pagination (`page`/`per_page`≤250); **no ETag/304**; rate limit **undocumented (needs account)**.

### Uptime Kuma — status-page JSON (verified against master source)
- Two endpoints: `/api/status-page/{slug}` (config + monitors: `id,name,type,sendUrl`,opt `url`) and `/api/status-page/heartbeat/{slug}` (`heartbeatList`, `uptimeList`).
- Latency: heartbeat `ping` (ms), ≤100 pts/monitor → sparkline ✅. No `avgPing` on this path.
- Uptime: `uptimeList["{id}_24"]` = **24h only**. 30d needs Socket.IO/Prometheus.
- Status-change: derive from heartbeat status flips (≤100-pt window).
- Deep-link: **none** — only the status page URL itself. `url` is the monitored target.
- Actions: **none** on status-page path (read-only, unauth). Pause/resume only via authenticated Socket.IO.
- Polling: plain GET, 1-min server cache. Socket.IO offers realtime push (bigger integration).
- Caveat: findings from master; older instances may differ — **needs a live instance to pin per-version**.

### Healthchecks.io — `GET /api/v3/checks/` (`X-Api-Key`)
- Read-only key **redacts** `uuid`,`ping_url`,`update_url`,`pause_url`,`resume_url`; adds `unique_key`. **Confirmed.**
- Latency: `GET /checks/<uuid>/pings/` → `duration` = **script run-time** (needs `/start` pings, RW key), not HTTP latency. No `last_duration` on the check.
- Uptime %: **none in the API** — would compute from flips.
- Status-change: `GET /checks/<uuid>/flips/` (works read-only via `<unique_key>/flips/`) → exact transition timestamps. ✅
- Deep-link: `/checks/<uuid>/details/` (RW key only). **Slug detail URLs don't exist** (tested 404; slugs are a *ping* convenience). ✅ confirmed.
- Actions: `POST /checks/<uuid>/pause` `/resume` (RW key).
- Polling: **no ETag/304**, no push; channels only list integrations. Keep < ~100 req/min.

---

## Next steps / what needs an account to verify (your constraint)

We currently have: **Watch4.me** (live token ✅), **BetterStack** + **Healthchecks**
(configured keys ✅). We do **NOT** have: **UptimeRobot** account, an **Uptime Kuma**
instance, BetterStack/Healthchecks **paid tiers**.

To fully verify before building any nudge, prioritized:

1. **UptimeRobot free account** (highest value — it's the #1 migration source).
   Verify: v3 vs v2 fields, the real response-time history window on free, whether
   a usable dashboard deep-link can be constructed from `id`.
2. **Uptime Kuma test instance** (Docker, 10 min). Verify status-page JSON fields
   against a pinned release; confirm sparkline/uptime/derived-status behavior.
3. **Healthchecks** — confirm `duration` behavior for checks *without* `/start`,
   and whether `flips` is the right "down since" source for the app.
4. **BetterStack** — measure the undocumented rate limit; confirm the N+1 cost is
   acceptable before surfacing latency/SLA.

## Implications for issue #5 (funnel nudges) — what's HONEST to build
- ✅ **Deep-link superiority** — the strongest true contrast. Watch4.me links work
  cleanly; BetterStack/Kuma can't, Healthchecks needs RW key, UptimeRobot unofficial.
- ✅ **"One fast call / conditional polling"** — a real efficiency story (Settings/Help copy).
- ❌ **Do NOT** claim sparklines/latency/uptime as Watch4.me-only — they're broadly available.
- ⚠️ **Remote mute/ack** as a contrast depends on Watch4.me shipping it (#710); pause
  exists on several providers, so frame around *mute/ack* specifically, not pause.
