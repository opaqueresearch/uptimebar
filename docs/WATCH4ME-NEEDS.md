# UptimeBar → Watch4.me: API & Integration Needs

**Audience:** the Watch4.me service/backend team (and any Claude session working
on the Watch4.me codebase).

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

---

## TL;DR for the Watch4.me team

If you do **one** thing: **add `public_id` to the `/api/v1/dashboard/`
response.** One field, no UptimeBar changes, instantly upgrades every Watch4.me
monitor click from "dump on dashboard" to "open the exact monitor" — better UX
and a stronger funnel into the web app.
