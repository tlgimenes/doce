# Phase 0 Research: Doce Landing Page

All Technical Context items had a clear default given the feature's scope
(one static marketing page); no `[NEEDS CLARIFICATION]` markers were carried
into this phase. This document records the decisions and why each
alternative was rejected.

## 1. Site stack: plain HTML/CSS/JS vs. a static site generator

- **Decision**: Plain HTML/CSS/vanilla JavaScript, no build step, no
  framework.
- **Rationale**: The page is a single, mostly-static screen. A generator
  (Jekyll, 11ty, Astro) earns its keep when there's templating/content reuse
  across many pages; here it would only add a toolchain and a build step to
  maintain for zero real benefit, contradicting the explicit "keep the stack
  minimal" direction.
- **Alternatives considered**: Jekyll (GitHub Pages' native SSG) — rejected,
  adds a Ruby toolchain for one page. 11ty/Astro — rejected, adds an
  npm-based build pipeline for content that doesn't need templating.

## 2. Hosting & deploy mechanism

- **Decision**: A dedicated `.github/workflows/pages.yml` using GitHub's
  native "deploy from Actions" flow (`actions/upload-pages-artifact` +
  `actions/deploy-pages`), triggered on push to `main` for `site/**` changes
  plus manual `workflow_dispatch`.
- **Rationale**: No synthetic `gh-pages` branch to maintain; matches the
  repo's existing Actions-based CI convention (`ci.yml`); official,
  actively-supported GitHub mechanism with a minimal permissions footprint
  (`pages: write`, `id-token: write`).
- **Alternatives considered**: `peaceiris/actions-gh-pages` pushing a
  `gh-pages` branch — rejected as the older pattern GitHub is moving away
  from. Classic Pages mode serving a `docs/` folder on `main` — rejected,
  couples generated web content directly into app source history with no
  build/gate step.

## 3. GitHub star count

- **Decision**: Fetch the count client-side, in `main.js`, against GitHub's
  public unauthenticated repository endpoint, with a short client-side cache
  (e.g. `localStorage`, ~1 hour) and a static fallback figure baked into the
  HTML for when the fetch fails or is rate-limited.
- **Rationale**: Keeps the site fully static — no server or scheduled job to
  maintain — while meeting the ≤24h freshness requirement (FR-003) and the
  graceful-degradation requirement (FR-007). The unauthenticated rate limit
  (60 requests/hour) is per visitor IP, not per site, so normal traffic won't
  trip it; the baked-in fallback covers the rare case cleanly.
- **Alternatives considered**: A shields.io stars badge `<img>` as the
  *primary* mechanism — rejected because an `<img>` can't drive custom
  fallback/loading behavior as cleanly, though it remains a reasonable
  fallback *image* option during implementation. A scheduled Action that
  commits a refreshed static number on a cron — rejected as unnecessary
  commit-back complexity for a number that already tolerates 24h staleness.

## 4. "Buy Me a Coffee" integration

- **Decision**: A plain styled `<a>` link/button to the project's Buy Me a
  Coffee page, opening in a new tab — no embedded widget script.
- **Rationale**: Keeps FR-010 (no third-party tracking) intact; the official
  BMC embeddable widget loads a third-party script/iframe on every page
  view for a P3, nice-to-have feature, which isn't worth the trade-off.
- **Alternatives considered**: Official BMC button/widget embed — rejected
  for the third-party script it introduces.

## 5. Testing approach

- **Decision**: No automated test framework for the site; validation is
  manual, via `quickstart.md`'s local-server + acceptance-scenario
  checklist, plus an ad hoc Lighthouse run for the load-time goal.
- **Rationale**: The page's only non-trivial behavior is the star-count
  fetch-with-fallback; standing up a browser test harness (the repo already
  has WebdriverIO, but scoped to the Tauri app) for that single behavior is
  disproportionate to the risk, especially since the fallback is designed to
  be visibly correct rather than silently wrong.
- **Alternatives considered**: Playwright/WebdriverIO e2e coverage —
  rejected as overkill for a static marketing page at this scope.
