# Design system

The local preview uses sample data and runs without cloud services. Routes are `#overview`, `#links`, `#link/1`, `#analytics`, `#qr`, `#health`, `#domains`, `#settings`, and `#login`. The dashboard and link list both open the Create Link dialog. The theme switch is in the top right. Settings offers English, German, and a system theme. On small screens, navigation opens in a side panel. The interface and sign-in use functional copy. [Design preview](../prototype/index.html)

## Color, typography, and layout

Graphite backgrounds and neutral borders structure tables and controls. Electric lime marks primary actions, active navigation, and the current time series. Statuses and errors also use text and icons, and some chart series use distinct line patterns. Numbers use tabular figures. Slugs and hostnames use monospace. The preview uses system fonts.

| Role | Dark | Light |
|---|---|---|
| Page background | `#101517` | `#f3f5ee` |
| Surface | `#192124` | `#fdfefb` |
| Raised surface | `#212b2e` | `#eef1e8` |
| Border / controls | `#303b3e` / `#59696c` | `#d8e0d5` / `#808f82` |
| Primary / secondary text | `#f3f6f2` / `#a3b0ab` | `#192320` / `#43564d` |
| Primary action background / text | `#d3fc51` / `#18220b` | `#d3fc51` / `#18220b` |
| Accent text on a surface | `#d3fc51` | `#314b00` |
| Success / warning / error / info | `#75dca6` / `#f5bf67` / `#ff8989` / `#9cbaff` | `#146140` / `#754900` / `#a02b35` / `#284fa1` |

Calculated token contrasts include **14.0:1** for dark text on lime. In dark and light themes respectively, secondary text on the surface measures **7.28:1 / 7.75:1**, muted text measures **5.57:1 / 6.01:1**, and accent text measures **13.87:1 / 9.71:1**. Verify WCAG 2.2 AA in the production interface, including interaction states, control contrast, keyboard operation, and screen reader behavior. [WCAG 2.2](https://www.w3.org/TR/WCAG22/)

The layout uses a 4 px grid. Spacing tokens are 4, 8, 12, 16, 20, 24, and 32 px. Corner radii are 5, 8, and 12 px. The preview's primary controls are 38–39 px high. The product target for touch controls is 44 px, with any exceptions in dense tables subject to review. Icons are 17 px, body text is 14 px, and page headings range from 28 to 40 px. Navigation switches at 920 px and complex views become more compact at 690 px. Tables scroll within their own containers. Primary row actions remain accessible at narrow widths. [Design tokens](../prototype/tokens.css)

## Components and workflows

- Share navigation, page headers, buttons, status badges, fields, and table components across views. Show validation and errors next to the relevant field. Give icon buttons accessible names.
- The dashboard preview shows a time range, key metrics, a main chart, destination health, and recent links. The links view demonstrates search, status and domain filters, sorting, selection, and a sample JSON export.
- The Create Link dialog accepts a destination, optional slug, and tags. An expandable section holds additional settings. The preview displays duplicate slug errors, saves changes locally, and opens the link details with a copy action. Permanent redirects carry a cache warning.
- QR Studio previews scannable codes for six sample addresses. The controls cover error correction, quiet zone, color, transparency, and SVG and PNG downloads. The color warning compares luminance. Verify the product exports for contrast and scanability with an independent decoder.
- Health shows sample probe results and labels simulated actions. Domains demonstrates creating and editing a domain. Settings groups appearance, language, privacy, and operations.
- Sign-in demonstrates local form states. The product implementation covers first-run setup, authentication, API integration, redirects, live health intervals, QR generation for arbitrary URLs, full settings, and integration jobs.
- Product components need visible focus, loading and empty states, inline validation, and confirmation for irreversible actions. Keep English and German UI strings in a shared typed Svelte catalog. Store timestamps in UTC and format dates and numbers for the selected locale.

## Branding

| Concept | Description |
|---|---|
| Circuit with lightning bolt | Preferred design. Two open circuit traces frame the bolt, and its solid silhouette stays legible at small sizes. |
| Branch | A node with branches represents rules and destination domains. [Branch concept](../assets/branding/concept-branch.svg) |
| Jump | A simplified arrow represents a redirect. [Jump concept](../assets/branding/concept-jump.svg) |

[Dark logo](../assets/branding/logo-dark.svg) · [Light logo](../assets/branding/logo-light.svg) · [Wordmark](../assets/branding/wordmark-dark.svg) · [App icon](../assets/branding/app-icon.svg) · [Favicon](../assets/branding/favicon.svg) · [192 px PWA icon](../assets/branding/pwa-192.png) · [512 px PWA icon](../assets/branding/pwa-512.png) · [Unraid icon](../assets/branding/unraid-docker.png)

Additional transparent PNG and SVG variants are in `assets/branding/`. The wordmark uses DejaVu Sans outlines. The preview uses local path icons, and the product is expected to use selected Lucide icons after license review. The demo QR fixtures were generated offline with the MIT-licensed generator identified in the fixture file. [DejaVu license](https://dejavu-fonts.github.io/License.html) · [QR Code Generator](https://github.com/kazuhikoarase/qrcode-generator)

The preview follows the written design brief. Review against the graphics in the separate ShortCircuit chat is pending.

## Design renders and browser verification

| View | Render |
|---|---|
| Desktop, dark, 1440 × 900 | [Dashboard](../assets/design-renders/desktop-overview-dark.png) |
| Desktop, light, 1440 × 900 | [Dashboard](../assets/design-renders/desktop-overview-light.png) |
| Mobile, dark, 390 × 844 | [Dashboard](../assets/design-renders/mobile-overview.png) · [Create Link](../assets/design-renders/mobile-create-link.png) |

The PNG renders use the preview's design tokens and sample data. Browser verification is pending. Chromium failed to start in the development sandbox, and the available WebKit build lacked system libraries. The capture script records 13 Chromium screenshots and checks seven viewport sizes for horizontal page overflow. Final design approval is pending. [Capture script](../prototype/capture.cjs) · [Capture instructions](../README.md)
