# Provider integration notes

Hard-won, per-provider technical notes for anyone working on UptimeBar's provider
adapters (`src-tauri/src/providers/`). This captures the **techniques** each backend
required, the **quirks** that bit us, and **comparative API observations** (what's
missing or desirable). It complements — doesn't duplicate — the capability *matrix*
in [`PROVIDER-CAPABILITIES.md`](PROVIDER-CAPABILITIES.md); read that for "what's
supported," read this for "how we did it and what to watch out for."

> **Just want the at-a-glance comparison?** See the tear-away
> [`PROVIDER-MATRIX.md`](PROVIDER-MATRIX.md) — a one-page "what we needed → how each
> provider let us solve it" grid, also linked from the root README.

All findings are live-verified against real accounts unless noted. Dates are when
verified. The provider trait these implement is `Provider` in
`src-tauri/src/providers/mod.rs`.

---

## The shared model (how an adapter works)

Every provider normalizes into a common `Monitor` (id, name, status, url, deep-link,
`public_id`, `is_muted`, …) and implements the `Provider` trait:
- `fetch_monitors()` — the cheap always-on status poll.
- `fetch_detail()` — on-demand rich detail (latency/uptime/sparkline), fetched only
  when the popover opens (the "two-tier" model). `None` = no detail tier.
- `monitor_action(id, action)` — pause/resume/mute/unmute. Keyed by the **native
  `Monitor.id`**; each adapter maps that to whatever its own API addresses by.
- `capabilities()` → `ActionCaps { pause, mute }` — which action buttons the UI shows.
- `probe_scope()` → `read | write | unknown` — token scope, so the UI can gate write
  actions *before* the user clicks (vs. discovering scope by eating a 403).

**Token scope is the recurring theme.** Actions need write access; reads don't. The
cleanest signal is an authoritative scope endpoint (only Watch4.me has one). For the
rest we found provider-specific side-effect-free probes — or, where none exists, fall
back to the runtime 403.

---

## Watch4.me — the reference integration

**Base:** `https://watch4.me` · **Auth:** `Authorization: Bearer w4m_<token>`

- **Status (cheap tier):** `GET /api/v1/monitors/status` returns the whole fleet in
  **one call**, with a strong **ETag**. Send `If-None-Match`; steady state is a
  `304` with no body — near-free polling. Gotcha: a `304` has no body — you must
  handle it **before** `.json()` or you get a decode error.
- **Detail tier:** `GET /api/v1/dashboard/` → latency (`latest_response_time_ms`,
  a **float** — bind to `f64`, not `int`), `uptime_pct`, `response_history[]` with
  per-bucket `failures` (the only provider whose sparkline can mark outages).
- **Actions:** `POST /api/v1/monitors/{public_id}/{pause|resume|mute|unmute}`. No
  body; mute takes `?duration_seconds=N` (omit = indefinite). All **idempotent** —
  re-issuing returns `changed:false`. **Only provider with mute.** Note actions key
  on `public_id`, but our `Monitor.id` is the numeric id — the adapter resolves
  id→public_id from its cached status list.
- **Token scope:** `GET /api/v1/token` → `{scope: "read"|"write", …}`. **The clean,
  authoritative introspection endpoint** — the one we wish everyone had. (We asked
  for and it shipped as watch4.me#732.)
- **Errors:** uniform envelope `{"error":{"code","message"}}`; `403` code
  `insufficient_scope` for a read-only token on a write action; `plan_limit` on a
  blocked resume.

**Verdict:** the model the others are measured against — one-call status, ETag/304,
float-typed latency, idempotent actions, scope introspection, uniform errors.

---

## UptimeRobot

**API:** `POST https://api.uptimerobot.com/v2/*` (form-encoded) · **Auth:** `api_key`
form field. Read-only key suffices for reads; the **main key** is needed for writes.

- **Status + detail in ONE call:** `POST /v2/getMonitors` with
  `response_times=1&custom_uptime_ratios=30` returns status, `average_response_time`,
  uptime %, AND a ~194-point response-time series (free tier). Correction to an
  earlier assumption: the free tier **does** return a usable latency series *if you
  omit* the `response_times_start_date/end_date` params — those *truncate* it to one
  point. Omit them, get the full 24h window.
- **String-typed numbers (landmine):** `average_response_time` and
  `custom_uptime_ratio` come back as **strings** (`"409.531"`), not numbers. Binding
  to `f64` silently fails → blank fields. Parse the strings.
- **Auth errors as 200 (landmine):** a bad/insufficient key returns **HTTP 200** with
  `{"stat":"fail","error":{"type":"not_authorized",…}}` — *not* a 401. You must
  inspect the body, not the status code.
- **Actions:** `POST /v2/editMonitor` with `id` + `status` (`0`=pause, `1`=resume).
  A read-only key fails with `type:"not_authorized"`. No mute.
- **Scope probe (nice find):** `POST /v2/getAccountDetails` is effectively
  write-gated — a main key gets `stat:"ok"`, a read-only key gets `not_authorized`.
  A clean, side-effect-free way to detect scope at startup.
- **Deep-link:** no dashboard-URL field, but `dashboard.uptimerobot.com/monitors/<id>`
  is stable and verified (unofficial).
- **Polling:** no ETag/304; free rate limit 10/min.

**Desired from UptimeRobot:** JSON errors as real HTTP status codes (not 200+fail);
numeric types not stringified; an official deep-link field; a scope-introspection
endpoint (getAccountDetails works as a proxy but isn't purpose-built).

---

## BetterStack (Better Uptime)

**Base:** `https://uptime.betterstack.com` · **Auth:** `Authorization: Bearer <token>`

- **Resource-oriented, drill-down API (the fundamental mismatch):** built for
  one-monitor-at-a-time consumers (its own dashboard, Terraform, incident tooling),
  **not** a fleet glance. There is **no fleet-aggregation endpoint** — status is one
  list call, but latency + uptime are **one call *per monitor* each** (an N+1
  fan-out). We bound it to ~6 concurrent and only run it on popover-open.
- **Latency in SECONDS (landmine):** `GET /monitors/{id}/response-times` returns
  `response_time` in **seconds** (`0.546`), not ms — ×1000. Also **split by region**
  with **unsynchronized timestamps**, so naïve cross-region averaging yields a jagged,
  meaningless line. We pick **one representative region** (prefer us→eu→as→au).
- **Uptime:** `GET /monitors/{id}/sla` → `availability` (already a percentage).
- **Actions:** `PATCH /monitors/{id}` with JSON `{paused: bool}`. No mute.
- **No clean token scope (the hard case):** BetterStack has **no read-only scope**
  concept, and **no side-effect-free way to detect write access** — a read-capable
  token may still be rejected on write with 401/403, and the only "test" for write is
  *actually writing* (which would pause a monitor). So BetterStack is the one provider
  with **no proactive `probe_scope`** — it relies on the runtime 403, which we map to
  `insufficient_scope` (not the misleading generic "key rejected").
- **Deep-link blocker:** the API exposes `team_name` but **no team URL slug and no
  monitor URL**, so `/team/<slug>/monitors/<id>` can't be built from the API. The user
  must supply the team slug once (optional "Team" field) for click-through.
- **Polling:** pagination (`page`/`per_page`≤250); no ETag/304; rate limit undocumented.

**Desired from BetterStack:** a fleet-aggregation endpoint (status+latency+uptime in
one call) — the single biggest gap; latency in ms (or documented units); a token
scope model + introspection; the team slug / monitor URL in the API for deep-links.

---

## Healthchecks.io

**Base:** `https://healthchecks.io` (or self-hosted) · **Auth:** `X-Api-Key` header.
Cron/heartbeat-focused — a different problem domain than HTTP uptime.

- **Read-only key REDACTS fields (both a landmine and a gift):** a read-only key omits
  `uuid`, `ping_url`, `update_url`, `pause_url`, `resume_url` and substitutes a
  `unique_key`. This *broke deep-links silently* (no error — the data just wasn't
  there; we detect the redaction and warn). **But** it doubles as a **free scope
  probe:** any check with a top-level `uuid` ⇒ read-write key; all `unique_key`-only
  ⇒ read-only. We infer scope from the same `GET /api/v3/checks/` we already poll —
  zero extra calls.
- **"Latency" isn't HTTP latency:** `GET /checks/<uuid>/pings/` `duration` is the
  monitored **job's run-time** (needs `/start` pings + RW key), not response time.
  So Healthchecks has **no HTTP latency** and **no uptime %** in its API — we leave
  those blank rather than show something misleading.
- **Status-change:** `GET /checks/<uuid>/flips/` gives exact transition timestamps and
  works even read-only (via `<unique_key>/flips/`).
- **Actions:** `POST /checks/<uuid>/pause` and `/resume` — **require the read-write
  key** (a read-only key can't even address them, since it lacks the uuid). No mute.
- **Deep-link:** `/checks/<uuid>/details/` (RW key only). Slug-based detail URLs
  **don't exist** (tested 404; slugs are a ping convenience, not a detail route).
- **Key management pain (operational note):** Healthchecks issues **one** key each for
  read, read-write, and ping *per project*, shown **once** and unretrievable after.
  For UptimeBar, use the **read-write** key (it enables deep-links + actions).
- **Polling:** no ETag/304, no push. Keep < ~100 req/min.

**Desired from Healthchecks:** don't silently redact — signal read-only scope
explicitly (a field/header) instead of by absence; an HTTP-latency metric distinct
from job run-time; retrievable/multiple keys.

---

## Uptime Kuma

**Status-page JSON only** (no stable REST API; live data is Socket.IO). Config: the
full status-page URL (`.../status/{slug}`).

- **No official API:** we integrate the public status-page endpoints
  (`/api/status-page/{slug}` + `/api/status-page/heartbeat/{slug}`). Undocumented and
  **version-dependent** — the hardest thing to integrate against; findings are from
  master and older instances may differ.
- **Latency + uptime:** heartbeat `ping` (ms) gives a real ≤100-pt sparkline;
  `uptimeList["{id}_24"]` gives 24h uptime only.
- **No actions on this path** (read-only, unauthenticated). Pause/resume would require
  authenticated Socket.IO — a much bigger integration.
- **Deep-links:** none — only the status page URL itself.
- **Self-hosted:** by design; UptimeBar accepts a custom base URL.

**Status in UptimeBar:** currently offered read-only where a public status page
exists; **paused from the Add-provider picker** — the Kuma ecosystem already has
purpose-built menu-bar clients, so we'd be duplicating tools better suited to it.
Adapter retained so existing configs keep working. Removal tracked in #33.

---

## Cross-cutting lessons (the TL;DR for a new adapter)

1. **Never trust declared types blindly.** UptimeRobot stringifies numbers;
   BetterStack reports latency in seconds. Verify units + types against a live
   response before binding.
2. **Errors aren't always HTTP status codes.** UptimeRobot returns `200 {stat:fail}`.
   Inspect bodies.
3. **Read-only keys fail in provider-specific ways** — a clean 403 (Watch4.me), a
   `200 not_authorized` (UptimeRobot), field redaction (Healthchecks), or an
   ambiguous rejection (BetterStack). Map each to `InsufficientScope` so the UI shows
   "needs a read+write token," not a generic error.
4. **Proactive scope detection beats click-then-403.** Only Watch4.me exposes a real
   introspection endpoint, so for the rest we went looking: side-effect-free proxies
   found for UptimeRobot (`getAccountDetails`) and Healthchecks (uuid redaction).
   BetterStack has no signal available, so it falls back to the runtime 403 —
   surfaced as a real message rather than a silent failure. Three of four providers
   gate their buttons before the click; the fourth degrades honestly.
5. **Aggregation shape drives polling cost.** One-call-for-the-fleet (Watch4.me,
   UptimeRobot-with-params) vs. N+1 fan-out (BetterStack) is the difference between a
   cheap always-on glance and an expensive one. Fetch detail on-demand, never
   per-poll — that keeps the N+1 providers viable rather than penalizing their users.
6. **Deep-links are inconsistent** — verify the route resolves (we hit 404s on
   assumed slug routes for both Healthchecks and Watch4.me).
