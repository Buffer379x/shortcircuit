# Design system: Polar Signal

Polar Signal is the visual system of the ShortCircuit admin interface and its public pages. The interface is near-monochrome and dark first. Pink is a signal: it marks your action and your current place. Every page is built from the same tokens and components. If a component is missing here, read the [UI guidelines](UI_GUIDELINES.md) before adding one. [Branding](BRANDING.md)

| Source | Purpose |
|---|---|
| `frontend/src/styles/tokens.css` | Every color, type, size, radius, layer, and motion token. The only file that may contain color literals. |
| `frontend/src/styles/base.css` | Reset, document type, focus ring, scrollbars, reduced motion. |
| `frontend/src/styles/ui.css` | Every shared component, defined once. |
| `frontend/src/styles/shell.css` | Application frame and shared page layouts. |
| `frontend/src/lib/` | Svelte components and helpers: icons, menus, dialogs, drawer, badges, tags, charts, status mapping, routing, toasts, catalog access. |
| `frontend/tools/lint-design.py` | Fails on color literals outside tokens, on literal font sizes, radii, or z-indexes, and on pages that restyle shared component classes. Runs with `npm run check`, together with `tools/contrast.py` and `tests/i18n_catalog.py`. |

## Color

Semantic tokens only. Components never use raw values. Both themes define the same tokens with their own values. The light theme is a separate palette, not an inversion.

| Role | Token | Dark | Light |
|---|---|---|---|
| Page background | `--bg` | `#101115` | `#F5F5F8` |
| Sidebar | `--bg-sidebar` | `#13151A` | `#EEEEF3` |
| Panel, table | `--surface` | `#1C1E24` | `#FFFFFF` |
| Dialog, menu | `--surface-raised` | `#242731` | `#FFFFFF` |
| Hover | `--surface-hover` | `#292D36` | `#F0EFF5` |
| Divider | `--border` / `--border-subtle` | `#333640` / `#272A32` | `#DADBE3` / `#E7E7EE` |
| Field border | `--border-control` | `#727788` | `#858998` |
| Text | `--text` / `--text-secondary` / `--text-muted` / `--text-disabled` | `#F6F6FA` / `#C3C5CF` / `#969AA9` / `#686C7B` | `#20222A` / `#484C59` / `#666A79` / `#9A9DAA` |
| Brand and primary action | `--accent` / `-hover` / `-active` / `-soft` | `#FF79C6` / `#FF94D2` / `#E963AF` / `#302333` | `#B52B7C` / `#9F226B` / `#8A1D5D` / `#FBEAF4` |
| Text on accent | `--on-accent` | `#101115` | `#FFFFFF` |
| Success / warning / error / info | `--success` … `--info` | `#43D19E` / `#F3BB63` / `#FF6B61` / `#85A9F5` | `#157254` / `#95610F` / `#C2372B` / `#365FC7` |

- **Contrast.** Every text pair meets WCAG 2.2 AA (4.5:1). Field borders and focus meet 3:1. `frontend/tools/contrast.py` checks every pair from `tokens.css` and runs with `npm run check`. Notable pairs:
  - Dark text on pink: 7.91:1.
  - Pink on the dark surface: 6.98:1.
  - Muted text: 5.94:1 dark, 5.38:1 light.
- **Error versus brand.** Error is shifted to hue 4–5°, away from the pink at 325°, and always comes with an icon and text.
- **Use pink for:**
  - The single primary action of a view.
  - The active navigation icon and the selected tab.
  - Focus.
  - Selected filters (as the soft tint).
  - The primary chart series.
  - Progress toward a limit.
  - Links.
- **Do not use pink for:** tags, counts, sparklines, rankings, status, or decoration.
- **Charts.**
  - Series `--chart-1` (pink, primary) to `--chart-6` (slate).
  - The comparison period is `--chart-previous`, dashed.
  - Rankings use slate.
  - Grid and axes use `--chart-grid` and `--chart-axis`.
- **Tags** use a palette without pink.
- **QR codes** stay dark on white in both themes (`--qr-fg`, `--qr-bg`).

## Typography

- **Families.**
  - Geist for interface text.
  - Geist Mono for hosts, slugs, URLs, tokens, and code.
  - Both are variable, local, and licensed under the SIL Open Font License (`frontend/src/fonts/OFL-Geist.txt`), so no external font service is used.
  - Mono is for link data, never for labels.
- **Scale.**

  | Size | Use |
  |---|---|
  | 11 px | Axes and counters |
  | 12 px | Metadata |
  | 13 px | Tables and secondary text |
  | 14 px | Body |
  | 16 px | Section titles |
  | 20 px | Dialog and hero titles |
  | 24 px | Page titles |
  | 30 px | Key figures |

  Line heights are in `tokens.css`.
- **Weights.** 400, 500 (labels and buttons), and 600 (titles, figures, primary buttons).
- **Figures** use tabular numbers (`.num`) and are right-aligned in tables.
- **Case.** Sentence case everywhere. No uppercase labels.

## Space, size, radius, elevation

- **Spacing.** A 4 px grid: `--space-1` (4) to `--space-12` (48). Page sections are 24 px apart, panel padding is 20 px, and fields are 16–20 px apart.
- **Controls.** 28 px (small), 32 px (default), and 40 px (large, only for sign-in and inline shortening). On `pointer: coarse` they grow to 36/44/48 px.
- **Radius by role.**

  | Size | Use |
  |---|---|
  | 2 px | Marks |
  | 4 px | Badges, chips, checkboxes |
  | 6 px | Buttons, inputs, menu items |
  | 8 px | Menus, notices, tooltips |
  | 12 px | Panels, dialogs, drawers |

- **Elevation.** Panels sit on the page through surface steps and a subtle border. Shadows are only for floating layers: `--shadow-popover` and `--shadow-dialog`.
- **Layers.** `--z-sticky` 10 < nav 20 < dropdown 40 < drawer 50 < dialog 60 < toast 70 < tooltip 80.
- **Motion.** 120 ms for hover, 180 ms for menus, dialogs, and toasts, 260 ms for the drawer. `prefers-reduced-motion` turns all of it off.
- **Breakpoints.**

  | Width | Change |
  |---|---|
  | 640 px | Phone: dialogs become bottom sheets, panel heads wrap |
  | 900 px | The sidebar becomes a drawer |
  | 1100 px | Search becomes an icon |
  | 1200 px | Two-column layouts stack |

  Tables use container queries instead of breakpoints.

## Layout

- **Shell.**
  - A 236 px sidebar, collapsible to a 60 px rail. Below 900 px it becomes a modal drawer, and the page behind it is inert.
  - The sidebar groups navigation into the main tasks, Configuration, and System.
  - It shows counts and the instance state.
  - A 56 px top bar holds a breadcrumb on deeper views, search (Ctrl/Cmd+K), the theme menu, and New link (shortcut N).
  - Content is at most 1,480 px wide.
- **Page anatomy.**
  1. A page header with the title, a one-line description, and right-aligned page actions.
  2. Optional tabs.
  3. Panels, each with a 52 px head that holds a title, an optional hint, and the panel's own actions.
- **Grids.**
  - `grid-main-side`: a main column plus a 300–380 px side column.
  - `grid-2`.
  - Key-figure strips are one panel divided by hairlines, not separate cards.
- **Detail pages** use a hero panel (badges, title, short link, destination, actions), then tabs. The tab state is kept in the URL.

## Components

The table lists every shared component in `ui.css` or `lib/`. Pages compose them and add only layout.

| Component | Rules |
|---|---|
| Button `.btn` | Variants: `primary`, `secondary` (default), `outline`, `ghost`, `danger`, `danger-quiet`, `link`. `data-icon-only` for square icon buttons, which need `aria-label` and `use:tip`. `aria-busy` shows the spinner and keeps the width. Disabled uses its own neutral style. One primary per view; destructive actions confirm. |
| Field | `.field` > `.field-label` + control + `.field-hint` or `.field-error`. The error has an icon, is linked through `aria-describedby`, and the control gets `aria-invalid`. |
| Inputs | `.input`, `.select`, `.textarea`. `.input-group` for a fixed host prefix plus a slug. `.search` with an icon and a key hint. Native options follow the theme. |
| Selection | `.checkbox`, `.radio`, `.switch` (applies immediately) inside `.check`. `.choice` cards for options with a description. `.seg` for exclusive modes. `.chip` for filters. |
| Status | `Badge` with the central mapping in `lib/status.ts`: every state has a tone, an icon, and text. Normal states (Active, Reachable) render quietly. `HealthIcon` for dense tables. |
| Tag | `TagChip`, `TagInput` (a combobox with suggestions and create). |
| Short link | `ShortUrl`: the mono host truncates first, and `/slug` stays whole. `CopyButton` confirms in place and with a toast. |
| Panel | `.panel`, `.panel-head` / `.panel-title` / `.panel-body` / `.panel-foot`. `.setting` rows for settings, with `.setting-source` for a value's source badge and reset action. `.savebar` is the sticky unsaved-changes bar with Discard and Save. `.facts` for key and value lists. |
| Table | `.table` in `.table-wrap`. Rows are 52 px, or 40 px compact. Sticky header. `.th-sort` with `aria-sort`. Numbers right-aligned. One opener plus a `Menu` per row, with destructive items last. Container-query tiers hide columns, then turn rows into cards. |
| Tabs | `.tabs` / `.tab` with `role="tab"` and arrow keys, or `aria-current` for route tabs. A fade shows where they overflow. |
| Menu | `Menu`: arrow keys, Home, End, Escape back to the trigger, stays inside the viewport. `menuitemradio` and `menuitemcheckbox` for choices. |
| Dialog | `Dialog`, a native `<dialog>`: the body scrolls while the head and foot stay visible, focus goes to the first field, and it becomes a bottom sheet on phones. `Drawer` for editing next to a list. |
| Preview frame | `.preview-frame`: a full-width `<iframe sandbox="">` with a subtle border that shows a page the server rendered, such as a public page draft. Never add `allow-scripts` or `allow-same-origin`. Give it a `title`. |
| Feedback | Toasts (`toast()`) for results: at most 4, 4.5 s, or 9 s for errors, in polite and alert regions. `.notice` in five tones for lasting conditions. `Empty` with a next action. `.skeleton`, `.spinner`. |
| Charts | `TimeChart` (pointer and arrow-key crosshair, comparison series, a hidden data table), `BarList` (label, bar, value, share), `Donut` (at most 5 parts, with a legend that repeats the numbers), `Columns`, `Sparkline` (slate). |
| Icons | `Icon` with 96 Lucide icons (ISC), 24-unit grid, stroke 1.75, 16 px by default (14 small, 20 large). Each icon has one meaning. Regenerate the set with `frontend/tools/icons.py`. |
| Command palette | Searches links, pages, and actions. A pasted URL offers to shorten it. |

## States and copy

- **Every list** has a loading state, an empty state with an action, a no-results state with "Clear filters", and an error with "Try again".
- **Every action** reports its outcome: a toast for results, an inline error next to the field, and a notice for lasting conditions.
- **Copy.** Functional and specific, in sentence case. A button names its result ("Create link", "Save rules"), and the toast repeats it ("Link created"). Errors say what happened and how to fix it.
- **Strings** live in the central catalog: `frontend/src/i18n.ts` merges `i18n/*.ts`, in English and German. No string is written into a component.

## Accessibility

- A visible 2 px focus ring.
- A skip link.
- Named landmarks.
- Headings receive focus after navigation.
- Icon buttons have names.
- Status never depends on color alone.
- Keyboard operation everywhere: menus, tabs, charts, and the palette.
- 44 px touch targets on coarse pointers.
- Reduced motion is respected.
- axe-core (WCAG 2.2 A/AA) runs in the browser test.

## Public pages

The password, one-time, not-found, expired, and unknown-domain pages use the same mark, colors, and type scale. They use the system font stack, contain no JavaScript, set a strict CSP, and choose English or German from `Accept-Language`. Their status codes and headers are unchanged.

Settings > Public pages can replace their copy per language, set one accent color (the button text switches between dark and white for 4.5:1), and replace the mark with a small raster logo, all inside this layout. Custom HTML is an expert mode that leaves the layout to the administrator. The starter template "Polar Signal" keeps this design with edited copy; "Minimal" is a plain custom design.
