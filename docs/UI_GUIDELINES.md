# UI guidelines

These rules keep new features consistent with Polar Signal. Read the [design system](DESIGN_SYSTEM.md) first.

## Before you build

1. **Look for an existing pattern.** Find a page that already solves a similar problem (list, detail, settings, wizard) and reuse its structure.
2. **Use the shared components** in `frontend/src/lib/` and the classes in `ui.css`. Add a component only for a functional need the library does not cover. Add it to `ui.css` or `lib/ui/`, never inside a page, and document it in the design system.
3. **Use tokens only.** No new colors, font sizes, radii, shadows, or z-indexes. If a value seems missing, use the nearest token. If it is truly missing, add a semantic token to `tokens.css` for both themes and check its contrast.
4. **Write the copy into the catalog.** Add every string to the area file in `frontend/src/i18n/`, in English and German. Keys are unique across all files.

## Page rules

- **Header.** One page title (h1) and one line of description. Page actions sit right-aligned in the header.
- **One primary action per view.** It is the action most people come for. Everything else is secondary, ghost, or in a menu.
- **Panels.** Group related content into panels with a title. A panel's actions sit in its head.
- **Tables.** Each row has one opener plus at most two quick actions on hover. Everything else goes in the row menu, with destructive items last.
- **Destructive actions** use `danger` or `danger-quiet`, name the object, and ask for confirmation. Irreversible bulk actions or data deletion ask for typed confirmation.
- **Forms.**
  - Labels sit above fields, with a hint below.
  - Errors appear next to the field and receive focus.
  - Optional fields say so.
  - Long forms use collapsed sections that show a summary.
  - Settings that apply immediately use a switch. Others use an explicit save with an unsaved-changes bar.
- **State in the URL.** Filters, tabs, and sorting live in the hash route, so reload and Back work.
- **Feedback.**
  - A toast for the result of an action.
  - An inline error for invalid input.
  - A notice for lasting conditions.
  - Never feedback that can be off-screen.
- **Mobile.** Check every view at 390 and 320 px: no horizontal scrolling, primary actions reachable, tables as cards, dialogs as sheets.
- **Data.** Never show numbers or settings the API does not provide. Label estimates and UTC days.

## Review checklist

- [ ] `npm run check` passes: svelte-check plus the design lint.
- [ ] Both themes at 1440, 1024, 768, 390, and 320 px, without overflow or truncated labels.
- [ ] Keyboard only: every control is reachable, focus is visible, and dialogs return focus.
- [ ] Every icon-only button has a name and a tooltip. Status uses text and an icon.
- [ ] Pink is used only for the primary action, the selection, focus, and the primary data series.
- [ ] German text fits at 320 px.
- [ ] No static `style="…"` attributes: the admin CSP blocks inline styles. Use classes, or `style:prop={value}` for dynamic values.
