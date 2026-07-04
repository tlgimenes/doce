# External Interface Contracts: doce Landing Page

This site exposes no API of its own — it's a static page. It does depend on
three external, read-only interfaces. This document is the contract each
must satisfy for the page to behave per the spec.

## 1. GitHub star count source

- **Direction**: Outbound, client-side, read-only.
- **Expectation**: Given the doce repository's owner/name, the source
  returns a current star count for that repository without requiring the
  visitor to authenticate.
- **Shape required by the page**: A numeric star count value, resolvable in
  a single round trip, that the page's JS can read and render as an
  integer (e.g. `1234` → `"1,234"` on screen).
- **Failure contract**: Any non-success response, network error, or
  timeout MUST be treated identically — fall back to the baked-in static
  count (`data-model.md` § Repository Star Count) with `isFallback = true`.
  The page MUST NOT surface the raw error to the visitor.
- **Freshness contract**: A value up to ~1 hour old (client cache) is
  acceptable; the spec's freshness bound is 24 hours (FR-003, SC-003).

## 2. Download target (GitHub Releases)

- **Direction**: Outbound navigation (visitor click), not fetched by the
  page's JS.
- **Expectation**: The primary download button's `href` points at the
  repository's Releases index (or latest-release redirect) so the link is
  always valid, even before any release has been published.
- **Failure contract**: If no release exists yet, the Releases page itself
  communicates "no releases yet" — the landing page does not need to detect
  or special-case this; the link must simply never 404.

## 3. Buy Me a Coffee page

- **Direction**: Outbound navigation (visitor click), opened in a new tab.
- **Expectation**: A stable URL to the project's Buy Me a Coffee (or
  equivalent) contribution page, supplied during implementation.
- **Constraint**: Reached via a plain link only — no embedded third-party
  script/widget is loaded on the landing page itself (see `research.md` § 4,
  and spec FR-010).
