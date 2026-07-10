# Tool Widgets Shadcn Unification Design

## Summary

Recompose the 11 read-only tool-call widgets in `src/views/chat/tool-widgets/`
onto a single unified, collapsible widget frame built from stock shadcn
primitives. Two new ui-layer primitives (`widget-frame`, `code-block`) carry
the shared chrome and the one visual shadcn lacks (monospace/diff rendering).
The bespoke `ToolDisclosure` collapsible is deleted. Raw emerald/sky/amber
status colors move to token-based Badges and Spinner. App widget files end up
composed of ui-layer components plus layout utilities only — the same
strictness the transcript surface now follows.

`UserAskWidget` (the live AskUserQuestion composer) is explicitly OUT of
scope and keeps its current chrome for a later pass.

## Scope

In scope — all of `src/views/chat/tool-widgets/` except UserAskWidget:

- `AskUserQuestionWidget.tsx` (the answered/read-only card)
- `BashWidget.tsx`
- `EditDiffWidget.tsx`
- `ReadPreview.tsx`
- `ReadWidget.tsx`
- `SearchResultsWidget.tsx`
- `TaskWidget.tsx`
- `ToolDisclosure.tsx` (deleted)
- `UnknownToolWidget.tsx`
- `ViewFullOutput.tsx`
- `WriteWidget.tsx`

New ui-layer files:

- `src/components/ui/widget-frame.tsx`
- `src/components/ui/code-block.tsx`

Out of scope:

- `UserAskWidget.tsx`, its theme.css animations, and the pending-question
  composer swap behavior
- Tool payload parsing (`parseToolResultDetail` and friends), the
  `ToolWidget` dispatcher in `TranscriptRow`, IPC, persistence
- `WidgetGallery` beyond reference updates (it stays the showcase surface)

## Decisions (user-confirmed)

1. Treatment: same shadcn-only strictness as the transcript refactor
   (2026-07-10-transcript-shadcn-only-design.md) — visuals may shift to
   shadcn defaults.
2. UserAskWidget: excluded for now.
3. Approach: unify the widget UX around one shared collapsible frame while
   migrating (not a minimal per-widget swap).

## Architecture

### The unified frame

Every widget renders through `WidgetFrame`: a header row that is always
visible and an optional collapsible body.

`src/components/ui/widget-frame.tsx` — a composition of stock `Item` +
`Collapsible` (precedent: shadcn's own `CommandDialog` composes Dialog +
Command; the doce `Bubble` `user` variant carries project identity in the ui
layer). Exports:

- `WidgetFrame` — root; owns the Collapsible state (`defaultOpen` prop;
  uncontrolled). Renders an `Item variant="outline"`-based card shell. When
  the widget has no body, renders the header alone with no trigger
  affordance.
- `WidgetFrameHeader` — the always-visible row inside `CollapsibleTrigger`
  (or a plain row when no body): `ItemMedia variant="icon"` for the tool
  icon, `ItemContent` › `ItemTitle`/`ItemDescription` for primary and
  secondary text, `ItemActions` for Badges/Spinner/chevron.
- `WidgetFrameContent` — `CollapsibleContent` wrapping the body.

Chrome (borders, radii, hover, chevron rotation) lives entirely in this file
using semantic tokens. App widgets pass content only.

### The code block

`src/components/ui/code-block.tsx` — the one visual shadcn does not ship.
Exports:

- `CodeBlock` — monospace block, `text-xs`-scale mono, `overflow-x-auto`,
  token colors (`bg-muted/50` family), used for terminal output, JSON dumps,
  plain-text file previews, offloaded payloads, and inline command display.
- `CodeBlockLine` — per-line row with `variant: "default" | "added" |
  "removed"` for the diff (added/removed tints defined here, tokens first;
  this file is the sanctioned home for diff colors).

### Status → token mapping

Raw palette classes disappear from app files:

- running → `Spinner` (decorative, `role="presentation"`) + muted text
- success → `Badge variant="secondary"`
- failed / nonzero exit code → `Badge variant="destructive"`
- interrupted → `Badge variant="outline"`

### Deletion

`ToolDisclosure.tsx` and its native `<details>`/`<summary>` styling hacks
are deleted; `WidgetFrame` covers the disclosure behavior via Collapsible.

## Per-widget mapping

Existing testids and user-visible text contracts are preserved.

| Widget | Header | Body | Default state |
|---|---|---|---|
| Bash | Terminal icon · command (inline CodeBlock) · exit/status Badge or Spinner · token chip | stdout/stderr in CodeBlock | collapsed; pending/running expanded |
| EditDiff | FilePen icon · file path · +N/−N Badges | CodeBlockLine added/removed rows | expanded |
| Read | FileText icon · path · bytes/tokens chips | ReadPreview | collapsed |
| Write | FilePlus icon · path · "N bytes" Badge | none | header-only |
| Glob/Grep | Search icon · pattern · match-count Badge | ItemGroup/Item match rows; Empty when none | collapsed |
| Task | Bot icon · status title (Spinner while running) · prompt as description · Badge (done/interrupted) | none | header-only |
| AskUserQuestion (answered) | MessageCircleQuestion icon · question as description · answer line as title | none | header-only |
| Unknown tool | Wrench icon · tool name | pretty JSON in CodeBlock | collapsed |

Error branches (spawn failed, edit/read/write failed): `Alert
variant="destructive"` › `AlertDescription` inside the frame, replacing the
four hand-rolled `border-destructive/40 bg-destructive/10` cards.
Interruptions render as `outline` Badges in the header, not error Alerts.

ReadPreview internals: loading → `Spinner`; unavailable/error → `Empty` ›
`EmptyTitle`/`EmptyDescription`; media playback stays native
`<img>`/`<video>`/`<audio>` elements with sizing-only classes (no
`Attachment` framing — YAGNI, the frame body already provides the card
context); markdown stays `MarkdownPreview` (sanctioned prose exception);
plain text → `CodeBlock`.

ViewFullOutput: raw link-styled `<button>` → `Button variant="link"
size="sm"`; loading → `Spinner`; loaded payload → `CodeBlock`.

## Behavior contracts (frozen)

- Tool payload parsing and the `ToolWidget` dispatcher are untouched;
  FR-011 holds: unknown or malformed payloads always render a diagnostic
  widget, never blank/broken/dropped.
- Pending Bash/Task widgets keep rendering inside the latest turn body;
  running state renders expanded.
- Read/Search collapse semantics from
  2026-07-09-collapsible-read-search-widgets-design.md survive identically
  (summary line content, collapsed by default).
- `ViewFullOutput`'s offloaded-payload IPC fetch is unchanged.
- No backend/IPC changes.

## Strictness rules

Identical to the transcript spec: in-scope app files may use only layout
utilities (flex/grid, gap, spacing, sizing, overflow/position, truncate);
no color/typography/border/shadow/radius utilities, no arbitrary values or
properties, no palette colors. All visuals come from ui-layer components.
`MarkdownPreview` remains the sole prose exception.

## Testing

- Per-widget colocated tests update to frame structure: data-slots, roles,
  Badge text, collapsible `data-state`, header/body text. Class assertions
  pinning old bespoke chrome are dropped, not ported.
- New colocated tests for `widget-frame` (header-only vs collapsible,
  defaultOpen, trigger toggling) and `code-block` (variants render, line
  variants) in `src/components/ui/`.
- `WidgetGallery` reference updates keep it compiling; it remains the
  visual verification surface for all widget states.

## Verification gates

- `npm run build`, `npm test`, `npm run lint`; no-Radix grep stays clean.
- Compliance sweep over `src/views/chat/tool-widgets/`: layout-only
  utilities, zero arbitrary values/properties, zero palette colors.
- Runtime: WidgetGallery screenshot pass (renders every widget state
  without model turns) plus the e2e `tool-call-widgets` spec with
  `DOCE_E2E_SKIP_WIPE=1`.

## Risks

- Bash collapsed-by-default is a real UX change; the header keeps command +
  status visible. EditDiff stays expanded to protect the review flow.
- The frame must not regress the Read/Search spec'd collapse behavior; its
  tests pin that contract.
- e2e `tool-call-widgets` spec may assert old DOM; budget for updating it.
