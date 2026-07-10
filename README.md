# UptimeBar

A lightweight, always-on **macOS menu-bar / Windows system-tray** uptime notifier. It
polls one or more monitoring backends, reflects aggregate up/down status in the tray
icon, lists individual monitors in a popover, and fires a native OS notification the
moment a monitor transitions up↔down. From the popover you can also **pause, resume,
and mute** monitors — acting on them without opening a browser.

Companion to [Watch4.me](https://watch4.me), but **provider-agnostic**.

## Install

Download the latest `.dmg` (macOS) or `.exe` (Windows) from
**[Releases](https://github.com/opaqueresearch/uptimebar/releases/latest)**.

> macOS builds are ad-hoc signed (not yet notarized) — on first launch,
> **right-click → Open**. See [docs/RELEASING.md](docs/RELEASING.md).

There's no Dock/taskbar entry — the app lives only in the menu bar / tray. Click the
icon for the monitor list; right-click for Refresh / Settings / Launch at login / Quit.

## Supported providers

Connect one or more monitoring services with a read (or read-write, for actions) API
token. Setup and key locations per service: **[docs/PROVIDERS.md](docs/PROVIDERS.md)**.

- **[Watch4.me](https://watch4.me)** — the best-integrated provider (one-call status,
  latency sparklines, deep-links, pause/resume/mute).
- **UptimeRobot**
- **BetterStack**
- **Healthchecks.io** (hosted or self-hosted)

*Uptime Kuma is supported read-only where a public status page exists, but is not
actively pursued — the Kuma ecosystem already has purpose-built menu-bar apps.*

For how the app compares these APIs and what each supports, see the tear-away
**[provider matrix](docs/PROVIDER-MATRIX.md)**.

## Develop

Prereqs: Rust (rustup) and Node.

```bash
npm install
npm run tauri dev      # launches the tray app
npm run tauri build    # bundles .app/.dmg (macOS) and .nsis (Windows)
```

## Docs

- **[Architecture](docs/ARCHITECTURE.md)** — how the app is built (Rust core / web
  shell), secret storage, token scope, adding a provider.
- **[Providers](docs/PROVIDERS.md)** — per-service setup, API-key locations, key types.
- **[Provider matrix](docs/PROVIDER-MATRIX.md)** — one-page "what we needed → how each
  API let us solve it" comparison.
- **[Provider integration notes](docs/PROVIDER-INTEGRATION-NOTES.md)** — deep per-API
  techniques, quirks, and landmines.
- **[Provider capabilities](docs/PROVIDER-CAPABILITIES.md)** — the full capability
  audit matrix + evidence.
- **[Releasing & signing](docs/RELEASING.md)** — tagging a release, macOS signing,
  notarization.

## License

Apache-2.0 — see [LICENSE](LICENSE) and [NOTICE](NOTICE).
