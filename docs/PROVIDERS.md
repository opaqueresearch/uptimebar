# Connecting providers

Add a provider in **Settings → Add provider**: pick the service, name it, set the
Base URL (prefilled), and paste an API key. **Test connection** verifies it before
you save. Clicking a monitor in the menu-bar dropdown opens it in your browser —
where you land (the specific monitor vs. a list) depends on the service and key
type; see [Deep-links](#click-to-open--deep-links).

## Quick reference

| Service | Base URL | Auth | Key for UptimeBar | Per-monitor deep-link |
|---|---|---|---|---|
| **Watch4.me** | `https://watch4.me` | `Authorization: Bearer w4m_…` | API token | Dashboard today; per-monitor pending ([watch4.me#698]) |
| **UptimeRobot** | _(fixed, none)_ | `api_key` form field | Main API key (read-only OK) | Dashboard |
| **BetterStack** | `https://uptime.betterstack.com` | `Authorization: Bearer …` | Uptime API token (team-scoped) | ❌ opens dashboard — API lacks the team URL slug |
| **Healthchecks.io** | `https://healthchecks.io` (or self-hosted) | `X-Api-Key` | Project key — **full key** for deep-links | ✅ only with a full key (read-only omits the UUID) |
| **Uptime Kuma** | `https://kuma.host/status/<slug>` | none (public status page) | — | Status page |

## Per-service setup

### Watch4.me
- **Base URL:** `https://watch4.me`
- **Key:** create an API token in Watch4.me; it starts with `w4m_`.
- Reads `GET /api/v1/dashboard/` (`Accept: application/json`).
- **Deep-link:** opens the dashboard for now. Per-monitor links
  (`/monitors/<uuid>/`) turn on automatically once the API returns `public_id`
  — tracked in [watch4.me#698]. No app change needed when it ships.

### UptimeRobot
- **Base URL:** not needed (the API host is fixed).
- **Key:** Dashboard → **Integrations & API** (left sidebar) → **API** → create a
  **Main API key**. A read-only main key is enough to list monitors.
- **Deep-link:** opens the UptimeRobot dashboard.

### BetterStack
- **Base URL:** `https://uptime.betterstack.com`
- **Key:** **API tokens → Team-based tokens → _(your team)_ → Uptime API tokens.**
  Tokens are **team-scoped**.
- Reads the **first page** (50 monitors); additional pages are not fetched yet.
- **Deep-link:** ❌ not supported. The monitors API returns `team_name` but **not**
  the `t<id>` URL slug, so we can't build `/team/<slug>/monitors/<id>`. Clicking
  opens the BetterStack dashboard, which redirects to your team.

### Healthchecks.io
- **Base URL:** `https://healthchecks.io` (or your self-hosted instance).
- **Keys are per-project.** Select the project → **Settings → API Access** →
  create a key. Add **one UptimeBar provider per Healthchecks project** (a key only
  returns its own project's checks).
- **Key type matters:**
  - **Read-only key** — lists checks, but Healthchecks omits each check's `uuid`
    and ping URL, so clicking opens the general checks list.
  - **Full (read-write) key** — includes the `uuid`, so clicking deep-links to
    `/checks/<uuid>/details/`.
  - Use a **full key** if you want per-check links.

### Uptime Kuma
- **Base URL:** the **full public status-page URL**, e.g.
  `https://kuma.example.com/status/prod`.
- **Key:** none.
- Only monitors **published on that status page** are visible (it reads the
  status-page JSON). Full-account coverage via Socket.IO is a future enhancement.

## Click-to-open & deep-links

Clicking a monitor opens its provider page in your default browser. Whether you
land on the *specific* monitor depends on what the provider's API exposes:

- **Read-only keys can block deep-links.** Some services (Healthchecks) strip the
  per-monitor identifier from read-only API responses — use a full key for links.
- **Some data simply isn't in the API.** BetterStack doesn't return the team URL
  slug; Watch4.me doesn't yet return `public_id` ([watch4.me#698]).

When UptimeBar can't build a per-monitor URL, it opens the closest working page
(the dashboard or check list) rather than a broken link.

[watch4.me#698]: https://github.com/joej/watch4.me/issues/698
