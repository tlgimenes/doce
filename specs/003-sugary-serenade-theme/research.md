# Phase 0 Research: Sugary Serenade Color Theme

No `[NEEDS CLARIFICATION]` markers were carried into this phase — the
palette, gradients, and scope (full replacement, no toggle) were all
specified directly by the user. The open question this phase had to resolve
was purely technical: **which text color(s) pair with a 5-color palette
that is uniformly light, while meeting WCAG AA everywhere the palette is
used (including across the gradients)?**

## 1. Text color selection

- **Decision**: A single warm dark-brown "espresso" tone, `#3a1f1c`, is used
  for all body/heading text; a lighter warm-brown "muted" tone, `#5c3a30`,
  is used for de-emphasized text (tagline, platform note, star-badge label).
- **Rationale**: All five Sugary Serenade colors are light pastels with no
  dark tone among them (relative luminance is high and clustered), so no
  color *from* the palette itself can serve as body text at AA contrast —
  a supporting neutral is unavoidable (already anticipated in the spec's
  Assumptions). Rather than guessing a shade, contrast ratios were computed
  directly via the WCAG relative-luminance formula against all 5 stops:

  | Text candidate | vs color-1 | vs color-2 | vs color-3 | vs color-4 | vs color-5 | Worst case |
  |---|---|---|---|---|---|---|
  | `#3a1f1c` (chosen, primary) | 8.71:1 | 11.14:1 | 12.05:1 | 11.99:1 | 9.34:1 | **8.71:1** |
  | `#5c3a30` (chosen, muted) | 5.77:1 (worst among all 5) | — | — | — | — | **5.77:1** |

  Both comfortably clear the 4.5:1 AA threshold for normal text (the primary
  tone clears 7:1, AAA). The primary tone was also checked against 200
  interpolated points along the actual linear-gradient path (not just the 5
  named stops, since text can sit on any point along it) — the worst
  contrast found along the whole gradient was still 8.71:1, at the
  peach/coral (`color-1`) end, meaning the gradient introduces no dip below
  what the stops alone already guarantee.
- **Alternatives considered**: Pure black `#000000` — technically passes
  contrast but reads as harsh/cold against a "sugary" pastel palette, which
  undercuts the intended warm, soft tone. A mid-gray neutral — rejected
  because grays desaturate the warm palette and read as a mismatched,
  generic UI gray rather than a color chosen to belong with the theme.

## 2. Base page background vs. gradient placement

- **Decision**: `--color-4` (cream, `#fae3b2` — the lightest, most neutral
  of the five) is the page's base background. The hero section uses the
  full linear gradient (`0.25turn` through all 5 colors) as its background,
  satisfying FR-002. Button hover states use the radial gradient (`circle`,
  same 5 colors) as a "pop" affordance, directly matching the user's own
  suggested placement ("hero background or button hover states").
- **Rationale**: Using one of the five defined colors as the page canvas
  keeps FR-001 ("no element outside the palette") literally true even for
  the page's base background, rather than introducing a sixth off-white
  neutral. Reserving the full 5-stop gradient for the hero (the first thing
  a visitor sees) makes it the single most visually prominent "Sugary
  Serenade" moment on the page, rather than diluting the effect by using it
  everywhere.
- **Alternatives considered**: Applying the linear gradient as the *entire
  page* background — rejected, since a moving 5-color gradient behind every
  section (including body copy) would fight with the muted/primary text
  contrast guarantees computed above (those were verified against the 5
  named stops and their direct interpolation, not against a gradient that's
  also been given `background-attachment`/section-relative sizing
  complexity); confining it to one section keeps the contrast math exact
  and simple to verify.

## 3. Button and badge color mapping

- **Decision**: Primary "Download" button uses `--color-1` (peach/coral) as
  its default background; the GitHub star badge uses `--color-2` (light
  apricot); the "Buy Me a Coffee" button uses `--color-3` (pale yellow, the
  closest in-palette echo of the previous brand-yellow BMC button). All
  three use the espresso text color and swap to the radial gradient on
  hover.
- **Rationale**: Spreads the 5-color palette across the page's interactive
  elements (satisfying FR-003) rather than reusing one color for every
  button, which would look flat. Assigning pale yellow to the BMC button
  specifically preserves a visual echo of Buy Me a Coffee's own
  well-known brand yellow without reusing its exact off-palette hex value
  (which FR-001 would otherwise rule out).
- **Alternatives considered**: Keeping BMC's official yellow (`#ffdd00`)
  as a deliberate exception (third-party brand color) — rejected, since
  FR-003 explicitly requires every interactive element to use Sugary
  Serenade colors, and `--color-3` already reads as "yellow" in this
  palette's context.

## 4. Star icon color

- **Decision**: The star icon (an inline SVG) is filled with the espresso
  text color rather than kept as the previous amber/gold (`#facc15`).
- **Rationale**: Keeps the badge internally consistent (icon + text one
  color) and stays inside the palette-plus-neutral system established
  above; the star shape itself (not its color) is what visitors recognize,
  satisfying the spec's edge case about the icon remaining "recognizable."
- **Alternatives considered**: Keeping a separate gold tone for the star —
  rejected as an unnecessary sixth color outside the defined system, adding
  a maintenance/consistency burden for a single small glyph.

## 5. Favicon / OG image regeneration

- **Decision**: Regenerate the same Python/Pillow script used in
  `002-landing-page` with the new palette: cream (`--color-4`) background,
  espresso text/icon color, peach (`--color-1`) accent circle.
- **Rationale**: Reuses the exact generation approach already established
  for these assets (no new tooling), satisfying FR-007 with minimal added
  complexity.
- **Alternatives considered**: Hand-designing new artwork — rejected as
  disproportionate effort for a placeholder brand asset at this stage.
