# Implementation Plan: Doce Landing Page

**Branch**: `002-landing-page` | **Date**: 2026-07-03 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/002-landing-page/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

A single-page, static marketing site for Doce, published via GitHub Pages,
that explains the project's value proposition, drives visitors to download
the app, shows the repository's GitHub star count, and offers a "Buy Me a
Coffee" support link. Technical approach: plain HTML/CSS/vanilla JS (no
framework, no build step) living in a new top-level `site/` directory,
deployed by a dedicated GitHub Actions workflow using GitHub's native
"deploy from Actions" Pages flow — kept fully decoupled from the existing
Tauri app's build (`src/`, `src-tauri/`) and its `ci.yml` workflow.

## Technical Context

**Language/Version**: HTML5, CSS3, vanilla JavaScript (ES2020+, no transpilation)

**Primary Dependencies**: None — no framework, no static site generator, no npm build step for the site itself

**Storage**: N/A (static site; no backend, no database)

**Testing**: No automated test framework for the site itself; validated manually via the `quickstart.md` checklist (local static server + acceptance-scenario walkthrough) plus an ad hoc Lighthouse audit for load-time goals

**Target Platform**: Web — modern evergreen desktop and mobile browsers, served as static files via GitHub Pages

**Project Type**: Static web site (single page), independent of the existing Tauri desktop app project in this repo

**Performance Goals**: Primary content (value proposition + calls-to-action) visible within 2s on a typical broadband connection (spec SC-006); Lighthouse performance score ≥ 90 used as an internal implementation proxy for this goal

**Constraints**: No third-party visitor tracking/analytics beyond basic, privacy-respecting counts (FR-010); no accounts, sign-in, or personal data collection (FR-009); star-count fetch failure must never block or blank the page (FR-007); no custom domain, single scrollable page, English-only (per spec Assumptions)

**Scale/Scope**: One HTML page + one stylesheet + one small script; excludes docs/blog/changelog pages, which are out of scope for this feature

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Zero-Config First Run** — N/A. This principle governs the macOS app's
  first-launch experience; the landing page is a separate, external
  marketing surface with no onboarding flow of its own.
- **II. Local-By-Default Privacy** — PASS. FR-009/FR-010 already commit the
  page to no accounts, no personal-data collection, and no third-party
  tracking beyond basic privacy-respecting counts, which mirrors the app's
  own no-telemetry-by-default stance rather than contradicting it.
- **III. Native macOS Polish** — N/A. Applies to the packaged Mac app's
  window chrome/install experience, not a public web page.
- **IV. Extensibility via MCP and Skills** — N/A. The landing page has no
  agent runtime, MCP client, or skills surface.
- **V. v1 Scope Discipline** — PASS. This feature adds a marketing surface
  only; it does not expand the shipped app's platform support, add
  cloud/hosted services, or touch the no-permission-system decision the
  principle governs.

No violations identified; Complexity Tracking section below is left empty.

## Project Structure

### Documentation (this feature)

```text
specs/002-landing-page/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
├── contracts/           # Phase 1 output (/speckit-plan command)
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
site/
├── index.html            # single landing page
├── assets/
│   ├── styles.css
│   └── main.js           # star-count fetch + fallback, small progressive-enhancement only
└── favicon(s)/og-image    # static images referenced by index.html

.github/
└── workflows/
    └── pages.yml          # new workflow: build/deploy site/ to GitHub Pages on push to main (site/** changes) + workflow_dispatch
```

**Structure Decision**: A new top-level `site/` directory holds the static
landing page, fully separate from the existing Tauri app source (`src/`,
`src-tauri/`) and its `vite`/`vitest` toolchain — the site has no
dependency on the app's build. Deployment is a new, independent
`.github/workflows/pages.yml`, alongside the existing `ci.yml`, scoped (via
path filter) to only run when `site/**` changes so it never interferes with
app CI.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

No violations — this section is intentionally empty.

## Post-Phase 1 Constitution Re-check

Re-evaluated after `data-model.md`, `contracts/`, and `quickstart.md` were
drafted: the design introduces no accounts, no telemetry beyond an optional
basic page-view count, and no new external write interfaces — only
read-only fetches of public GitHub data and outbound navigation links. The
Constitution Check verdicts above (PASS on II and V, N/A on I/III/IV) still
hold unchanged.
