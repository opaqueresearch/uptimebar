# UptimeBar → Watch4.me: API & Integration Needs

**Audience:** the Watch4.me service/backend team working on the Watch4.me codebase.

**Context:** UptimeBar is a cross-platform menu-bar/tray uptime notifier and a
deliberate **funnel into the Watch4.me SaaS**. It is provider-agnostic (also
speaks UptimeRobot, Uptime Kuma, BetterStack, Healthchecks), but Watch4.me is
the first-party provider — the one backend we control on both ends. This doc is
the prioritized list of what UptimeBar needs *from the Watch4.me service* to
serve its users better and convert more of them into Watch4.me users.

## How UptimeBar consumes Watch4.me today

Single call, polled on an interval:

```
GET {base}/api/v1/dashboard/
Authorization: Bearer w4m_<token>
Accept: application/json          # JSON only when Accept is exactly this
```

Expected shape (fields the adapter reads — see `src-tauri/src/providers/watch4me.rs`):

```jsonc
{
  "monitors": [
    {
      "id": 123,                          // stable id (required)
      "name": "API",                      // display name
      "url": "https://api.example.com",   // monitored target
      "is_up": true,
      "is_paused": false,
      "is_stale": false,                  // data too old -> UptimeBar shows "Unknown"
      "latest_check_at": "2026-06-25T14:00:00Z",
      "public_id": "…"                    // OPTIONAL today, NOT returned — see P0 below
    }
  ]
}
```

Status mapping in UptimeBar: `is_paused` → Paused; else `is_stale` → Unknown;
else `is_up` → Up/Down. (A flaky/unreachable API maps to Unknown, never Down.)

---

## Prioritized needs

| Priority | Need | Effort (Watch4.me side) | Payoff |
| --- | --- | --- | --- |
| **P0** | Return **`public_id`** in dashboard JSON | tiny (one field) | per-monitor deep-links; client already supports it |
| **P0** | **Document/version** the integration endpoint + field contract | small | ship to customers without fear of silent breakage |
| **P1** | Confirm **list completeness / pagination** behavior | small | correctness with many monitors |
| **P1** | One-click **"Connect UptimeBar"** token flow | medium | funnel conversion (onboarding is where funnel apps leak) |
| **P2** | **Read-only** token scope | small–med | least-privilege; user trust |
| **P2** | Richer per-monitor fields (latency, down-duration, status-change time) | medium | richer popover; differentiates the first-party provider |

---

### P0 — Add `public_id` (the deep-link UUID) to the dashboard response

**This is the single highest-leverage ask.** UptimeBar's adapter already looks
for `public_id` and, when present, deep-links a monitor click to
`{base}/monitors/<public_id>/`. It is **not currently returned**, so every
Watch4.me monitor click falls back to `{base}/dashboard` — the generic
dashboard, not the specific monitor.

- **Client work required: none.** The code path exists
  (`watch4me.rs`, the `public_id` field + `detail_url` construction).
- **Watch4.me work: add one field** to the JSON the dashboard endpoint already
  returns — the same identifier already used in monitor page URLs.
- **Why it matters for the funnel:** a click that lands on the *specific*
  monitor page is a far stronger pull into the Watch4.me web UI than dumping the
  user on the dashboard. Competing providers (BetterStack, Healthchecks) already
  deep-link to the exact monitor; the first-party provider should not be worse.

### P0 — Document / version the integration endpoint

The adapter currently piggybacks on an endpoint documented as "live dashboard +
customer integrations," and depends on a fragile content-negotiation quirk (JSON
only when `Accept` is exactly `application/json` and not also `text/html`). For
an app shipped to customers we need either:

- a **versioned, documented integration endpoint**, or
- a written guarantee that the shape and these fields are stable:
  `id`, `name`, `url`, `is_up`, `is_paused`, `is_stale`, `latest_check_at`,
  and the new `public_id`.

### P1 — List completeness / pagination

The adapter consumes `monitors` wholesale with no paging. If a customer has many
monitors and the endpoint ever truncates or paginates, UptimeBar would silently
show a **partial** list — and the omitted monitor could be the one that's down.
Please confirm: is the returned list always complete? If it can paginate, how
(and we'll implement paging client-side)?

### P1 — One-click "Connect UptimeBar" token flow

Today a user must find and paste a `w4m_` token by hand. Funnel conversion would
improve markedly with a **"Connect UptimeBar" affordance in the Watch4.me web
UI** that mints a scoped token and hands it to the app with minimal friction —
e.g. a one-click copy page, or (better) a deep link / custom URL scheme that
passes the token back into UptimeBar. Onboarding friction is where funnel apps
leak users.

### P2 — Read-only token scope

A monitor *viewer* should use a least-privilege credential. Confirm `w4m_`
tokens can be minted **read-only**; it's both correct and reassuring to users.
(See the analogous Healthchecks.io guidance in `docs/PROVIDERS.md`.)

### P2 — Richer per-monitor fields (additive, optional)

UptimeBar's normalized model is lean, but Watch4.me — as the first-party
provider — could light up UX the others can't. All optional/additive:

- **Last response time / latency (ms)** → a "degraded but up" signal and
  sparklines; no other provider feeds this, so it would differentiate Watch4.me.
- **Current incident / outage duration** ("down for 14m") → more useful than a
  bare red dot.
- **Last status-change timestamp** → "up since…" / "down since…".
- **Explicit degraded/unknown health** beyond binary `is_up` → makes UptimeBar's
  "Problems" (down + degraded) filter shine for Watch4.me specifically.

### Attribution signal: the UptimeBar User-Agent

UptimeBar sends `User-Agent: UptimeBar/<version>` (e.g. `UptimeBar/0.2.0`) on
every request, so Watch4.me can already **identify and segment funnel traffic
from the app**. A future UptimeBar enhancement will enrich this to include OS +
arch (e.g. `UptimeBar/0.2.0 (macOS; aarch64)`) for finer analysis — worth
logging/segmenting on the Watch4.me side.

---

## TL;DR for the Watch4.me team

If you do **one** thing: **add `public_id` to the `/api/v1/dashboard/`
response.** One field, no UptimeBar changes, instantly upgrades every Watch4.me
monitor click from "dump on dashboard" to "open the exact monitor" — better UX
and a stronger funnel into the web app.

---

# Watch4.me team response (2026-06-26)

Status of each ask, and a new purpose-built endpoint for UptimeBar to adopt.

## What shipped

### `GET /api/v1/monitors/status` — a lean, conditionally-cacheable status surface ✅ LIVE

We built a dedicated endpoint for exactly UptimeBar's always-on polling use case,
instead of having you keep polling `/api/v1/dashboard/`. It is **live in
production now** and validated (200 + ETag, 304 conditional, rate-limit backstop,
all fields incl. `public_id`).

- Issue: watch4.me#706 · PR: watch4.me#709
- Design rationale: `docs/design/desktop-app-conditional-polling.md` (in the
  watch4.me repo) — why polling+ETag beats SSE for an always-on, ~0–5
  events/day client.

```
GET https://watch4.me/api/v1/monitors/status
Authorization: Bearer w4m_<token>
If-None-Match: "<last etag>"        # optional; send the ETag from your last 200
```

**200 response** (carries `ETag` + `Cache-Control: no-cache`):

```jsonc
{
  "monitors": [
    {
      "id": 123,                              // stable integer id
      "public_id": "550e8400-e29b-41d4-...",  // UUID — deep-link target (now returned)
      "name": "API",
      "url": "https://api.example.com",       // null for non-HTTP monitor types
      "is_up": true,
      "is_paused": false,
      "is_stale": false,                       // data too old -> show "Unknown"
      "state_since": "2026-06-25T11:48:00Z",   // when the CURRENT state began (see below)
      "latest_check_at": "2026-06-26T14:00:00Z"
    }
  ]
}
```

**304 Not Modified**: empty body, carries the `ETag`. Means "nothing changed —
reuse your cached list." This is the steady-state response and should be the vast
majority of your polls.

### Field contract (stable — build against these)

| Field | Type | Notes |
|---|---|---|
| `id` | int | Stable integer id. |
| `public_id` | string\|null | **UUID for deep-links** -> `https://watch4.me/monitors/<public_id>/`. |
| `name` | string | Display name. |
| `url` | string\|null | Monitored target; `null` for monitor types without a URL. |
| `is_up` | bool | Up/down. |
| `is_paused` | bool | Paused -> show Paused (takes precedence over up/down). |
| `is_stale` | bool | Data too old (no recent checks) -> show **Unknown**, never Down. |
| `state_since` | string\|null | ISO-8601 timestamp the **current** state began. Up: when it last came up. Down: when it last went down. Paused: `null`. |
| `latest_check_at` | string\|null | Timestamp of the most recent check. **Advances every check.** |

Status mapping is unchanged from what you already do:
`is_paused` -> Paused; else `is_stale` -> Unknown; else `is_up` -> Up/Down.

## How UptimeBar should use this

### 1. Switch the provider to `/api/v1/monitors/status`
Same Bearer auth you already use. The response is a strict subset of what you
read today (we dropped only the rolling-window fields you don't consume), so the
adapter change is small.

### 2. Implement conditional polling (the whole point)
- Cache the `ETag` and the last monitor list per provider.
- Send `If-None-Match: <last_etag>` on every poll.
- **Handle 304 *before* calling `.json()`** — a 304 has no body and will crash a
  blind `.json()` decode. On 304, skip parsing and reuse the cached list (so your
  diff logic finds no transition -> fires no notification, which is correct).
- Because `fetch_monitors(&self)` is immutable and providers are shared as
  `Arc<dyn Provider>`, the cached ETag + list need **interior mutability**
  (`Mutex<Option<String>>` / `Mutex<Vec<Monitor>>`), or live in `AppState` keyed
  by provider id — not plain `self.field = …` assignments.
- First poll after app restart / config change sends no `If-None-Match`, gets a
  full 200, and establishes a baseline with no spurious notifications (matches
  today's first-poll behavior).

### 3. Drive "down for Xm" from `state_since` (no extra request)
`state_since` only changes on a real status flip, so it costs nothing against the
304 rate. Compute duration locally: `now - state_since`. Do **not** expect a
server-side `duration_seconds` — it would be stale between checks.

### 4. Freshness display: use your own sync clock, not `latest_check_at`
The ETag deliberately **excludes** `latest_check_at` (it advances every check and
would near-zero the 304 hit rate). Consequence: during a long stable period you
get 304s and won't see a new `latest_check_at`. Show freshness from your **own
last-successful-sync clock** ("synced 12s ago") and rely on `is_stale` for
"checks stopped" — not on the per-monitor `latest_check_at` string.

### 5. Respect the rate-limit backstop
There is a per-credential backstop of **1 request / 5 seconds**. Your 10s poll
floor is comfortably clear of it, so it won't trip in normal operation. If you
ever do get a `429`, back off — do **not** mark monitors Unknown on a 429 alone
(treat it as transient, distinct from a real check failure).

### 6. Keep `/api/v1/dashboard/` for the detail tier (two-tier model)
This status endpoint is the cheap always-on tier. For latency, uptime %, and
sparklines in the popover, fetch `/api/v1/dashboard/` **on demand when the
popover opens** — not on every background poll. That keeps the always-on path
cheap while still powering the rich detail view when a human is looking.

## Status of your prioritized asks

| Ask | Status |
|---|---|
| **P0 — `public_id` in response** | Done. Returned by `/monitors/status` (and added to `/dashboard/` in watch4.me#699). Deep-links work. |
| **P0 — documented/versioned endpoint + stable field contract** | Done. `/api/v1/monitors/status` is a stable, decoupled contract (table above); independent of the dashboard's rolling-window internals so future dashboard changes can't silently break you. |
| **P1 — list completeness / no pagination** | Guaranteed. The full list is always returned, never paginated/truncated. Size is bounded per user by plan limits. (If we ever add an unlimited tier we'll add an *opt-in* cursor; clients that send none keep getting full lists.) |
| **P1 — one-click "Connect UptimeBar" token flow** | Tracked — watch4.me#710. |
| **P2 — read-only token scope** | Tracked — watch4.me#710. `/monitors/status` is the read-only viewer surface these tokens will pair with. |
| **P2 — richer per-monitor fields (latency, down-duration, status-change time)** | Addressed via the two-tier model. `state_since` gives you "down for Xm" on the status tier; latency/uptime/sparklines come from on-demand `/dashboard/` fetch. We intentionally did **not** put churning fields in the polled response (they'd destroy the 304 rate). |

## Cross-repo follow-ups
- **UptimeBar (this repo):** the client adoption above — switch endpoint, ETag
  caching with interior mutability, 304 handling, `state_since`-driven duration,
  two-tier popover fetch.
- **watch4.me#710:** read-only token scope + self-serve token UI (one-click
  "Connect UptimeBar"). Needed to remove the hand-paste-a-token funnel friction.

---

## Watch4.me response to your live-testing findings (2026-06-26)

Thanks for the field reports — both are accurate and useful. Verified against
the server source.

### Finding 1 — "the 304 didn't fire; I got a 200 with a new ETag"

**Working as designed, not a missed optimization.** The status ETag is a hash of
*state* fields only — `[id, name, url, is_up, is_paused, is_stale, state_since]`.
It deliberately **excludes `latest_check_at`**, so a routine check that finds the
same state does **not** flip the ETag. What *does* flip it is a real state change,
including a flap captured via `state_since`.

So the 304 rate tracks **how often your monitors change state**, not how often
they're checked:

- **Steady account** (nothing flapping): the vast majority of polls are 304s, as
  intended. Steady state really is cheap.
- **Busy/flapping account**: the ETag changes legitimately each time `state_since`
  moves. A 200 there is *correct* — the state genuinely changed and you need the
  new body. There's nothing to "fix"; a 304 would be wrong.

One gotcha that can mimic "ETag won't settle": on a **freshly created** monitor,
`state_since` and `latest_check_at` can coincide for the first checks while the
monitor establishes its baseline. Once it's stable, `state_since` stops moving and
the ETag stabilizes. If you tested seconds after adding monitors, that's likely
what you saw. Your handling (304 → reuse cache, 200 → reparse) is exactly right
either way — keep it.

There is also a server-side **15s cache** on the row computation, so even a 200
that you can't avoid costs us one cached read, not a fresh DB scan, per user per
15s window.

### Finding 2 — detail (`/dashboard/`) field names — confirmed, here's the contract

Your discoveries are correct. The authoritative per-monitor schema in the
`/api/v1/dashboard/` response (`monitors[]`) is:

| Field | Type | Notes |
|---|---|---|
| `id` | int | |
| `public_id` | str \| null | deep-link id |
| `name` | str | |
| `type` | str | `"http"`, etc. |
| `url` | str \| null | |
| `is_up` / `is_paused` / `is_stale` | bool | |
| `check_interval_seconds` | int | |
| `uptime_pct` | float \| null | **not** `uptime_percentage`/`uptime` |
| `latest_response_time_ms` | **float** \| null | **not** `response_time_ms`/`latency_ms`. It's a float (e.g. `906.26`) — don't bind it to an int. |
| `latest_check_at` | str (ISO-8601) \| null | |
| `response_history` | `ResponseHistoryPoint[]` | hourly buckets — real sparkline data |

`ResponseHistoryPoint`:

| Field | Type | Notes |
|---|---|---|
| `bucket` | str (ISO-8601) \| null | bucket start |
| `avg_ms` | float \| null | |
| `min_ms` | float \| null | |
| `max_ms` | float \| null | |
| `failures` | int | count in bucket; drives per-segment red/green coloring |

Good catch capturing `response_history` — that *is* the sparkline source the
watch4.me dashboard itself renders from, so you get the same data the web UI uses.
Note `latest_response_time_ms` being a float is load-bearing: we hit an HTTP 500
on the JSON path once for exactly this reason (pydantic rejecting `906.26` for an
int field), so bind it as a float on your side too.

These dashboard field names are stable, but they live on the **detail tier** —
keep fetching them on demand (popover open / explicit refresh), not on the poll
loop, so the 304 economics above stay intact.
