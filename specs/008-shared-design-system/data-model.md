# Phase 1 Data Model: Shared Design System for Interactive Elements

This feature has no persisted data; "entities" here are structural
(component API and migration bookkeeping), not database records.

## Shared Interactive Component — Button (this pass)

| Field | Type | Notes |
|---|---|---|
| `variant` | `"primary" \| "secondary" \| "destructive" \| "ghost"` | Visual style. Default `"primary"`. `"destructive"` maps to the existing `--color-destructive` token (already used ad hoc, e.g. delete actions). |
| `size` | `"sm" \| "md"` | Default `"md"`. Matches the two sizes already observed in hand-rolled buttons (`px-3 py-1 text-sm` vs. larger CTA-style buttons). |
| `disabled` | `boolean` | Native `disabled` semantics; drives FR-002/FR-003 (no pointer cursor / no hover style when true). |
| `asChild` | `boolean` (optional) | When true, renders its styling/behavior onto a single child element instead of a `<button>` (Radix `Slot` pattern) — needed for the "button that must be an `<a>`" edge case in the spec. |
| `children` | `ReactNode` | Label/content. |
| `className` | `string` (optional) | Merged via `cn()`, last-wins on conflicting utilities. |
| ...native `button` props | `ComponentPropsWithoutRef<"button">` | `onClick`, `type`, `data-testid`, `aria-*`, etc. pass through untouched so migration doesn't lose existing test hooks (FR-009). |

**States** (FR-002/FR-003/FR-006): default, hover, focus-visible,
active/pressed, disabled. All defined as Tailwind utility classes keyed
off variant, using existing theme tokens (`--color-primary`,
`--color-destructive`, `--color-muted`, `--color-ring` for focus ring).

**Validation rules**: None beyond TypeScript's prop typing — this is a
presentational component, not a data-entry one.

## Migration Site (tracking, not a runtime entity)

For the audit portion of User Story 3, each hand-rolled `<button>`
found in `src/views` is tracked as:

| Field | Value for this pass |
|---|---|
| File + line | Located via the audit in `quickstart.md` |
| Target component | `Button` (`variant`/`size` chosen to match current visual appearance) |
| Existing `data-testid` | Preserved unchanged on the migrated element |
| Status | `migrated` or `exempted` (with a written reason — expected rare per spec Assumptions) |

This tracking lives in the migration's PR description / commit, not as
a persisted artifact — there is no runtime storage for it (Assumptions:
presentational component library, no persistence).
