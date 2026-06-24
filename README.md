# UptimeBar

A lightweight, always-on **macOS menubar / Windows system-tray** uptime notifier.
It polls one or more monitoring backends, reflects aggregate up/down status in the
tray icon, lists individual monitors in a popover, and fires a native OS
notification the moment a monitor transitions up↔down.

Companion to [Watch4.me](https://watch4.me), but **provider-agnostic** — it also
speaks UptimeRobot and Uptime Kuma.

## Architecture

"Fat Rust core, thin web shell." The Rust side (`src-tauri/src`) owns all
long-lived state so monitoring keeps running even when no window is open:

| File | Responsibility |
| --- | --- |
| `providers/mod.rs` | `Provider` trait, normalized `Monitor` model, factory + registry |
| `providers/{uptimerobot,watch4me,uptimekuma}.rs` | one adapter per backend |
| `poller.rs` | tokio scheduler — concurrent fetch, timeout, refresh signal |
| `state.rs` | `AppState` + transition detection (the notification gate) |
| `tray.rs` | tray icon (drawn in code), native menu, popover positioning |
| `notify.rs` | transition → native notification |
| `config.rs` | non-secret config (`tauri-plugin-store`) + secrets (0600 plaintext file today; OS keychain intended for signed builds — see Behavior notes) |
| `commands.rs` | the `#[tauri::command]` surface for the UI |

The frontend (`src/`) is vanilla TypeScript: `popover.ts` (the tray list) and
`settings.ts` (provider management). Two Vite entry points: `index.html` (popover)
and `settings.html`.

## Providers

> **Setup, API-key locations, key types, and deep-link support per service:
> [docs/PROVIDERS.md](docs/PROVIDERS.md).**


- **UptimeRobot** — POSTs to `getMonitors` with an account API key.
- **Watch4.me** — `GET {base}/api/v1/dashboard/` with `Authorization: Bearer w4m_<token>`
  and `Accept: application/json`. This is Watch4.me's existing dashboard API
  ("for live dashboard + customer integrations"), which returns per-monitor
  `is_up`/`is_paused`/`is_stale` + `latest_check_at` + stable `id`. No server change
  needed. (The `/monitors/export` endpoint is config-only and is NOT used here.)
- **Uptime Kuma** — reads the public status-page JSON (set `base_url` to the full
  `…/status/{slug}` URL). Only monitors on that status page are visible; full
  Socket.IO support is a later enhancement.
- **BetterStack** — `GET /api/v2/monitors` with `Authorization: Bearer <token>`
  (first page).
- **Healthchecks.io** — `GET /api/v3/checks/` with `X-Api-Key: <key>`; works against
  the hosted service or a self-hosted instance via `base_url`. Read-only key is enough.

Add a provider type by dropping a new file in `providers/`, adding a match arm in
`providers::build`, and an entry to `providers::KINDS`.

## Behavior notes

- **Notifications fire only on Up↔Down transitions.** The first observation of a
  monitor sets a silent baseline (no startup notification storm).
- **Provider errors map to Unknown, not Down**, after `FAILURE_THRESHOLD`
  consecutive failures — a flaky API never masquerades as an outage.
- **Secrets are write-only from the UI** and are kept out of the synced config
  store and out of git. They are **currently stored in a plaintext JSON file**
  (`secrets.json`) in the app's local data dir, with `0600` (owner-only)
  permissions — *not* encrypted, *not* in the OS keychain. This is a deliberate
  workaround: the keychain binds each item to the app's code signature, so an
  unsigned dev build can't read back what a previous rebuild wrote (keys
  appeared to vanish after every recompile). The 0600 file survives rebuilds.
  **Released (code-signed) builds SHOULD store secrets in the OS keychain**
  (macOS Keychain / Windows Credential Manager) for encryption-at-rest and
  per-app access control; the plaintext file should remain only as the
  unsigned-dev fallback. See `config.rs` (`set_secret`/`get_secret`).

## Development

Prereqs: Rust (rustup) and Node.

```bash
npm install
npm run tauri dev      # launches the tray app
npm run tauri build    # bundles .app/.dmg (macOS) and .nsis (Windows)
```

There is no Dock/taskbar entry — the app lives only in the menubar/tray. Click the
tray icon for the monitor list; right-click for Refresh / Settings / Launch at
login / Quit.

> **macOS notifications** are unreliable from unsigned `tauri dev` builds; test
> them from a signed `.app`. **Windows toasts** require the installed NSIS/MSI
> build (an AppUserModelID), not the raw dev exe.
