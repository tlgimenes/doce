# Task 2 Report: `ui/widget-frame.tsx` primitive

## Summary

Implemented `WidgetFrame` / `WidgetFrameHeader` / `WidgetFrameContent` exactly
per the brief's Step 3 code, with one required adaptation to the trigger
composition (see below) and one adaptation to the test's visibility
assertions (documented in the brief as expected/likely).

## TDD evidence

**RED** (`npx vitest run src/components/ui/widget-frame.test.tsx` before
creating `widget-frame.tsx`):

```
FAIL  src/components/ui/widget-frame.test.tsx [ src/components/ui/widget-frame.test.tsx ]
Error: Failed to resolve import "./widget-frame" from "src/components/ui/widget-frame.test.tsx". Does the file exist?
```

**GREEN** (final run, after implementation + both adaptations below):

```
 Test Files  1 passed (1)
      Tests  3 passed (3)
```

## Adaptation 1: trigger composition needed `nativeButton={false}`

The brief's `<CollapsibleTrigger render={<Item .../>}>` composition (the
primary approach, not the fallback) worked structurally — `aria-expanded`,
`aria-controls`, click handling, and keyboard support all wired up correctly.
However, the first GREEN attempt still failed 2/3 tests because
`screen.getByRole("button")` couldn't find the trigger.

Root cause: Base UI's `CollapsibleTrigger` defaults `nativeButton={true}`,
which assumes the `render` target ultimately resolves to a real `<button>`
element. In that mode it merges `{ type: "button" }` into the rendered
element's props and does **not** add `role="button"`. Since `Item` (a
`useRender`-based component) defaults to rendering a `<div>`, the resulting
DOM node was a `<div type="button" aria-expanded="...">` with no accessible
button role — confirmed by Base UI's own dev-mode console warning:

```
Base UI: A component that acts as a button expected a native <button> because the
`nativeButton` prop is true. Rendering a non-<button> removes native button
semantics... Use a real <button> in the `render` prop, or set `nativeButton` to `false`.
```

Fix: added `nativeButton={false}` to the `CollapsibleTrigger` in
`WidgetFrameHeader`. This is Base UI's own documented escape hatch for
exactly this case — with it, `useButton` merges `{ role: "button" }` instead
of `{ type: "button" }`, and `getByRole("button")` resolves correctly, along
with proper Enter/Space keyboard activation for a non-native-button element.
This is a one-line addition on top of the brief's primary
`render={<Item/>}` composition — I did **not** need the brief's documented
div-wrapping fallback (`<CollapsibleTrigger render={<div/>}>` wrapping the
`Item`); the `render={<Item/>}` composition stands as designed, just with
`nativeButton={false}` set.

## Adaptation 2: visibility assertion for the closed state

The brief flagged this as a likely necessary adaptation. Base UI's
`Collapsible.Panel` unmounts its content when closed by default (no
`keepMounted` prop set), rather than rendering it hidden via CSS/attributes.
Confirmed empirically: with the frame collapsed (no `defaultOpen`),
`screen.queryByText("body text")` returned `null` (not a hidden element), so
`not.toBeVisible()` threw ("received value must be an HTMLElement... Received
has type: Null") rather than passing.

Per the brief's explicit guidance ("match the primitive's real behavior, do
not force it"), changed that one assertion in `widget-frame.test.tsx` from:

```tsx
expect(screen.queryByText("body text")).not.toBeVisible();
```

to:

```tsx
expect(screen.queryByText("body text")).not.toBeInTheDocument();
```

with a comment explaining why. The subsequent open-state assertions
(`toBeVisible()` after click, and the `defaultOpen` test) needed no change —
once mounted/open, the panel content is a real visible element.

## Files changed

- `src/components/ui/widget-frame.tsx` (new) — `WidgetFrame`,
  `WidgetFrameHeader`, `WidgetFrameContent`, matching the brief's produced
  interfaces exactly, plus the `nativeButton={false}` addition on the
  collapsible trigger.
- `src/components/ui/widget-frame.test.tsx` (new) — brief's three tests, with
  the one visibility-assertion adaptation described above.

## Self-review

- Typecheck: `npx tsc -b` — clean, no output/errors.
- Lint: `npx oxlint src/components/ui/widget-frame.tsx src/components/ui/widget-frame.test.tsx` — clean.
- Format: `npx oxfmt src/components/ui/widget-frame.tsx src/components/ui/widget-frame.test.tsx` — applied (reflowed a few multi-prop JSX lines); re-ran tests after formatting, still 3/3 pass.
- Confirmed the non-collapsible frame (`WidgetFrame` without `collapsible`) renders a plain `<div data-slot="widget-frame">` with an `Item` header and no button role (test 1).
- Confirmed `defaultOpen` renders the collapsible frame already expanded with `aria-expanded="true"` and visible body content (test 3).
- Did not touch any other files. `tsc -b` implicitly typechecks the whole project and passed, so no regressions from this addition.
