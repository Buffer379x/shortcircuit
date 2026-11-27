# Branding

The ShortCircuit name stays unchanged. The tagline is **Take the shorter route.** [Design system](DESIGN_SYSTEM.md)

## The mark

Two circuit traces with round terminals, short-circuited by a bolt: the direct route replaces the detour. The trace ends are cut parallel to the bolt edges with an equal gap. The figure is point-symmetric. It refines the earlier lime mark, so the old and new marks are recognizably the same symbol.

| Asset | Use |
|---|---|
| `mark-dark.svg` (`#FF79C6`) / `mark-light.svg` (`#B52B7C`) | On dark or light backgrounds |
| `mark-mono-white.svg`, `mark-mono-black.svg`, `mark-current-color.svg` | Single-color reproduction |
| `logo-horizontal-{dark,light}.svg` | Mark and wordmark: sidebar, documents |
| `logo-tagline-{dark,light}.svg`, `logo-stacked-{dark,light}.svg` | Sign-in, setup, splash, print |
| `wordmark-{dark,light}.svg` | Text-only placements |
| `favicon.svg`, `favicon-16.png`, `favicon-32.png`, `favicon.png` | Browser tab. A hinted 16-unit design with a larger bolt, on a `#1C1E24` tile |
| `app-icon.svg`, `app-icon-{48…1024}.png` | App and launcher icon: the mark on a rounded `#1C1E24` tile |
| `apple-touch-icon-180.png`, `pwa-192.png`, `pwa-512.png`, `pwa-maskable-512.png` | iOS and the web manifest |
| `unraid-docker.png` (256 px) | Unraid Docker Manager icon |
| `readme-banner.{svg,png}`, `social-preview.{svg,png}` | Repository banner and link previews |

All files are in `assets/branding/`. They are generated from one geometry definition and the Geist wordmark outlines:
- `source/build_brand.py` writes the SVGs.
- `source/wordmark.py` outlines Geist with HarfBuzz shaping.
- `source/rasterize.cjs` renders the PNGs with Chromium.

Edit the source, not the outputs. The admin interface draws the logo inline (`frontend/src/lib/ui/Logo.svelte`, paths in `logo-paths.ts`) with the accent and text tokens, so it follows the theme.

## Rules

- **Clear space** around the mark is half its height. Minimum sizes:
  - Mark: 16 px, using the favicon design below 32 px.
  - Horizontal logo: 96 px wide.
- **Color.**
  - Use pink `#FF79C6` on dark, `#B52B7C` on light, or one of the single-color versions.
  - Never use the old lime.
  - No gradients, glows, outlines, or shadows on the mark.
- **Wordmark.**
  - Geist SemiBold with −2.2 % tracking, in the text color, never pink.
  - Do not split "Short" and "Circuit" into two colors or weights.
- **The trace motif** may appear once per surface as quiet decoration: two grey detours and a direct pink route that meet at one terminal. Examples are the README banner and the sign-in and setup backgrounds.
- **What branding does not touch.** Published short links, QR codes, saved QR designs, and colors set by users are never changed by branding updates.
