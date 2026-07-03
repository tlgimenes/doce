# Data Model: Doce Landing Page

This feature has no persistent storage or database — it is a static page.
The "data" involved is limited to two small pieces of client-side state
sourced from external, read-only data.

## Repository Star Count

Represents the number of GitHub stars shown to the visitor (User Story 2 /
FR-003, FR-007).

| Field | Type | Notes |
|-------|------|-------|
| `count` | integer | Current star count for the Doce repository |
| `fetchedAt` | timestamp | When `count` was last successfully retrieved |
| `isFallback` | boolean | `true` when `count` is the baked-in static fallback rather than a live fetch |

**Validation rules**:
- `count` MUST be a non-negative integer.
- If a live fetch fails or the cached value is older than the refresh
  window (~1 hour client-side cache; ≤24h per FR-003), the page falls back
  to the baked-in static value and sets `isFallback = true` — it MUST NOT
  render an empty, loading-forever, or error state (FR-007).

**State transitions**: `loading → live` (fetch succeeds) or
`loading → fallback` (fetch fails/rate-limited/times out). No further
transitions occur within a single page view.

## Download Target

Represents where the primary call-to-action button sends the visitor
(User Story 1 / FR-002, FR-006).

| Field | Type | Notes |
|-------|------|-------|
| `url` | string (URL) | Target of the download button — the repository's GitHub Releases page/latest asset |
| `platformRequirement` | string | Human-readable platform requirement text shown near the button (e.g. "macOS, Apple Silicon") |

**Validation rules**:
- `url` MUST resolve even before a release exists (points at the Releases
  index page, which degrades gracefully to "no releases yet" rather than a
  404 — see spec Edge Cases).
- `platformRequirement` MUST be visible in the same viewport as the download
  button (FR-006, SC-002).

Both entities are read-only from the page's perspective — nothing on this
static site writes back to GitHub or any other external system.
