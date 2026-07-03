# Feature Specification: Shared Design System for Interactive Elements

**Feature Branch**: `008-shared-design-system`

**Created**: 2026-07-03

**Status**: Draft

**Input**: User description: "Shared design system: introduce base UI primitives (Button, Checkbox, Radio, Select, Link/clickable) that the app's views use instead of hand-rolled markup, so interactive states like cursor-pointer, hover, focus, and disabled styling are consistent and accessible everywhere. Components should be built on accessible base primitives (e.g. Radix/Base UI style unstyled primitives) rather than raw HTML, so keyboard navigation, ARIA roles/attributes, and focus management come for free. Scope includes auditing existing hand-rolled buttons/checkboxes/clickable elements across src/views and src/components and migrating them to the shared components."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Every clickable element looks and feels clickable (Priority: P1)

A user moves their mouse over any interactive control in the app — a
button, a checkbox, a menu item, a link — and immediately sees it react
(pointer cursor, hover/active/focus styling). Disabled controls never
appear clickable. This behavior is the same everywhere in the app, not
just in some views.

**Why this priority**: This is the concrete problem that surfaced the
feature (buttons/checkboxes not showing a pointer cursor) and is the
smallest slice that delivers visible value on its own.

**Independent Test**: Can be fully tested by hovering over every
interactive control across each view (chat, settings, workspace,
onboarding) and confirming a pointer cursor and consistent hover/focus
styling appear on enabled controls, and that disabled controls show
neither.

**Acceptance Scenarios**:

1. **Given** any enabled button, checkbox, radio, select, or clickable
   link in the app, **When** a user hovers over it with a mouse, **Then**
   the cursor changes to a pointer and a hover style is visibly applied.
2. **Given** a disabled control, **When** a user hovers over it, **Then**
   the cursor remains the default arrow and no hover/clickable styling is
   applied.
3. **Given** two different views that each contain a button (e.g. Chat and
   Settings), **When** compared side by side, **Then** the buttons share
   the same visual language (shape, spacing, states) rather than looking
   like independently styled one-offs.

---

### User Story 2 - Every control is usable without a mouse (Priority: P2)

A user navigates the app entirely with the keyboard (Tab/Shift+Tab,
Enter/Space, arrow keys) or with a screen reader. Every interactive
control can be reached, its purpose and state are announced correctly,
and it can be activated the same way native macOS/browser controls work.

**Why this priority**: Accessibility was explicitly called out as a goal
("base UI constructs for accessibility") and is the reason to build on
shared primitives rather than just adding a CSS rule — but it depends on
User Story 1's component set existing first.

**Independent Test**: Can be fully tested by unplugging the mouse,
tabbing through a view end-to-end, and confirming every control receives
a visible focus indicator, is operable via keyboard, and (via a screen
reader or accessibility inspector) exposes a correct role, name, and
state (e.g. checked, disabled, expanded).

**Acceptance Scenarios**:

1. **Given** a view containing buttons, checkboxes, and a select,
   **When** a user tabs through it, **Then** focus visits every
   interactive control in a logical order and each shows a visible focus
   indicator.
2. **Given** a focused checkbox or radio, **When** the user presses
   Space (checkbox) or an arrow key (radio group), **Then** its checked
   state changes exactly as it would with a mouse click.
3. **Given** a screen reader or accessibility inspector, **When** it
   reads a shared control, **Then** it announces the correct role (e.g.
   "button", "checkbox"), accessible name, and current state.

---

### User Story 3 - The whole app is migrated, not just new code (Priority: P3)

A developer auditing the codebase finds that existing hand-rolled
buttons, checkboxes, and clickable elements across the app's views have
been replaced with the shared components, so the consistent,
accessible behavior from User Story 1 and 2 applies retroactively
everywhere, not only to newly written screens.

**Why this priority**: Without migrating existing call sites, the shared
components would only prevent the problem going forward while leaving
today's inconsistency in place — this closes that gap, but it's ordered
last because it depends on the component set (P1) and its accessibility
behavior (P2) being finalized first, and existing automated tests must
keep passing through the migration.

**Independent Test**: Can be fully tested by grepping the codebase for
raw interactive HTML elements outside the shared component
implementations themselves, confirming none remain in `src/views` or
`src/components`, and by running the existing automated test suites to
confirm no regressions.

**Acceptance Scenarios**:

1. **Given** the current codebase's hand-rolled interactive elements
   (e.g. in Chat, Settings, Workspace, onboarding views), **When** the
   migration is complete, **Then** each has been replaced with the
   corresponding shared component.
2. **Given** the app's existing automated test suites (unit and
   end-to-end), **When** they are run after migration, **Then** all
   previously passing tests continue to pass, including tests that
   locate elements via `data-testid`.

---

### Edge Cases

- What happens when a control is both `disabled` and mid-async-action
  (e.g. a submit button while a request is in flight)? It must still
  read as non-clickable (no pointer cursor, no hover style) and
  communicate its busy state to assistive technology.
- How does the system handle a clickable element nested inside another
  clickable element (e.g. a delete button inside a clickable list row)?
  Only the innermost control should be reachable/activatable at that
  point; the outer element's own clickable affordance must not bleed
  into the inner control's hit area in a way that breaks either one.
- How does a shared component behave when it must render as a different
  underlying element for layout reasons (e.g. a "button" that must be an
  `<a>` for right-click/open-in-new-tab support)? It must still expose
  button semantics and behavior to assistive technology.
- What happens to a control's shared styling in the app's dark theme vs.
  light theme? States (hover/focus/disabled/checked) must remain
  visually distinguishable in both.
- What happens when a migrated element previously exposed a
  `data-testid` or other selector relied on by an existing automated
  test? The migrated element must preserve that hook so the test keeps
  passing.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The app MUST provide a shared, reusable set of interactive
  components — at minimum Button, Checkbox, Radio, Select, and a
  clickable Link/Row-style element — available for use across all views.
- **FR-002**: Every enabled interactive control MUST display a pointer
  cursor and a visible hover style when the mouse is over it.
- **FR-003**: Every disabled interactive control MUST NOT display a
  pointer cursor or any hover/active styling.
- **FR-004**: Shared components MUST be fully operable via keyboard alone
  (reachable via Tab, activatable via Enter/Space, and, for grouped
  controls like radios, navigable via arrow keys per platform
  convention).
- **FR-005**: Shared components MUST expose correct semantic role,
  accessible name, and state (e.g. checked, disabled, pressed, expanded)
  to assistive technology without requiring each usage site to hand-wire
  ARIA attributes itself.
- **FR-006**: Shared components MUST display a visible focus indicator
  when reached via keyboard navigation.
- **FR-007**: Shared components MUST render correctly in both the app's
  light and dark themes, with all interaction states remaining visually
  distinguishable in each.
- **FR-008**: Existing hand-rolled interactive elements in `src/views`
  and `src/components` MUST be audited and migrated to the shared
  components, except where a documented reason justifies leaving a
  specific element unmigrated.
- **FR-009**: Migration of existing elements MUST preserve test hooks
  (e.g. `data-testid` attributes) already relied upon by the app's
  automated test suites.
- **FR-010**: Migration MUST NOT change the existing behavior or visible
  content of a migrated screen beyond the interactive-state styling and
  accessibility improvements described above (no incidental redesign).
- **FR-011**: This is a standing convention, not a one-time cleanup: any
  future building-block UI element (a new interactive control type, or a
  new variant/state of an existing one) MUST be added to the shared
  design system and consumed from there, rather than hand-rolled inline
  in a view. A view-specific one-off is permitted only when it has no
  reusable interactive behavior (e.g. static, non-interactive
  presentation), not merely because building the shared version would
  take longer.

### Key Entities

- **Shared Interactive Component**: A reusable UI primitive (Button,
  Checkbox, Radio, Select, Clickable/Link) with a defined set of visual
  variants (e.g. primary/secondary, size) and interaction states
  (default, hover, focus, active, disabled, and checked/indeterminate
  where applicable).
- **Migration Site**: An existing hand-rolled interactive element found
  during the audit (file + location), mapped to the shared component
  that should replace it, with a status (migrated / exempted with
  reason).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of interactive controls in the app show a pointer
  cursor and hover styling when enabled, and show neither when disabled,
  verified across every view.
- **SC-002**: 100% of interactive controls in the app can be reached and
  activated using only the keyboard, with a visible focus indicator at
  every stop.
- **SC-003**: A codebase audit finds zero raw hand-rolled
  buttons/checkboxes/radios/selects in `src/views` or `src/components`
  outside the shared components' own implementation, excluding any
  explicitly documented exceptions.
- **SC-004**: 100% of previously passing automated tests (unit and
  end-to-end) continue to pass after migration, with zero test changes
  required solely to relocate a `data-testid`.

## Assumptions

- The app's existing Tailwind-based styling system and theme tokens
  (`src/styles/theme.css`) remain the styling foundation; the shared
  components are styled with that system rather than introducing a
  second styling approach.
- "Accessible base primitives" means adopting an existing unstyled/
  headless component library (final library choice is a planning-phase
  decision) rather than hand-building keyboard/ARIA behavior from
  scratch for each control type.
- No new visual design language (colors, spacing scale, typography) is
  required beyond what already exists; this feature standardizes
  interaction behavior and component structure, not the visual theme.
- Doce is a macOS desktop app (per project constitution); touch/mobile-
  specific interaction patterns are out of scope.
- Exemptions from migration (FR-008) are expected to be rare and must be
  recorded with a reason rather than silently skipped.
- FR-011's build-under-the-design-system convention outlives this
  feature's initial component set; it applies to component work done in
  later features too, and should be treated as a review-time check (is
  this a new building block? does it already live in the design
  system?) rather than something fully enforceable by an automated test.
