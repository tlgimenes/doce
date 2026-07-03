# Phase 0 Research: Shared Design System for Interactive Elements

## Decision: Headless primitives library — Radix UI Primitives

**Decision**: Use `@radix-ui/react-*` packages (installed per-primitive,
e.g. `@radix-ui/react-dialog`, `@radix-ui/react-checkbox`) for the
components whose behavior is genuinely hard to get right by hand
(focus trapping, roving tabindex, typeahead, portal/positioning):
Dialog, Select/Combobox base, Checkbox, Radio Group, Tooltip, Dropdown
Menu. `Button` itself is NOT built on a Radix primitive — a button is
just a native `<button>`/`<a>`, and Radix has no Button primitive
because there's nothing non-trivial to delegate; it's a thin styled
wrapper.

**Rationale**:
- Radix ships one unstyled primitive per package, each independently
  installable — matches this feature's incremental scope (add Button
  now, Dialog/Combobox later, without pulling in unrelated components).
- Mature, widely adopted, stable API; used by shadcn/ui and countless
  production apps, which lowers the risk of hitting undocumented edge
  cases while building on React 19.
- Ships correct ARIA roles/states and keyboard behavior out of the box
  (FR-004/FR-005/FR-006), which is the entire reason to adopt a headless
  library instead of hand-rolling `role`/`aria-*`/keydown handlers per
  component.
- Style-agnostic — renders unstyled elements (or via `asChild`, lets us
  render our own styled element), so it composes cleanly with the
  existing Tailwind v4 theme tokens in `src/styles/theme.css` without a
  second styling system.

**Alternatives considered**:
- **Base UI** (`@base-ui-components/react`) — newer library from
  overlapping Radix/MUI authors, explicitly positioned as Radix's
  eventual successor. Rejected for now: still pre-1.0 and iterating on
  its API, smaller community/example base, and its main advantages
  (nested-popup handling, some newer patterns) aren't needed for the
  Button-first scope of this pass. Worth revisiting when Dialog/Combobox
  work starts if Base UI has stabilized further by then.
- **React Aria Components** (Adobe) — excellent accessibility track
  record, but a heavier adoption footprint (its own hook-based styling
  model, larger API surface) than this codebase's currently minimal
  component layer needs.
- **Headless UI** — Tailwind Labs' library, smaller primitive set
  (no Combobox positioning as robust as Radix's, weaker Dialog focus
  management historically). Rejected in favor of Radix's broader,
  more battle-tested set.
- **Hand-rolled** (no headless library) — rejected: this is exactly
  what the feature is trying to move away from; reimplementing focus
  trap / roving tabindex / typeahead per component is the source of the
  inconsistency and accessibility gaps the spec calls out.

## Decision: `cn()` className helper

**Decision**: Add `src/lib/cn.ts` exporting `cn(...inputs) = twMerge(clsx(inputs))`,
using the already-installed `clsx` and `tailwind-merge` dependencies
(both present in `package.json` but currently unused anywhere in the
codebase).

**Rationale**: Every shared component needs to accept a caller-supplied
`className` and merge it with its own variant classes without
Tailwind's later-class-wins ordering issues; `clsx` + `tailwind-merge`
is the standard pairing for this and the dependencies are already
installed, suggesting this was anticipated.

**Alternatives considered**: Plain template-string concatenation —
rejected, breaks when a caller overrides a conflicting utility class
(e.g. caller passes `bg-red-500` but component's own `bg-primary` wins
by source order instead of by intent).

## Decision: Variant styling approach — hand-written variant maps, no `cva` dependency

**Decision**: Define each component's variants (e.g. Button's
`variant`/`size`) as plain TypeScript objects mapping variant name to a
Tailwind class string, composed via `cn()`. Do not add
`class-variance-authority` as a new dependency for this pass.

**Rationale**: The initial component (Button) has a small variant
surface (2–3 variants × 2–3 sizes); a plain object keeps the dependency
count down per the codebase's currently lean `package.json`. If the
variant matrix grows meaningfully once Dialog/Combobox/others are added,
revisit — `cva` composes well with the same `cn()` helper so adopting it
later is not a breaking change to component call sites.

**Alternatives considered**: `class-variance-authority` (`cva`) —
common pairing with Radix + Tailwind (this is the shadcn/ui pattern).
Not rejected outright, just deferred as unnecessary for a single
component; cheap to add later without touching consumer code.

## Decision: Migration scope for this pass

**Decision**: This planning pass covers Button's design and the first
migration slice (User Story 1 + the start of User Story 3). Checkbox,
Radio, and Select are NOT designed here — the codebase audit found zero
existing usages of any of them and no upcoming spec (005, 006, 007)
requires them yet, matching the spec's Assumptions section. Dialog and
Combobox are noted as the concrete next additions (driven by specs 005
and 006) but are out of scope for this plan's task generation; they get
their own `/speckit-plan` pass when that work starts, reusing the same
`src/components/ui/` convention and Radix decision established here.

**Rationale**: Avoids building speculative components ahead of real
demand, per the spec's User Story 3 framing ("audit and migrate what
exists") and FR-011 (future building blocks go into the design system
when they're actually needed, not pre-built).
