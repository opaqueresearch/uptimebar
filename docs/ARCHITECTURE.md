# Architecture

"Fat Rust core, thin web shell." The Rust side (`src-tauri/src`) owns all long-lived
state so monitoring keeps running even when no window is open; the frontend is an
ephemeral view onto it.

## Rust core (`src-tauri/src`)

| File | Responsibility |
| --- | --- |
| `providers/mod.rs` | `Provider` trait, normalized `Monitor` model, factory + registry |
| `providers/{watch4me,uptimerobot,betterstack,healthchecks,uptimekuma}.rs` | one adapter per backend |
| `poller.rs` | tokio scheduler — concurrent fetch, timeout, refresh signal |
| `state.rs` | `AppState` + transition detection (the notification gate) |
| `tray.rs` | tray icon (drawn in code), native menu, popover positioning + sizing |
| `notify.rs` | transition → native notification |
| `config.rs` | non-secret config (`tauri-plugin-store`) + secrets (keychain in release, 0600 file in dev) |
| `commands.rs` | the `#[tauri::command]` surface for the UI |

## Frontend (`src/`)

Vanilla TypeScript, two Vite entry points:
- `index.html` + `popover.ts` — the tray monitor list (status, latency/uptime detail,
  sparklines, per-monitor action buttons).
- `settings.html` + `settings.ts` — provider management, preferences.
- `icons.ts` — shared inline Lucide SVG icon helpers.

## Adding a provider

Drop a new file in `providers/`, add a match arm in `providers::build`, and an entry
to `providers::kinds_meta()`. Implement the `Provider` trait — at minimum
`fetch_monitors()`; optionally `fetch_detail()` (latency/uptime, on popover-open),
`monitor_action()` (pause/resume/mute), `capabilities()`, and `probe_scope()`. See
[`PROVIDER-INTEGRATION-NOTES.md`](PROVIDER-INTEGRATION-NOTES.md) for the hard-won
per-API techniques and gotchas.

## Behavior notes

- **Notifications fire only on Up↔Down transitions.** The first observation of a
  monitor sets a silent baseline (no startup notification storm).
- **Provider errors map to Unknown, not Down**, after `FAILURE_THRESHOLD` consecutive
  failures — a flaky API never masquerades as an outage.
- **Muted monitors** (Watch4.me) can be silenced from UptimeBar's own notifications
  too, via a global "silence muted monitors" preference (default on).
- **Popover order is frozen while open** — sorted by status on open/refresh, held
  stable so acting on a monitor doesn't reshuffle rows; status dots recolor in place.

## Secret storage

Secrets are **write-only from the UI** and kept out of the synced config store and out
of git. Where they live depends on the build:

- **Release builds** store keys in the **OS keychain** (macOS Keychain / Windows
  Credential Manager) via the `keyring` crate — encrypted at rest, per-app access.
  This requires a stable code signature; CI ad-hoc signs the macOS builds (see
  [`RELEASING.md`](RELEASING.md)) so the keychain is reliable.
- **Debug builds** (`tauri dev`) use a `0600` (owner-only) plaintext file
  (`secrets.json`) in the app data dir. The keychain binds each item to the app's code
  signature, so an unsigned dev build can't read back what a previous rebuild wrote
  (keys appeared to vanish after every recompile); the file survives rebuilds.
- On release, `get_secret` falls back to the file and **migrates** it into the
  keychain, so upgrading from an older file-based build doesn't lose keys.

See `config.rs` (`use_keychain` / `set_secret` / `get_secret`).

## Token scope (read vs. read-write)

Monitor *actions* (pause/resume/mute) need a read-write token; *reads* don't. UptimeBar
detects a token's scope so it can gate action buttons before a failed click:
- **Watch4.me** — authoritative `GET /api/v1/token` introspection.
- **UptimeRobot / Healthchecks** — side-effect-free proxies (a write-gated read call;
  a read-only key's field redaction, respectively).
- **BetterStack** — no clean signal; falls back to the runtime `403` on the first
  write attempt (surfaced as "needs a read+write token").

A read-only token still *shows* action buttons (so paused/muted state is visible) but
disabled. Full rationale + per-provider technique in
[`PROVIDER-INTEGRATION-NOTES.md`](PROVIDER-INTEGRATION-NOTES.md).
