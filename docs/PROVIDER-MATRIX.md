# Provider matrix — what we needed vs. how each API let us solve it

A one-page, tear-away comparison of the monitoring provider APIs UptimeBar
integrates, framed as **the feature we needed → how (or whether) each provider let us
solve it**. Distilled from live integration; full detail in
[`PROVIDER-INTEGRATION-NOTES.md`](PROVIDER-INTEGRATION-NOTES.md), full capability
matrix in [`PROVIDER-CAPABILITIES.md`](PROVIDER-CAPABILITIES.md).

**This is an engineering record, not a scorecard.** It exists so the next person
touching an adapter knows what each API does and doesn't give us. A ⚠️ or ❌ is a
note about an API surface, not a judgement about a product — these are mature
services solving problems broader than a menu-bar client's.

**Every gap here that could be worked around, was.** UptimeBar's goal is feature
parity across providers wherever the API permits it, and the workarounds are in
[The parity work](#the-parity-work) below. Where a capability is genuinely absent
from an API, we say so and move on; where it's awkward, we wrote code. The one
asymmetry we can't engineer away: we can add endpoints to Watch4.me because we
run it, and we can't do that for anyone else — so gaps elsewhere stay gaps until
that provider chooses to close them.

Legend: ✅ clean / native · ⚠️ workaround or caveat · ❌ not possible via the API

| What UptimeBar needed | Watch4.me | UptimeRobot | BetterStack | Healthchecks | Uptime Kuma |
|---|---|---|---|---|---|
| **Fleet status in one cheap call** | ✅ `/monitors/status` + ETag/304 (near-free steady state) | ⚠️ one `getMonitors` w/ params | ❌ N+1 (per-monitor for detail) | ⚠️ list + per-check | ⚠️ 2 calls (page + heartbeat) |
| **Current latency** | ✅ `latest_response_time_ms` (float) | ⚠️ `average_response_time` — **string-typed** | ⚠️ per-monitor, **in seconds** | ❌ none (job run-time only) | ✅ heartbeat `ping` (ms) |
| **Latency history (sparkline)** | ✅ `response_history[]` w/ failure marks | ✅ ~194 pts free (omit date range) | ⚠️ per-region, unsynced → pick one region | ❌ | ✅ heartbeat series |
| **Uptime %** | ✅ `uptime_pct` | ✅ `custom_uptime_ratios` | ✅ `/sla` (N+1) | ❌ not in API | ⚠️ 24h only |
| **Status-change time ("down for Xm")** | ✅ `state_since` | ✅ `logs=1` | ⚠️ via `/incidents` (N+1) | ✅ `/flips/` | ⚠️ derive from heartbeats |
| **Per-monitor deep-link** | ✅ `public_id` | ⚠️ unofficial `/monitors/<id>` route | ❌ team slug not in API | ⚠️ uuid → RW key only | ❌ status page only |
| **Actions: pause/resume** | ✅ | ✅ `editMonitor` | ✅ `PATCH {paused}` | ✅ `/pause` `/resume` (RW key) | ❌ Socket.IO only |
| **Actions: mute/unmute** | ✅ (only provider with mute) | ❌ | ❌ | ❌ | ❌ |
| **Detect token scope up front** | ✅ `GET /api/v1/token` (real introspection) | ⚠️ proxy: `getAccountDetails` write-gated | ❌ no signal → runtime 403 only | ⚠️ proxy: RO key redacts `uuid` | n/a (unauth) |
| **Errors as real HTTP codes** | ✅ uniform envelope | ❌ `200 {stat:"fail"}` | ✅ | ✅ | ⚠️ raw |

## The landmines (what bit us)

- **UptimeRobot** — numbers come back as **strings** (`"409.531"`); auth failures are
  **`HTTP 200`** with `{stat:"fail"}`, not 401. Inspect bodies, parse strings.
- **BetterStack** — latency is in **seconds**, not ms; split by region with unsynced
  timestamps (pick one region, don't average). **No read-only scope** → the only way
  to know write access is to try and get 403.
- **Healthchecks** — a read-only key **silently redacts** `uuid`/URL fields (broke
  deep-links with no error). Same redaction doubles as a free scope probe.
- **Uptime Kuma** — no stable API; undocumented status-page JSON that **varies by
  version**.

## The parity work

What we built to close gaps the APIs left open. This is the point of the doc: a
⚠️ in the table above usually means *we wrote something*, not that we gave up.

| Provider | The gap | What UptimeBar does about it |
|---|---|---|
| **UptimeRobot** | Numbers arrive as strings (`"409.531"`); auth failure is `200 {stat:"fail"}` | Parse and coerce; inspect bodies rather than trusting status codes. The adapter absorbs the contract so the UI sees clean types. |
| **UptimeRobot** | No scope-introspection endpoint | Side-effect-free probe via a write-gated read call, so action buttons are gated *before* a failed click rather than after |
| **UptimeRobot** | No official deep-link field | Unofficial `/monitors/<id>` route, documented as unofficial so nobody mistakes it for supported |
| **BetterStack** | No token scope signal at all | Runtime `403` fallback surfaced as "needs a read+write token" — actions still work, the user gets a real message instead of a silent failure |
| **BetterStack** | Latency per-region, in seconds, timestamps unsynced | Convert to ms; pick one region rather than averaging incomparable series |
| **BetterStack** | No fleet aggregation (N+1) | Concurrent fetch with bounded timeouts so the N+1 doesn't feel like one |
| **Healthchecks** | Read-only keys silently redact `uuid` (deep-links broke with no error) | Turned the redaction into a free scope probe — the bug became the signal |
| **Healthchecks** | No HTTP latency (cron/heartbeat domain, not an API gap) | Surface what exists (`/flips/` for state-change time) rather than faking a metric the service doesn't measure |

Two things we won't do: fake a metric a provider doesn't measure, or lean on an
unofficial route without labelling it. Where the data isn't there, the UI says so.

## The one-line summary per provider

- **Watch4.me** — one-call status, ETag/304, float latency, idempotent actions incl.
  mute, scope introspection, uniform errors. We run it, so when UptimeBar needs an
  endpoint we can add one — see [`WATCH4ME-NEEDS.md`](WATCH4ME-NEEDS.md) for what's
  still missing there.
- **UptimeRobot** — capable data in one cheap call. Contract quirks (string numbers,
  200-on-fail) are absorbed in the adapter. Actions and a scope proxy both work.
- **BetterStack** — mature and drill-down-shaped; built for dashboards rather than
  fleet polling. Actions work; scope needs the 403 fallback.
- **Healthchecks** — a different domain (cron/heartbeat), so no HTTP latency or
  uptime % — not a gap so much as a different product. Actions and scope-by-redaction
  work with a read-write key.
- **Uptime Kuma** — self-hosted, status-page path only; no stable API. Paused from the
  picker: the Kuma ecosystem already has purpose-built menu-bar clients, so we'd be
  duplicating better-suited tools. Removal tracked in #33.

## What we'd ask each provider to add

These are genuine asks, not rhetorical ones. Every item is something that would
let UptimeBar show parity for that provider's users — the same features their
users can already see for other services in the same app. We'd rather file these
upstream than document a gap forever.

- **UptimeRobot:** JSON errors as real status codes; non-stringified numbers; an
  official deep-link field and a scope-introspection endpoint.
- **BetterStack:** a fleet-aggregation endpoint (the biggest gap); latency in ms; a
  token scope model + introspection; team slug / monitor URL for deep-links.
- **Healthchecks:** signal read-only scope explicitly instead of by field-redaction;
  an HTTP-latency metric distinct from job run-time.
- **Uptime Kuma:** a documented, versioned read API (the status-page JSON is a moving
  target).
