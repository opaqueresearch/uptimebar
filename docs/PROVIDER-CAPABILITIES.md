# Provider Capability Matrix

What each supported provider's API actually exposes, audited 2026-06-27 against
each provider's own docs and (where open-source) source.

**Why this exists:** UptimeBar aims for feature parity across every provider its
API access permits. To do that we had to know precisely what each API offers —
and, just as importantly, what it doesn't, so we never imply a provider lacks a
capability it actually has. This doc is the source of truth for that, and the
guardrail is: *only surface a capability difference in the UI if it is real and
verified here.*

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
| **Latency history (sparkline)** | ✅ `response_history[]` (1 call) | ✅ `response_times[]` (~194 pts/24h, 1 call) — **free, re-verified 2026-06-28** | ⚠️ `/response-times` 24h, **N+1** | ✅ `heartbeatList` (≤100 pts) | ⚠️ `/pings/` run-time, RW key |
| **Uptime %** | ✅ `uptime_pct` | ✅ `custom_uptime_ratios` | ✅ `/sla` (N+1) | ⚠️ 24h only (`uptimeList`) | ❌ none (compute from flips) |
| **Status-change time ("down for Xm")** | ✅ `state_since` (1 call) | ✅ `logs=1` | ⚠️ via `/incidents` v3 (N+1) | ✅ derive from heartbeats | ✅ `/flips/` (even read-only) |
| **Per-monitor deep-link** | ✅ `public_id` | ✅ `/monitors/<id>` (unofficial route, verified) | ❌ team slug not in API | ❌ status page only | ⚠️ uuid → RW key only |
| **Remote pause/resume** | ❓ (#710 pending) | ✅ `editMonitor status` | ✅ `PATCH paused` | ❌ (needs auth Socket.IO) | ✅ `/pause` `/resume` |
| **Remote mute/ack** | ❓ (#710 pending) | ❌ | ⚠️ incident ack (not mute) | ❌ | ❌ |
| **Conditional polling (ETag/304)** | ✅ | ❌ | ❌ | ❌ (1-min server cache) | ❌ |
| **All data in ONE call** | ✅ status endpoint | ⚠️ one big getMonitors w/ params | ❌ N+1 fan-out | ⚠️ 2 calls (page+heartbeat) | ⚠️ list + per-check for detail |
| **Auth model fit (read-only viewer)** | ✅ Bearer | ✅ read-only key | ⚠️ no read-only scope | ✅ public, no key | ⚠️ RW key needed for detail/links |
| **Self-hostable** | ⚠️ (SaaS; self-host TBD) | ❌ SaaS only | ❌ SaaS only | ✅ self-hosted by design | ✅ open-source, self-hostable |
| **Custom base URL in UptimeBar today** | ✅ base_url | ❌ hardcoded (SaaS, fine) | ✅ base_url (vestigial — SaaS) | ✅ **required** (paste instance URL) | ✅ base_url ("supports self-hosted") |

---

## The honest strategic read

**Most per-monitor *capabilities* are NOT unique to Watch4.me.** Uptime and
status-timing exist almost everywhere. **Latency *history* (sparklines) is more
nuanced — and an earlier finding here was WRONG, corrected 2026-06-28:**
- **UptimeRobot free tier: real series available** — `response_times=1` WITHOUT
  an explicit date range returns the full retained window (**~194 points / ~24h**
  at 5-min buckets), live-re-verified. A real sparkline IS free-tier viable.
  ⚠️ The prior "1 point only" finding came from passing explicit
  `response_times_start_date/end_date` — *those params* truncate it; omit them and
  you get the whole series. **Caveat:** `value` is integer ms; series carries no
  per-bucket failure flag (failed checks are simply omitted), so its sparkline has
  no red failure dots (Watch4.me's `response_history` does).
- **BetterStack:** 24h only, **N+1** (a call per monitor) — costly.
- **Uptime Kuma:** genuinely has ≤100-point ping history (a real sparkline).
- **Healthchecks:** only job run-time, RW key, per-check call — not HTTP latency.

So sparklines are **less of a differentiator than first thought** — Watch4.me,
UptimeRobot, and Uptime Kuma all do a real one. **Do NOT lean on "we have
sparklines"** as a contrast. Watch4.me's *honest* edges remain (a) the whole fleet
in **one cheap ETag/304 call** (UptimeRobot pays a full re-fetch each poll; this
is the durable architectural win), and (b) sparklines with **per-bucket failure
markers** (the `failures` field) — a fidelity detail UptimeRobot's series lacks.

**Watch4.me's genuine, defensible differentiators are architectural, not feature-checkboxes:**

1. **Everything in one cheap call.** Watch4.me returns status + latency + uptime +
   state_since + deep-link id from a single endpoint. The others force **N+1
   fan-out** (BetterStack: a call per monitor for latency, another for SLA,
   another for incident timing) or split across calls. For an always-on menu-bar
   poller, that efficiency is real and felt.
2. **Conditional polling (ETag/304) — unique to Watch4.me.** Every other provider
   is full-fetch-every-poll. This is a true, verifiable "only Watch4.me" trait.
3. **Deep-links that just work.** Watch4.me `public_id` → specific monitor page,
   no caveats and via an official id. BetterStack **can't** (slug not in API).
   Healthchecks needs a read-write key. UptimeRobot links per-monitor only via an
   *unofficial* (undocumented) dashboard route. Uptime Kuma has no per-monitor
   page at all. So the honest contrast is now **"first-class/official vs.
   caveated"** — Watch4.me's is the only documented, no-asterisk one; the others
   each carry a caveat (missing, RW-key-gated, or unofficial).
4. **Read-only-friendly full fidelity.** Watch4.me gives everything to a simple
   Bearer token; Healthchecks cripples read-only keys, BetterStack has no
   read-only scope.

**Per-provider honest one-liners (safe to use in UI/marketing):**
- **UptimeRobot** — capable: one call gives latency, 30d uptime %, AND a ~24h latency series (real sparkline) on free tier; per-monitor deep-link works via an unofficial route. Weaknesses vs Watch4.me: full re-fetch every poll (no ETag/304) and no per-bucket failure markers in the series. Free tier 10 req/min.
- **BetterStack** — rich data, but spread across N+1 calls and **no per-monitor deep-link** (team slug isn't in the API).
- **Uptime Kuma** — great self-hosted data, but the status-page path has **no per-monitor links** and **no remote control**; 24h uptime only.
- **Healthchecks.io** — cron/heartbeat focused: **no uptime %**, "latency" is job run-time, and deep-links/detail need the **read-write** key.

---

## UptimeBar app differentiation (app-vs-app — OUR message to own)

> **Scope of this section (vs. the rest of the doc).** Everything above analyses
> **watch4.me as a provider** vs other monitoring services — that's watch4.me's
> market story and should feed **upstream to watch4.me** (centralized competitive
> research lives there). **This section is different:** it's **UptimeBar the app** vs.
> **other menu-bar / desktop uptime apps** — the differentiation UptimeBar *owns* and
> the website *cites*. Keep the two stories separate.
>
> Honesty gate: only list a differentiator that's **real and verified**; mark
> unverified/planned as "planned". The website must trace claims here.

**UptimeBar's genuine app-level differentiators (verify each before marketing):**
1. **Multi-provider in one glance.** One tray icon + popover spanning UptimeRobot,
   BetterStack, Healthchecks.io, watch4.me (Uptime Kuma planned) — most menu-bar
   uptime apps are single-provider (usually just UptimeRobot). ✅ real today.
2. **Truly cross-platform, native.** macOS menu bar **and** Windows system tray from
   one codebase (Tauri). Many competitors are macOS-only. ✅ real today.
3. **Read-write API control, where the provider allows it.** Not just *viewing* —
   UptimeBar can pause/resume (and mute/ack where supported) via provider APIs that
   expose it. Contrast: apps that are read-only status viewers. ⚠️ gate this per
   provider on the matrix above (pause exists on several; mute/ack is narrower).
4. **Honest failure semantics.** Provider errors map to **Unknown, not Down** after a
   threshold — a flaky API never manufactures a fake outage. A trust/quality
   differentiator most simple pollers lack. ✅ real today.
5. **First-class deep-links + latency sparklines with failure markers** — where the
   provider supplies them (best with watch4.me). Gate on the matrix (three providers
   have sparklines; the failure-marker fidelity is narrower). ⚠️ per-provider.

**Direct-competitor framing (for the website — fit, not attack):** position UptimeBar
as "the multi-provider, cross-platform, *does-things-not-just-shows-things* menu-bar
uptime app." Lead with the felt benefit; cite the specific capability as proof.

**Feed-up note:** competitive analysis of *provider* capabilities (the matrix above)
should be shared to watch4.me as `competitive-research` issues on `joej/watch4.me`.
This app-differentiation section stays app-owned.

---

## Customer-facing articulation (website / About-Help "bones")

> **Purpose of this section.** These are the verified, honest *bones* for the
> `uptimebar_website` marketing content and a future in-app About/Help → "How
> UptimeBar talks to your providers" note. Everything here is reproducible from
> public API docs — that's the point: a developer audience can *check* it, which
> is what makes it persuasive. **Lead with the felt benefit; let the numbers be
> the proof underneath.** Frame as *fit*, never as "competitor X is bad."

### The one-glance fleet test (the headline contrast)

The job a menu-bar uptime app actually does: **show every monitor's live latency
+ uptime in one cheap, always-on glance.** Score each provider by how many HTTP
calls that takes for ~30 monitors:

| Provider | Calls for 30 monitors' status + latency + uptime | Why |
|---|---|---|
| **Watch4.me** | **1 call** (then ~free) | Purpose-built `/monitors/status` aggregation endpoint + ETag/304 → steady state returns 304, nearly free |
| **UptimeRobot** | **1 call** | Aggregation bolted onto the list call via params (`response_times`, `custom_uptime_ratios`) |
| **BetterStack** | **~61 calls** (1 + 2×30) | No fleet-aggregation path: latency + SLA are **one call per monitor each** |
| **Uptime Kuma** | **2 calls** | Status-page + heartbeat endpoints (self-hosted; one published page) |
| **Healthchecks.io** | n/a | No uptime % and "latency" is job run-time — different problem domain |

The kicker line, and the **only true "only-Watch4.me" trait: conditional polling
(ETag/304).** Every other provider is full-fetch-every-poll; Watch4.me's steady
state is a 304 with no body. "It just stays current in the background, nearly for
free" is a benefit only Watch4.me can claim.

### Why BetterStack costs 61 calls — and why that's *not* a knock on BetterStack

This must be framed precisely or it reads as a hit piece (and a developer will
see through it). BetterStack's API is **mature and rich** — per-region latency
series, SLA with date ranges, a full incidents resource, write actions. It is
**not** under-built. The mismatch is **interaction model, not quality**:

- BetterStack's API is **resource-oriented / one-entity-per-call** — the textbook
  REST design, and the *right* one for **its** primary consumers: its own web
  dashboard (you drill into one monitor, *then* it loads that monitor's chart +
  SLA), Terraform/config-as-code, and incident-management integrations. None of
  those ever need "all monitors' stats at once," so no such endpoint exists.
- **Our** consumer is the opposite: an **at-a-glance fleet aggregator**. We want
  the whole fleet's stats in one shot, on a background timer.
- So the honest story is **"different tools, different shapes."** BetterStack's
  drill-down model is correct for drill-down consumers; Watch4.me's aggregation
  endpoint is correct for a menu-bar glance — *because Watch4.me built it for
  exactly this use case.* You're choosing the axis of comparison, and choosing
  one where the answer is honestly in our favor.

**The proxy caution:** the call-count is *evidence*, not the pitch. What the
customer feels is the consequence — faster popovers, lighter network/battery, no
rate-limit risk, "always current in the background." Latency/SLA numbers don't
change second-to-second, so a per-monitor fan-out is wasted work for a glance.

### Where the contrast is NOT in our favor (keep us honest)

- **Sparklines aren't "only Watch4.me"** — UptimeRobot (free, one call, ~194 pts)
  AND self-hosted Uptime Kuma both have real latency series. **Do NOT pitch
  "we have sparklines" as a differentiator** — three providers do. The honest
  fidelity edge is *narrower*: Watch4.me's series carries **per-bucket failure
  markers** (the `failures` field → red dots on the line); UptimeRobot's omits
  failed checks, so it can't mark outages on the sparkline. Lead with the ETag/304
  "stays current nearly for free" story instead — that one IS Watch4.me-only.
- BetterStack is a **bigger, broader product** — we lose a feature-checkbox war.
  That's *why* we compete on fit-for-a-menu-bar, not on feature count.

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

### UptimeRobot — `POST /v2/getMonitors` — ✅ LIVE-VERIFIED (free tier, read-only key, 2026-06-27)
- **One call returns everything** the app needs: monitor `id`, `friendly_name`,
  `url`, `status`, `average_response_time`, `custom_uptime_ratio`,
  `all_time_uptime_ratio`, `response_times`, `logs`. Read-only key works for all reads. ✅
- Latency **current**: `average_response_time` — returned as a **STRING**
  (e.g. `"409.531"`), not a number. Parse it. ✅
- Latency **history (sparkline)**: ✅ **CORRECTED 2026-06-28 — real series IS
  free-tier viable.** `response_times=1` with **NO date range** returns the full
  retained window (**~194 points / ~24h**, 5-min buckets), each `{datetime, value}`
  with `value` = integer ms, newest-first. The earlier "1 point only" result was
  an artifact of passing explicit `response_times_start_date/end_date` — *those*
  truncate the series; omit them. **Caveat:** no per-bucket failure flag (failed
  checks are omitted from the series), so the sparkline has no red markers.
- Uptime: ✅ `custom_uptime_ratios=1-7-30` → `"100.000-100.000-100.000"`;
  `all_time_uptime_ratio` works. Free tier. ✅
- Status-change ("down for Xm"): `logs=1` → `{type,datetime,duration}`. Live monitor
  returned `logs: []` (brand-new, no events yet) — **mechanism documented, empty
  until events occur; revisit once the monitor has history.** ⚠️
- Deep-link: no dashboard-URL *field* in the API, but the route is stable and
  **live-verified**: `dashboard.uptimerobot.com/monitors/<numeric id>` opens that
  monitor directly. We build it from `id`. (Unofficial but reliable.) ✅
- Actions: `editMonitor` pause/resume (RW key). No mute/ack.
- Polling: **no ETag/304**; rate limit free 10/min.

**Public status page JSON (NO key) — bonus finding.** A user's public status page
(`stats.uptimerobot.com/<id>`) exposes, unauthenticated, via
`GET stats.uptimerobot.com/api/getMonitorList/<pageId>`:
- per-monitor `dailyRatios` (**90 days** of daily uptime %), `30dRatio`,
  `90dRatio`, `ratio`, `lastDowntime`, `statusClass`, `name`, `monitorId`,
  plus top-level `statistics.counts {up,down,paused,total}`.
- **Uptime history is richer here than the free authed API** (90 daily points).
- **But NO response-time/latency** anywhere → still no sparkline source.
- Only exists if the user created a public status page; not a general path.

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

## What is honest to say (issue #5)

The rule: **a capability difference may only be mentioned if it is verified above
and still true.** Most of this section is a list of things *not* to claim.

- ❌ **Do NOT** claim sparklines / latency / uptime % as Watch4.me-only — they are
  **broadly available** across providers. Claiming otherwise would be false.
- ❌ **Do NOT** frame pause/resume as a differentiator — it works on UptimeRobot,
  BetterStack, and Healthchecks too, and UptimeBar implements it for all of them.
- ⚠️ **Mute/ack** is currently Watch4.me-only, and only once it ships (#710). If any
  other provider adds a mute API, this stops being true and comes out of the copy.
- ⚠️ **Deep-links** — Watch4.me's resolve cleanly via `public_id`; BetterStack has no
  team slug in the API, Healthchecks needs a RW key, UptimeRobot's route is
  unofficial. State the mechanism, never the word "superiority" — this is an API
  surface difference, not a product judgement, and it changes the day they add a
  field.
- ✅ **Conditional polling (ETag/304)** — a real, factual efficiency property worth
  documenting. Describe what it does; don't editorialize about who lacks it.

Anything found to be untrue, or made untrue by a provider shipping something, comes
out of the UI immediately. This list is a constraint on us, not a pitch.
