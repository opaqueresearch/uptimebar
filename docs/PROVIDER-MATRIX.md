# Provider matrix — what we needed vs. how each API let us solve it

A one-page, tear-away comparison of the monitoring provider APIs UptimeBar
integrates, framed as **the feature we needed → how (or whether) each provider let us
solve it**. Distilled from live integration; full detail in
[`PROVIDER-INTEGRATION-NOTES.md`](PROVIDER-INTEGRATION-NOTES.md), full capability
matrix in [`PROVIDER-CAPABILITIES.md`](PROVIDER-CAPABILITIES.md).

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

## The one-line verdict per provider

- **Watch4.me** — the reference: one-call status, ETag/304, float latency, idempotent
  actions incl. mute, real scope introspection, uniform errors. The bar the rest are
  measured against.
- **UptimeRobot** — capable data in one call, but a sloppy contract (string numbers,
  200-on-fail). Actions + a scope *proxy* both work.
- **BetterStack** — mature but drill-down-shaped (N+1, no fleet aggregation); actions
  work but scope can't be known without a 403.
- **Healthchecks** — different domain (cron/heartbeat): no HTTP latency or uptime %;
  actions + scope-by-redaction work with the read-write key.
- **Uptime Kuma** — self-hosted, read-only status-page path; no actions; retained but
  paused from the picker (low funnel yield).

## What we'd ask each provider to add

- **UptimeRobot:** JSON errors as real status codes; non-stringified numbers; an
  official deep-link field and a scope-introspection endpoint.
- **BetterStack:** a fleet-aggregation endpoint (the biggest gap); latency in ms; a
  token scope model + introspection; team slug / monitor URL for deep-links.
- **Healthchecks:** signal read-only scope explicitly instead of by field-redaction;
  an HTTP-latency metric distinct from job run-time.
- **Uptime Kuma:** a documented, versioned read API (the status-page JSON is a moving
  target).
