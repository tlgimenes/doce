# Implementation Plan: Sugary Serenade Color Theme

**Branch**: `003-sugary-serenade-theme` | **Date**: 2026-07-03 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/003-sugary-serenade-theme/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

A pure visual restyle of the existing `002-landing-page` static site: replace
the current dark navy/sky-blue theme with the "Sugary Serenade" pastel
palette (5 defined colors + a linear and a radial gradient) across
backgrounds, buttons, and accents, while keeping every line of copy, every
link target, and the star-fetch/fallback JS behavior identical. Technical
approach: introduce the palette as CSS custom properties in the existing
`site/assets/styles.css`, pair it with one fixed dark neutral text color
chosen for WCAG AA contrast against all five tones (verified by direct
luminance calculation, not guesswork), apply the linear gradient to the hero
section and the radial gradient to button hover states, and regenerate the
favicon/OG image assets in the new palette. No HTML or JS changes.

## Technical Context

**Language/Version**: CSS3 (custom properties); no new HTML or JavaScript — `site/index.html` and `site/assets/main.js` from `002-landing-page` are unchanged

**Primary Dependencies**: None — same as `002-landing-page` (no framework, no build step); Python 3 + Pillow used one-off, offline, to regenerate the favicon/OG PNGs (not a runtime dependency of the site)

**Storage**: N/A (static site; no backend, no database)

**Testing**: No automated test framework (continuing `002-landing-page`'s decision); contrast compliance is verified analytically via WCAG relative-luminance calculation against all 5 palette colors (and the full gradient interpolation) before implementation, then spot-checked visually plus with a Lighthouse accessibility pass during `quickstart.md` validation

**Target Platform**: Web — same static GitHub Pages deployment as `002-landing-page`; no changes to `.github/workflows/pages.yml`

**Project Type**: Static web site restyle (single project), modifying `002-landing-page`'s `site/` in place

**Performance Goals**: No regression to `002-landing-page`'s load-time goal (primary content visible within 2s on broadband) — this feature changes CSS property values and swaps two small PNGs, adding no new requests or render-blocking resources

**Constraints**: Every text/background pairing MUST meet WCAG 2.1 AA contrast (≥4.5:1 normal text, ≥3:1 large text/headings) — this is the binding constraint on which text color(s) can pair with the (uniformly light) palette; no changes to DOM structure, copy, or JS logic, which keeps this feature's blast radius to CSS + 2 regenerated image assets

**Scale/Scope**: One CSS file modified in place (`site/assets/styles.css`) plus 3 regenerated image assets (`favicon.png`, `favicon-32.png`, `og-image.png`); no new pages, sections, or files

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Zero-Config First Run** — N/A. Governs the macOS app's first-launch
  experience, not the public marketing page's color scheme.
- **II. Local-By-Default Privacy** — PASS (unchanged from `002-landing-page`).
  This feature touches only CSS values and static image assets; it adds no
  network calls, accounts, or tracking of any kind.
- **III. Native macOS Polish** — N/A. Applies to the packaged Mac app, not a
  public web page's palette.
- **IV. Extensibility via MCP and Skills** — N/A. No agent runtime, MCP
  client, or skills surface involved.
- **V. v1 Scope Discipline** — PASS. A color restyle of the existing
  marketing page does not expand the shipped app's platform support, add
  services, or touch the no-permission-system decision the principle
  governs.

No violations identified; Complexity Tracking section below is left empty.

## Project Structure

### Documentation (this feature)

```text
specs/003-sugary-serenade-theme/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

No `contracts/` directory — this feature adds no new external interface;
the three read-only interfaces documented in `002-landing-page`'s
`contracts/external-interfaces.md` (GitHub stars, download link, Buy Me a
Coffee link) are untouched by a color-only restyle.

### Source Code (repository root)

```text
site/                    # existing, from 002-landing-page — modified in place
├── index.html           # UNCHANGED by this feature
└── assets/
    ├── styles.css       # MODIFIED: Sugary Serenade custom properties + remapped rules
    ├── main.js           # UNCHANGED by this feature
    ├── favicon.png        # REGENERATED to match the new palette
    ├── favicon-32.png      # REGENERATED to match the new palette
    └── og-image.png        # REGENERATED to match the new palette
```

**Structure Decision**: No new top-level structure. This feature edits
`002-landing-page`'s existing `site/assets/styles.css` in place and
regenerates its 3 image assets; `site/index.html`, `site/assets/main.js`,
and `.github/workflows/pages.yml` are untouched, since the feature is
scoped to visual styling only (FR-005, FR-006).

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

No violations — this section is intentionally empty.

## Post-Phase 1 Constitution Re-check

Re-evaluated after `research.md`, `data-model.md`, and `quickstart.md` were
drafted: the chosen design (fixed dark-neutral text color validated against
all 5 palette tones by direct contrast calculation, CSS-only change, no new
network calls or assets beyond 3 regenerated PNGs) introduces nothing that
touches privacy, telemetry, or platform scope. The Constitution Check
verdicts above (PASS on II and V, N/A on I/III/IV) still hold unchanged.
