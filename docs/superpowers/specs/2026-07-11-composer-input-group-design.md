# Composer on Stock InputGroup Design

## Summary

Recompose RichInput's shell — the last hand-rolled chrome in the app — onto
stock `InputGroup`. The TipTap editor (protected: attachments, skill
mentions, paste-chips, submit semantics) stays exactly as is; only its
wrapper changes. One component change restyles all three composing surfaces
(Workspace composer, EmptyState, UserAskWidget free-text mode).

## Mapping

- The bespoke shell div (`flex flex-col rounded-lg border border-border
  bg-card shadow-sm`, RichInput.tsx:488) → `InputGroup` (stock bordered
  rounded-lg field; renders as a column automatically via its
  `has-[>[data-align=block-end]]` rules).
- The TipTap `EditorContent` becomes the group's control: wrapped in a
  layout-only `flex-1 w-full` div, and the editor's `editorProps.attributes`
  gains `"data-slot": "input-group-control"` so focusing the editor lights
  the stock focus ring (`has-[[data-slot=input-group-control]:focus-visible]`).
  The existing editor attribute class (`min-h-12 w-full px-3 py-2 text-sm
  leading-6 outline-none [&_p]:m-0`) stays.
- The bottom row (`flex items-center justify-between px-3 pb-2`,
  line 501) → `InputGroupAddon align="block-end"`:
  - attach button → `InputGroupButton size="icon-xs" aria-label` (ghost
    default), keeping its testid and handler;
  - anything else in the left cluster keeps its current composition inside
    the addon;
  - send button → `InputGroupButton variant="default" size="icon-sm"
    className="ml-auto"` keeping `submitTestId`, disabled logic, and the
    SendHorizontal icon.
- The attachment-error line and any below-shell elements stay outside the
  InputGroup unchanged.
- Delete the now-dead bespoke class strings; no `src/components/ui/**`
  edits.

## Contracts preserved

- Enter submits / Shift+Enter newline; skill-picker Enter deference; paste
  collapse; attachment flows; disabled states; all testids
  (`inputTestId`, `submitTestId`, `rich-input-attachment-error`, attach
  button's testid if present).
- Known nit accepted: InputGroupAddon's built-in click-to-focus targets
  `input` elements and no-ops for the contenteditable — same as today's
  behavior (no shell click-to-focus existed).

## Verification

- RichInput + UserAskWidget + EmptyState + Workspace suites green; tsc;
  compliance grep (no bespoke border/bg/shadow classes left in
  RichInput.tsx beyond layout).
- Runtime screenshot: composer renders as a stock InputGroup field in
  light and dark.
