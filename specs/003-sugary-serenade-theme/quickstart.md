# Quickstart: Sugary Serenade Color Theme

Validates the restyled `site/` against `spec.md`'s acceptance scenarios.
This is a superset of `specs/002-landing-page/quickstart.md` — every check
there must still pass unchanged, plus the new theme-specific checks below.

## Prerequisites

- `site/assets/styles.css` has been updated per `data-model.md`'s token
  mapping (produced by `/speckit-implement`).
- `site/assets/favicon.png`, `favicon-32.png`, and `og-image.png` have been
  regenerated in the new palette.
- A way to serve static files locally (see `specs/002-landing-page/quickstart.md`
  for the exact command).

## Validation checklist

1. **Non-regression (User Story 2)** — Re-run every item in
   `specs/002-landing-page/quickstart.md` unchanged: hero copy, download
   link target, star-count fetch + fallback behavior, Buy Me a Coffee link
   target, responsive layout, no third-party tracking. All must still pass
   exactly as before — only appearance may differ.
2. **Palette applied (User Story 1 / FR-001)** — Load the page and confirm
   no element still shows the previous dark navy (`#0d1117`) background or
   sky-blue (`#38bdf8`) button — every background, button, and accent
   should read as a Sugary Serenade tone.
3. **Gradient present (FR-002)** — Confirm the hero section visibly renders
   the linear gradient sweeping through all 5 colors, and that at least one
   button visibly shows the radial gradient on hover.
4. **Contrast (FR-004, SC-002)** — Using a browser accessibility inspector
   or a Lighthouse accessibility audit, confirm no contrast-related
   failures are reported anywhere on the page.
5. **Copy unchanged (User Story 2 / FR-005)** — Diff the visible text
   against `specs/002-landing-page/spec.md`'s wording — headline, tagline,
   platform note, star-count label, coffee-button label must be identical.
6. **Brand surfaces (User Story 3 / FR-007, SC-004)** — View the favicon in
   the browser tab and open `site/assets/og-image.png` directly; both
   should visually read as Sugary Serenade (warm/cream/peach tones), not
   the previous dark navy icon.
7. **No theme toggle (FR-008)** — Confirm there is no user-facing control
   to switch themes, and that the page looks the same regardless of the
   OS-level light/dark preference (toggle it and reload if convenient).
8. **Accessibility regression check (SC-005)** — Run a Lighthouse
   accessibility audit and confirm the score is at or above the
   `002-landing-page` baseline (100, per that feature's implementation
   notes) — no new failures introduced by the color change.

## Validating the deployed site

After merging to `main`, confirm `.github/workflows/pages.yml` (unchanged
by this feature) redeploys automatically, then repeat steps 2-6 above
against the live GitHub Pages URL.
