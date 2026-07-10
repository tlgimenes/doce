# Transcript Shadcn-Only Refactor Design

## Summary

Migrate the chat transcript surface to compose stock shadcn components only.
App-level transcript files stop carrying visual styling: no bespoke components,
no custom classes beyond layout utilities on wrappers, with four named
exceptions. Three components are deleted outright (`StickyUserMessage`,
`UserMessageBubble`, `ContextUsageGauge`), the custom `use-stick-to-bottom`
scroller is replaced by the already-installed shadcn `MessageScroller`
primitive, and the row dispatcher is renamed to end a name collision with the
stock message primitives.

This tightens the 2026-07-09 shadcn Base UI redesign, which sanctioned
app-specific components that "compose shadcn components". After this refactor,
in-scope app files may only arrange stock primitives with flex/grid layout
wrappers. Visuals are allowed to shift to shadcn defaults.

## Scope

In scope (the transcript pane and its satellites):

- `src/views/workspace/Workspace.tsx` (transcript shell; composer shell inside
  it is out of scope)
- `src/views/workspace/TranscriptTurn.tsx`
- `src/components/MessageContent.tsx` (renamed, see below)
- `src/components/UserMessageBubble.tsx` (deleted)
- `src/views/workspace/StickyUserMessage.tsx` (deleted)
- `src/views/workspace/StreamingStatus.tsx`
- `src/views/workspace/PlanTracker.tsx`
- `src/views/workspace/WorkspaceTopbar.tsx` and
  `src/components/ContextUsageGauge.tsx` (deleted)
- `src/components/Timer.tsx` (kept; logic-only)
- `src/components/MarkdownPreview.tsx` (kept; sanctioned exception)

Out of scope, per explicit user decision:

- Sidebar (`ConversationList`, `SearchPanel`, `ConversationSearchDialog`)
- Tool widgets (`src/views/chat/tool-widgets/`)
- Composer (`RichInput`, `EmptyState`, `UserAskWidget`, the composer shell's
  view-transition hook)
- `Topbar.tsx` portal infrastructure (Tauri drag region)
- The `src/components/ui/` layer itself, including its sanctioned adaptations
  (8px radius cap, `Bubble` `user` variant, custom `Button` API). Restoring
  `ui/button.tsx` to the stock registry version is a separate follow-up if
  desired; nothing in the refactored transcript imports `Button` directly.
- `WidgetGallery` visuals (only compile-level reference updates)

## Decisions (user-confirmed)

1. Scope: transcript + messages, plus all three satellites (StickyUserMessage,
   PlanTracker, WorkspaceTopbar contents).
2. Strictness: stock shadcn components with documented variants only. Wrappers
   limited to layout utilities. No arbitrary values, no arbitrary properties,
   no color/typography/border/shadow/radius utilities in app files, no new
   `theme.css` rules. Visuals may shift to shadcn defaults.
3. Markdown: `MarkdownPreview` (react-markdown + `prose prose-sm
   dark:prose-invert max-w-none`) stays as the single typography exception. No
   other in-scope file may use `prose` or typography utilities.
4. Sticky user message: behavior dropped entirely. The user message renders as
   a plain stock `Bubble` at the top of its turn.
5. Execution: one coordinated pass (staged commits per surface, single arc),
   not a two-phase or minimal-diff migration.

## Ground rules

`src/components/ui/` is the shadcn layer and the only place visual identity
lives. In-scope app files compose it and may use only:

- flex/grid: `flex`, `grid`, `flex-col`, `items-*`, `justify-*`, `gap-*`
- spacing: `p-*`, `m-*`, `mx-auto`, `space-*`
- sizing: `w-*`, `h-*`, `min-w-0`, `max-w-3xl`, `flex-1`, `shrink-0`,
  `size-*`
- overflow/position where structural: `overflow-*`, `relative`, `absolute`,
  `inset-*`, `z-*`, `truncate`
- standard container queries: `@container`, `@5xl:*`

Named exceptions (each single-location):

1. `MarkdownPreview` prose classes (decision 3).
2. `tabular-nums` on the streaming chron and the message `Timer` output — the
   streaming-status spec requires jitter-free numerals and no shadcn primitive
   provides them.
3. `[view-transition-name:chat-composer]` on the composer shell in
   `Workspace.tsx` — composer is out of scope.
4. Tauri drag-region plumbing in `WorkspaceTopbar` (`pointer-events-none`/
   `pointer-events-auto`, `data-topbar-no-drag`) — functional, not visual.

## Deletions

- `src/views/workspace/StickyUserMessage.tsx` + test. Also delete the sticky
  masking shim in `TranscriptTurn` and the `min-h-[100cqh]` +
  `[container-type:size]` viewport trick in `Workspace` (both existed to serve
  the sticky UX). PlanTracker's container queries move to the standard
  `@container` utility on the same wrapper.
- `src/components/UserMessageBubble.tsx` + test — redundant with
  `Bubble variant="user"` + `MessageFooter`.
- `src/components/ContextUsageGauge.tsx` + test — SVG donut and hand-rolled
  tooltip replaced by stock `Progress` + `Tooltip` (visual change:
  donut → small bar).
- `use-stick-to-bottom` from `package.json` — `Workspace.tsx` is the sole
  consumer.

## Rename

`src/components/MessageContent.tsx` → `src/views/workspace/TranscriptRow.tsx`
(component `TranscriptRow`). It dispatches transcript rows and name-collides
with `ui/message.tsx`'s `MessageContent`. Same logic, views-layer home per
repo convention. `WidgetGallery` imports updated.

## Per-surface mapping

### Workspace.tsx

- `StickToBottom` render-prop → `MessageScrollerProvider` › `MessageScroller`
  › `MessageScrollerViewport` › `MessageScrollerContent` (stock wrappers over
  the `@shadcn/react` headless scroller).
- Custom scroll-to-bottom `Button` (glassy pill restyle) → stock
  `MessageScrollerButton` with built-in detached-state show/hide.
- Re-arm on send: call the primitive's `useMessageScroller().scrollToBottom()`
  where `scrollToBottom` is called today.
- Generation-error box → `Alert variant="destructive"` › `AlertDescription`.
- Transcript column keeps `mx-auto max-w-3xl` (layout).
- Composer shell untouched.

### TranscriptTurn.tsx

- Turn wrapper → `MessageGroup` (its `gap` replaces per-row `mb-*` margins).
- User message at turn start → `Message align="end"` › `Bubble
  variant="user"` › `BubbleContent`; body via `MarkdownPreview` (markdown) or
  `UserMessageContent` (rich text); uploaded-token meter via `MessageFooter`.
- Sticky shim deleted.
- Inline error → `Alert variant="destructive"` › `AlertDescription`.
- Pending Bash/Task widget call-sites untouched.

### TranscriptRow.tsx (renamed dispatcher)

- assistant → `Message` › ui `MessageContent` › `MarkdownPreview`; the
  metadata line (`Timer` + "↓ N tokens") moves from a bare `<p>` into
  `MessageFooter`.
- user → the same `Message align="end"` › `Bubble variant="user"`
  composition described under TranscriptTurn. The composition is written
  once; implementation decides whether it lives inline in `TranscriptTurn`
  or in `TranscriptRow`'s user branch — not both.
- error → `Alert variant="destructive"` (replaces the destructive-restyled
  `Marker`).
- context notice → plain `Marker` › `MarkerContent`, stock styling (drop the
  `bg-muted` box restyle and the `/70` opacity tier).
- tool_result → existing widgets, unchanged.
- Drop the `max-w-none` stacking on `Message`/`MessageContent`/`Bubble`/
  `BubbleContent` (`variant="ghost"` already yields full width).

### StreamingStatus.tsx

- Component and behavior stay (chron seeded from optimistic user-message
  `createdAt`, never resets on tool calls; `role="status"`; animation
  `aria-hidden`; suppressed while a dedicated pending widget shows; rendered
  between transcript and composer, never as a chat message).
- Chrome → `Marker` with `Spinner` as the icon, "Working" + chron in
  `MarkerContent`. Chron keeps `tabular-nums` (exception 2), drops `w-[7ch]`
  and `font-mono` (accept the trivial width change).
- Hand-rolled `[animation-delay:*]` dots, `border-b` bar chrome, and bespoke
  typography deleted; the wrapper is flex layout only.

### PlanTracker.tsx

- `PlanCard` → `Card` › `CardHeader`/`CardTitle`/`CardContent`; glassy
  `bg-card/95 backdrop-blur` recipe dropped for the stock Card look.
- Steps → `ItemGroup`/`Item` with lucide `Check`/`Circle` icons in
  `ItemMedia`; done/current/todo expressed through stock Item/Badge variants
  and semantic tokens; hardcoded `emerald`/`amber` classes removed.
- n/m chip → `Badge`; card header adds stock `Progress` showing done/total.
- Collapsed rail → compact `Card` of numbered `Badge`s (done=`default`,
  current=`secondary`, todo=`outline`); collapse toggle → `Button
  variant="ghost" size="sm"`.
- Card/rail split via standard `@container` + `@5xl:` variants.
- 300 ms fade-out-then-unmount timer dropped; plain unmount.

### WorkspaceTopbar.tsx

- Title/path stack → `Item` › `ItemContent` › `ItemTitle`/`ItemDescription`.
- Context gauge → stock `Progress` (small fixed width, layout) wrapped in
  stock `Tooltip`; usage numbers and warning/just-compacted state move into
  tooltip copy; amber/emerald state colors dropped.
- Portal + drag-region plumbing unchanged (exception 4).

### Timer.tsx

- Kept as-is (interval logic, bare span). Rendered inside `MessageFooter` so
  typography comes from the primitive. `tabular-nums` under exception 2.

## Behavior contracts

- Data flow unchanged: `Workspace` derives all signals; children receive
  props. No IPC/backend changes. `groupTranscriptTurns` untouched.
- Scroll semantics must survive the scroller swap: auto-follow during
  streaming, escape on scroll-up, scroll button when detached, re-arm on
  send. If the `@shadcn/react` primitive cannot reproduce escape/re-arm
  semantics, stop and report — do not paper over with custom code.
- Error/fallback behavior preserved: unknown or malformed tool payloads still
  render diagnostic widgets; generation errors render (now as `Alert`)
  without erasing transcript state; pending AskUserQuestion still replaces
  the composer; pending Bash/Task widgets render inside the latest turn body.

## Testing

- jsdom cannot prove real scrolling: unit tests assert structure and wiring
  (viewport/content/button present via data-slots/roles, `scrollToBottom`
  invoked on send); real scroll behavior is verified in the running app and
  via the `workspace-chat` e2e spec with `DOCE_E2E_SKIP_WIPE=1`.
- Updated colocated tests: `Workspace.test.tsx` (scroller structure, error
  Alert, send re-arm), `TranscriptTurn.test.tsx` (MessageGroup structure,
  user Bubble, shim gone), `TranscriptRow.test.tsx` (renamed; row-kind
  dispatch, Alert/Marker swap), `StreamingStatus.test.tsx` (Spinner,
  invariants), `PlanTracker.test.tsx` (Card/Item/Badge structure, container
  split), `WorkspaceTopbar.test.tsx` (Item stack, Progress + Tooltip).
- Tests for deleted components are deleted with them.
- Tests target behavior and stable structure (roles, `data-slot`s, text) per
  repo convention; class assertions that pinned bespoke styling are removed,
  not ported.
- `WidgetGallery` reference updates keep the app compiling.

## Verification gates

Before claiming completion:

- `npm run build`, `npm test`, `npm run lint`, `npm run format:check`
- `rg "@radix-ui|radix-ui" src package.json` stays clean
- Compliance sweep over in-scope files: no styling utilities beyond the
  layout allowlist and the four named exceptions — specifically no arbitrary
  values (`text-[13px]`-style), no arbitrary properties (`[mask-image:...]`),
  no palette colors (`emerald-*`/`amber-*`), no `bg-*/text-*/border-*/
  shadow-*/rounded-*` in app transcript files.
- Manual run of the app to verify scroll semantics and overall transcript
  rendering.

## Risks

- Scroller swap is the largest behavioral risk; mitigated by verifying
  escape/re-arm in the running app early (first surface in the pass) and by
  the e2e workspace-chat spec.
- Dropping the sticky user message and the 100cqh trick changes scroll UX
  (new turns no longer force-fill the viewport). Accepted by decision 4.
- Visual regressions are expected and accepted (strictness decision):
  donut → bar gauge, plan tracker loses glassmorphism/status colors, user
  bubble width 85% → 80%, context notices lose their boxed look.
- `ConversationList.test.tsx` full-suite flake (GPU load) predates this work;
  test runs use isolation where needed.
