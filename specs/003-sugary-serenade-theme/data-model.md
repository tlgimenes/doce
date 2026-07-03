# Data Model: Sugary Serenade Color Theme

This feature has no application data — the "model" here is the set of
design tokens (CSS custom properties) the theme introduces, and how they
map onto the page's existing elements.

## Palette tokens (as given)

| Token | Hex | Role |
|-------|-----|------|
| `--color-1` | `#f9b4a9` | peach/coral — primary (Download) button |
| `--color-2` | `#f6d9b6` | light apricot — GitHub star badge |
| `--color-3` | `#fce4a6` | pale yellow — Buy Me a Coffee button |
| `--color-4` | `#fae3b2` | cream — page base background |
| `--color-5` | `#f4c1a4` | soft salmon — gradient stop only |

## Gradient tokens (as given)

| Token | Definition | Usage |
|-------|-----------|-------|
| `--gradient-linear` | `linear-gradient(0.25turn, var(--color-1), var(--color-2), var(--color-3), var(--color-4), var(--color-5))` | Hero section background (FR-002) |
| `--gradient-radial` | `radial-gradient(circle, var(--color-1), var(--color-2), var(--color-3), var(--color-4), var(--color-5))` | Button hover states |

## Supporting neutral tokens (new — not in the original 5, required for contrast)

| Token | Hex | Role | Worst-case contrast vs. palette |
|-------|-----|------|-----|
| `--color-text` | `#3a1f1c` | headings, primary body text, button/badge text, star icon fill | 8.71:1 (vs `--color-1`) |
| `--color-text-muted` | `#5c3a30` | tagline, platform-requirement note, star-count label | 5.77:1 (vs `--color-1`) |

Both exceed the WCAG 2.1 AA minimum (4.5:1 normal text) — see
`research.md` § 1 for the full per-color breakdown and the gradient-path
verification.

## Element → token mapping

| Existing element (from `002-landing-page`) | Background token | Text/icon token |
|---|---|---|
| `body` (page canvas) | `--color-4` | `--color-text` |
| `#hero` section | `--gradient-linear` | `--color-text` (heading), `--color-text-muted` (tagline) |
| `.button-primary` (Download) | `--color-1`; hover → `--gradient-radial` | `--color-text` |
| `.platform-note` | (inherits hero background) | `--color-text-muted` |
| `.star-badge` | `--color-2`; hover → `--gradient-radial` | `--color-text` |
| `.star-icon` (SVG fill) | n/a | `--color-text` |
| `.button-secondary` (Buy Me a Coffee) | `--color-3`; hover → `--gradient-radial` | `--color-text` |

No entity has a lifecycle or persistence — this table is a one-time,
static mapping applied at build/author time in `site/assets/styles.css`.
