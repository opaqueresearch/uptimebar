# ADR 0001 — Shared brand mark for watch4.me and uptimebar

## Status
Accepted — 2026-06-30

## Context
uptimebar (a remake of the defunct uptimebar.app, originally a generic globe
icon) is being repositioned as a macOS menu-bar / Windows-tray funnel into
watch4.me. We needed an icon strategy: one shared mark, or two similar marks.

watch4.me already ships an "almost icon": a broadcast/signal glyph — a center
dot with concentric arc-waves radiating left and right — in periwinkle
(#6372D6) on a dark navy squircle (#040A16), paired with a lowercase geometric
wordmark ("watch4" white, ".me" periwinkle).

Two surfaces with opposite themes constrain the design:
- watch4.me web/app: dark.
- uptimebar menu bar: light (and the macOS menu bar tints template images).

## Decision
Adopt a single shared mark — the existing signal/broadcast glyph — across both
products, with surface-specific renderings rather than one fixed asset.

- watch4.me (dark web/app): full 3-arc signal, #6372D6 on #040A16 squircle.
- Small favicons (16/32/48): 2-arc simplified signal — the 3-arc version
  merges into mush below ~48px.
- uptimebar (light menu bar): NO tile. Bare 2-arc glyph as a black-on-
  transparent macOS *Template* image (filename ends in `Template`), so the
  OS tints it (dark on light bar, white on dark/active). A dark squircle tile
  was explicitly rejected — it renders as a black blob on a light bar.
- Windows tray: bare 2-arc glyph as a fixed dark-slate (#1b2740) multi-size
  .ico, since trays do not reliably auto-tint.

## Rejected alternatives
- Keep the legacy globe: generic, non-uptime-specific, disintegrates at 16px.
- W-with-arrow monogram: more ownable and "up"-reading, but discards the
  signal-mark equity already shipped on watch4.me. Held in reserve.
- Keep the exact 3-arc glyph at all sizes: thin concentric arcs merge below
  ~48px; unusable in a menu bar.

## Consequences
- One mark, two products, instantly recognizable as the same family (funnel).
- Two glyph variants to maintain (3-arc rich / 2-arc reduced).
- Menu-bar asset is a template image, not a colored tile — colors come from
  the OS, not the file.
- Accent color #6372D6 and tile color #040A16 are now load-bearing brand
  tokens; record them wherever brand tokens live.
