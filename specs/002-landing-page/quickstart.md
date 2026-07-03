# Quickstart: Doce Landing Page

Validates the `site/` static page end-to-end against the acceptance
scenarios in `spec.md`. No build step is required — see `research.md` § 1.

## Prerequisites

- The `site/index.html`, `site/assets/styles.css`, and `site/assets/main.js`
  files exist (produced by `/speckit-implement`).
- A way to serve static files locally, e.g. Python 3 (`python3 -m http.server`)
  or Node's `npx serve` — either works, no project-specific tooling needed.

## Run it locally

```bash
cd site
python3 -m http.server 8080
# open http://localhost:8080 in a browser
```

## Validation checklist

Walk through each item and confirm the observed behavior matches the spec:

1. **Value proposition (User Story 1 / FR-001, SC-001)** — Load the page.
   Without scrolling, confirm you can read that Doce is a fully local,
   zero-config personal AI agent for macOS, with no API keys and no cloud
   dependency.
2. **Download CTA (User Story 1 / FR-002, FR-006, SC-002)** — Confirm the
   primary download button and the "macOS, Apple Silicon" platform note are
   both visible in the first screen. Click the button and confirm it opens
   the GitHub Releases page (see `contracts/external-interfaces.md` § 2).
3. **Star count — happy path (User Story 2 / FR-003)** — Confirm a numeric
   GitHub star count renders on the page after it loads.
4. **Star count — fallback path (User Story 2 / FR-007, SC-004)** — In
   browser devtools, block or throttle the request to the star-count source
   (e.g. via the Network tab's request-blocking) and reload. Confirm the
   page still renders a complete, unbroken layout showing the static
   fallback count — no error text, no blank gap.
5. **Buy Me a Coffee (User Story 3 / FR-004)** — Confirm the button is
   visible and clicking it opens the support page in a new tab.
6. **No accounts / no tracking (FR-009, FR-010)** — Confirm nothing on the
   page requires sign-in or form submission, and that browser devtools show
   no third-party analytics/tracking requests beyond, at most, a basic
   privacy-respecting page-view count.
7. **Responsive layout (FR-008)** — Resize the browser to a narrow (mobile)
   width and a wide (desktop) width. Confirm no horizontal scrolling and no
   overlapping content at either size.
8. **Two-click reachability (SC-005)** — From page load, confirm the
   download target and the coffee page are each reachable in 2 clicks or
   fewer.
9. **Load speed (SC-006)** — Run a Lighthouse audit (Chrome DevTools →
   Lighthouse, or `npx lighthouse http://localhost:8080`) and confirm the
   primary content is visible well within 2 seconds on a simulated
   broadband connection.

## Validating the deploy workflow

After merging changes under `site/**` to `main`:

1. Confirm `.github/workflows/pages.yml` runs and succeeds in the Actions
   tab.
2. Open the repository's GitHub Pages URL (Settings → Pages) and repeat the
   checklist above against the live deployment.
