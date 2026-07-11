# Marker Widgets, Shimmer Status, Overlay Composer Design

## Summary

Three user directives, one coordinated change to the chat surface:

1. **Widgets become Marker rows.** The read-only tool widgets drop their
   bordered card frames for the stock `Marker` activity-line idiom
   (ui.shadcn.com/docs/components/base/marker): slim muted rows with a tool
   icon (`MarkerIcon`), text (`MarkerContent`), trailing Badges, and — for
   widgets with bodies — a chevron and a `Collapsible` panel underneath.
   The transcript reads as an activity log, not a stack of cards.
2. **The Working status shimmers.** Per the Marker docs' streaming
   pattern: `Spinner` in `MarkerIcon`, `role="status"`, and the `shimmer`
   utility class on `MarkerContent`.
3. **The composer floats inside the scroller.** Workspace's composer
   shell (with PlanTracker and StreamingStatus stacked above it) moves
   into an absolute bottom overlay INSIDE `MessageScroller` (which is
   `relative`), with bottom padding on `MessageScrollerContent` so the
   last message clears it, and the scroll-to-bottom button offset above
   the overlay. MessageScroller keeps filling its height-constrained
   parent per its docs.

## Decisions (autonomous; user directives verbatim: "refactor the widgets
to use the new Marker component", "the Working status should be a Shimmer.
inspire yourself on the shadcn docs …/base/marker", "also MessageScroller
for the message composer")

- App-level utility classes are the project standard (stock components +
  composition); no new ui-layer components.
- Collapsible markers compose stock parts: `CollapsibleTrigger
  nativeButton={false} render={<Marker …/>}` (Marker is useRender-based),
  chevron as the trailing child, `CollapsibleContent` with a `pl-6`
  indent for the body. The docs show no collapsible marker — this is
  plain composition of two stock primitives, same pattern as the widget
  frames used for Item.
- Bodies keep their existing content (mono pre blocks, previews, match
  lists, JSON) unchanged apart from the indent wrapper replacing the
  bordered panel.
- Failed/interrupted states: Marker row + `destructive`/`outline` Badge,
  error text as a stacked second line inside `MarkerContent`
  (`flex-col` per the docs' stacking note). Live Workspace/TranscriptTurn
  error banners keep their Alert form.
- The old `data-slot="widget-frame*"`/`"code-block*"` attributes die with
  the frames (markers carry stock `data-slot="marker"`); every
  `data-testid` survives on the new roots. Tests move to marker-slot
  structure.

## Per-widget mapping

| Widget | Marker row | Body (Collapsible) |
|---|---|---|
| Bash (running) | Spinner icon · shimmer "Running…" · command as second line | open (defaultOpen) — command pre |
| Bash (done) | Terminal icon · command (mono, truncated) · status/exit/token Badges · chevron | stdout/stderr pres; ViewFullOutput; header-only when outputs empty |
| Bash (spawn failed) | Terminal icon · "Failed to run" + error line · destructive Badge | none |
| EditDiff | FilePen icon · path · +N/−N Badges · chevron | diff lines (defaultOpen) |
| EditDiff (failed) | FilePen icon · path + error line · destructive Badge | none |
| Read | FileText icon · "Read <path>" · bytes/token Badges · chevron | ReadPreview + ViewFullOutput |
| Read (failed) | FileText icon · "Read <path>" + error line · destructive Badge | none |
| Write | FilePlus icon · path · "Write · N bytes" second line · "Written" Badge | none |
| Glob/Grep | Search icon · tool+pattern (mono) · count/token Badges · chevron | context + match list; Empty when none |
| Search (interrupted) | Search icon · pattern + interrupted line · outline Badge | none |
| Task | Bot icon · status title (Spinner while running) · prompt second line · Badge | none |
| AskUserQuestion (answered) | MessageCircleQuestion icon · question · "You chose/replied: …" second line · outline Badge when interrupted | none |
| Unknown tool | Wrench icon · tool name · chevron | JSON pre |

Trailing cluster: Badges + chevron sit after `MarkerContent` with
`ml-auto` on the first trailing element (Marker root is `flex items-center
gap-2 w-full`). Multi-line content stacks inside `MarkerContent` with
`flex flex-col` (first line normal, second line `text-xs`-muted per the
existing description styling).

## StreamingStatus

Per the docs' streaming pattern exactly:

    <Marker role="status"-carrier as today>
      <MarkerIcon><Spinner role="presentation" aria-label={undefined}/></MarkerIcon>
      <MarkerContent className="shimmer"><span role="status" …>Working</span></MarkerContent>
      chron (tabular-nums, aria-live="off") trailing as today
    </Marker>

All existing behavior invariants (chron seeding, suppression, live-region
shape) unchanged. If `shimmer` needs the text to be the direct styled node,
apply it to the inner span instead — match what renders (the utility ships
with the shadcn chat set; attachment.tsx already uses it).

## Composer overlay

In `Workspace.tsx`: the block `PlanTracker → StreamingStatus → composer
shell` moves inside `<MessageScroller>` as:

    <div className="absolute inset-x-0 bottom-0 z-10 flex flex-col">
      <PlanTracker …/>
      {showGenericStreamingStatus && <StreamingStatus …/>}
      <div className={composer-shell as today}>…</div>
    </div>

- `MessageScrollerContent` gains bottom clearance (`pb-36`-class layout
  padding; tune to the overlay's typical height).
- `MessageScrollerButton` gets a `bottom-*` offset class so it floats
  above the overlay.
- The composer shell keeps `[view-transition-name:chat-composer]` and its
  conditional border; a `bg-background` on the overlay keeps text
  scrolling behind it readable (utility-level, standard practice).
- Placement tests update: PlanTracker's docking test now asserts the
  tracker inside the scroller overlay ABOVE the composer shell (document
  order tracker → status → composer within the overlay), replacing the
  scroller-root non-containment guard with an overlay-containment guard.
- StreamingStatus invariants ("between transcript and composer, not a
  chat message") still hold inside the overlay.

## Verification

- All widget/workspace/tracker suites updated and green; tsc; lint.
- Runtime pass (one combined run): marker rows render in a real
  transcript, Working shimmers, composer floats with content scrolling
  behind it, scroll button sits above the overlay, light + dark. Also
  re-verifies the composer InputGroup fix (no washed-out group when
  empty).

## Risks

- The overlay changes scroll clearance math; wrong padding shows as the
  last message hiding behind the composer — the runtime pass must
  scroll to bottom with a long transcript to check.
- PlanTracker/StreamingStatus DOM-order tests are contract changes, not
  regressions — updated deliberately.
- Marker rows are visually lighter than cards; density is the requested
  aesthetic.
