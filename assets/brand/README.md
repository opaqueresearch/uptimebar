# Brand assets — source of truth

The UptimeBar signal mark, shared with watch4.me (the funnel destination). These
are **owned assets** that replaced the previous icon of unknown provenance.

Design decision: [`0001-shared-brand-mark.md`](0001-shared-brand-mark.md) (ADR).

## The mark
A broadcast/signal glyph — a center dot with arc-waves radiating left and right —
in periwinkle (`#6372D6`) on a dark navy squircle (`#040A16`).

## Files
| File | Use |
|---|---|
| `watch4me-mark.svg` | Master — full 3-arc mark on dark squircle |
| `watch4me-512.png` | App/DMG icon source → generates `src-tauri/icons/*` |
| `uptimebar-template.svg` | Menu-bar mark (2-arc reduction), reference geometry |
| `uptimebarTemplate.png` / `@2x` | 18/36px macOS template images (reference) |
| `uptimebar-16Template.png` / `@2x` | 16/32px template images (reference) |
| `uptimebar-tray.ico` | Windows tray (fixed dark-slate) |

## How the icons are actually produced

- **App / DMG / bundle icon** (`src-tauri/icons/*.png`, `.icns`, `.ico`):
  generated from `watch4me-512.png` via `npx @tauri-apps/cli icon <path>`.
  Regenerate if the mark changes. (The mobile `android/` `ios/` output that
  command also emits is deleted — this is a desktop-only app.)

- **Menu-bar / tray status icon**: NOT a file. It's drawn at runtime in
  `src-tauri/src/tray.rs::signal_icon`, which renders this mark's geometry
  (center dot + one arc per side) in the live status color — green (all up),
  amber (degraded/unreachable), red (something down). This keeps the
  at-a-glance color signal a static template image can't provide, while
  matching the brand shape.

## Brand tokens
- Tile / dark bg: `#040A16`
- Accent: `#6372D6`
- Windows tray fixed fill: `#1b2740`
