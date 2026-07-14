# Todo List Text Truncation — Design

**Date:** 2026-07-13
**Status:** Approved

## Problem

Long step descriptions in the plan/todo tracker (`PlanTracker`) do not truncate.
The `truncate` class is already applied to both titles (expanded step rows and
the collapsed one-liner trigger), but the shared `Item` primitives defeat it:

- `ItemContent` is `flex-1` without `min-w-0`, so it cannot shrink below the
  text's intrinsic width — long text pushes the row wide, and because `Item`
  itself is `flex-wrap`, the badge + chevron on the collapsed trigger wrap onto
  a second line.
- `ItemTitle` is `flex w-fit` — `w-fit` grows the box to fit its content so it
  never overflows, and a flex container clips overflowing text without ever
  rendering an ellipsis.

Other views already work around this by hand (`WorkspaceTopbar` adds
`min-w-0 flex-1 truncate`; `Settings` adds `min-w-0` to `ItemContent`),
confirming the footgun lives in the primitive.

## Decision

Single-line ellipsis everywhere in the plan card (no multi-line clamp), fixed
in the shared primitive rather than patched a third time at a call site. Full
text stays available via the existing `title` hover tooltip.

## Changes

1. **`src/components/ui/item.tsx` — `ItemContent`:** add `min-w-0` to the base
   classes. `flex-1` children can now shrink; the collapsed one-liner's badge +
   chevron stay on one line. Existing manual `min-w-0` overrides in Settings
   and WorkspaceTopbar become harmless no-ops.
2. **`src/components/ui/item.tsx` — `ItemTitle`:** replace `w-fit` with
   `min-w-0 max-w-full` so a title can be constrained by its parent. Keep
   `flex`, which Settings relies on (`flex-wrap` with inline badges).
3. **`src/views/workspace/PlanTracker.tsx`:** because `ItemTitle` is a flex
   container, `text-overflow: ellipsis` on it never renders — so wrap the step
   text in an inner `<span className="truncate">` in both places (expanded step
   rows and the collapsed trigger). The `title` attribute stays on `ItemTitle`.
4. **`src/views/design-system/WidgetGallery.tsx`:** add or extend a plan mock
   with a very long step description so the truncation case is permanently
   visible in the Cmd+D gallery.

## Verification

- Visual: build and drive the real app via the `verify` skill against the
  WidgetGallery long-text mock; screenshot collapsed and expanded states.
- Regression eyeball of Settings and the workspace topbar, since the primitive
  changed.
- Existing unit tests pass untouched (this is CSS-only behavior).

## Out of Scope

- Styling changes to other `Item` consumers beyond confirming no regression.
- Multi-line clamping.
