# Releasing & signing

## Cutting a release

Pushing a version tag triggers `.github/workflows/release.yml`:

```bash
# bump the version in package.json + src-tauri/{tauri.conf.json,Cargo.toml} first
git tag v0.5.0 && git push origin v0.5.0
```

CI builds installers on hosted runners — macOS **aarch64 + x86_64** `.dmg` and
Windows `.nsis` — and attaches them to a GitHub Release for that tag. (A Mac can't
build the Windows installer and vice-versa; CI is how we produce both.)

Download the latest at
[`/releases/latest`](https://github.com/opaqueresearch/uptimebar/releases/latest).

## Signing today (free, no Apple Developer account)

macOS builds are **ad-hoc signed** (`APPLE_SIGNING_IDENTITY: -`). This gives a
*stable* signature — which is what makes the keychain-backed secret storage reliable —
but does **not** make Gatekeeper trust the app. On first launch users must
**right-click → Open** (or `xattr -dr com.apple.quarantine UptimeBar.app`).

> A `gh`-downloaded `.dmg` isn't quarantined, so it launches without the prompt; a
> browser-downloaded one is.

## Trusted (notarized) builds, later

Enroll in the Apple Developer Program ($99/yr) and add these repo secrets —
`tauri-action` uses them with no workflow change: `APPLE_CERTIFICATE`,
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`,
`APPLE_TEAM_ID`. Notarization is automated (minutes); there is **no App Store review**
for a direct `.dmg` download.

## Notifications caveat (testing)

- **macOS notifications** are unreliable from unsigned `tauri dev` builds; test them
  from a signed `.app`.
- **Windows toasts** require the installed NSIS/MSI build (an AppUserModelID), not the
  raw dev exe.
