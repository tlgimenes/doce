# Collapsible Read/Search widgets

**Status**: Approved
**Context**: Follow-up to `2026-07-08-read-widget-grouped-success-design.md`. That design grouped Read success/truncated/offloaded into one UI, but the retained `ViewFullOutput` row still made offloaded reads look visually different from standard successful reads. This design supersedes the Read-only final UI direction and expands the same collapsed/expanded treatment to `Read`, `Grep`, and `Glob`.

## Motivation

Read, Grep, and Glob are all result-bearing tools. Their widgets should first read as compact activity summaries in the conversation, then reveal details inline when the user asks for them. The default state should not flood the chat with file contents or match lists, and offloaded/truncated Read results should not look like separate success states.

A shared disclosure shell gives these widgets one visual and interaction model:

- Compact summary in the collapsed state.
- Chevron on the right for expansion.
- Inline expanded content with a max height and internal scroll.
- Tool-specific preview/list content inside the same surface.

## Scope

- Add a shared disclosure shell for successful `Read`, non-interrupted `Grep`, and non-interrupted `Glob` results.
- Use native `<details>` / `<summary>` semantics for the shared shell.
- Keep collapsed summaries visually quiet and consistent with the current card style.
- Render expanded bodies inline with a fixed max height and internal scrolling.
- Use a hybrid Read preview strategy:
  - captured `detail.outcome.content` for text-like previews;
  - disk-loaded bytes through existing `commands.readAttachedFile` for native/binary previews.
- Keep failure and interrupted states visibly distinct from normal successful disclosures.
- Update the design-system gallery and focused widget tests.

Out of scope:

- Syntax highlighting.
- A full file browser/sidebar.
- Editing files from previews.
- Persisting expanded/collapsed state across rerenders or sessions.
- Automatically fetching full text for every text file.
- Changing the model-facing Read/Grep/Glob tool behavior.
- Changing Bash, Write, Edit, Task, AskUserQuestion, or UnknownTool widgets.

## Shared Disclosure Shell

Create a small shared component at `src/views/chat/tool-widgets/ToolDisclosure.tsx`, named `ToolDisclosure`.

Responsibilities:

- Render one `rounded-lg border border-border bg-card text-sm` surface.
- Render a native `<details>` / `<summary>` disclosure control with the summary row as the clickable target.
- Place the chevron on the far right of the summary row.
- Rotate or otherwise visually change the chevron when open.
- Render the expanded body directly under the summary inside the same surface.
- Apply `max-h-80 overflow-y-auto` to the expanded body.
- Avoid nested cards in the body.

The component should accept:

- summary content as React children;
- expanded body content as React children;
- optional test ids for the root, summary, and body;
- optional body class names for tool-specific layout.

It should not know about Read/Grep/Glob data. Tool-specific widgets build their own summary/body and pass them in.

## ReadWidget Behavior

### Successful summary

Every successful Read result renders the same collapsed summary shape:

```text
Read <path> · <bytes> · <tokens> tok
```

Rules:

- Byte count comes from `detail.outcome.content.length`.
- Token count appears when `detail.tokenCount != null`.
- If `tokenCount` is absent on older rows, omit only the token segment.
- `detail.outcome.truncated` does not render an `Output truncated` row, warning badge, or separate state.
- `detail.offloadedTo` does not render a separate row, badge, or state label.
- The chevron is the only visible expansion affordance.

### Successful expanded body

Read expansion uses a hybrid preview strategy.

Text-like files:

- Render `detail.outcome.content`.
- This preserves the exact content captured in the tool result, which is what the agent saw.
- Text-like extensions are `.txt`, `.md`, `.markdown`, `.mdx`, `.json`, `.yaml`, `.yml`, `.toml`, `.rs`, `.ts`, `.tsx`, `.js`, `.jsx`, `.css`, `.html`, `.py`, `.sh`, `.sql`, `.xml`, `.csv`, `.log`, `.ini`, `.env`, `.go`, `.java`, `.c`, `.cpp`, `.h`, `.hpp`, `.swift`, `.kt`, `.rb`, `.php`, and `.vue`.
- Plain text/source/config should render as whitespace-preserving monospace text.
- Markdown extensions (`.md`, `.markdown`, `.mdx`) should render with the app's existing markdown renderer. If `MessageContent` cannot be reused directly without changing its public behavior, extract a small shared markdown preview helper and use it from both places.

Native/binary preview candidates:

- Use `commands.readAttachedFile(detail.filePath)` to load bytes when the file extension is one of the supported native preview types below.
- Render images with `<img>`.
- Render video/audio with native controls.
- Supported image extensions: `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.svg`.
- Supported video extensions: `.mp4`, `.webm`, `.ogg`, `.mov`.
- Supported audio extensions: `.mp3`, `.wav`, `.ogg`, `.m4a`, `.flac`.
- PDF is unsupported in this pass and should render `Preview unavailable`.
- The preview should use `data:<mimeType>;base64,<data>` URLs from `readAttachedFile`.

Unsupported files:

- Show a compact `Preview unavailable` message in the expanded body.
- Include enough context for the user to understand what was read: path and metadata.
- Do not show a broken media element.

Errors:

- If disk loading fails for a native preview, show a small inline error in the expanded body.
- The collapsed summary remains unchanged.
- The error does not turn the successful Read result into a failure card, because the original Read succeeded; only preview loading failed.

### Offloaded and truncated reads

Offloaded and truncated successful Reads use the exact same collapsed visual treatment as standard successful Reads.

This pass does not use `offloadedTo` as a preview source. Text-like previews use captured `outcome.content`; native previews load from `detail.filePath`. `offloadedTo` remains persisted data for possible future full-output access, but it does not create visible UI or change collapsed/expanded behavior in this pass.

## SearchResultsWidget Behavior

`SearchResultsWidget` uses `ToolDisclosure` for non-interrupted `Glob` and `Grep` results.

### Glob summary

Collapsed summary:

```text
Glob <pattern> · <N> files · <tokens> tok
```

Rules:

- Use `files`, not `matches`, in the visible count.
- Use `0 files` for zero results.
- Append token count when present.
- Keep `path` out of the summary; render it in the expanded body when present.

Expanded body:

- Render the matched file list in monospace rows.
- For zero results, render `No files matched`.
- Include `path` context when present.
- Body is max-height limited and scrollable.

### Grep summary

Collapsed summary:

```text
Grep <pattern> · <N> matches · <tokens> tok
```

Rules:

- Use `matches` for plural and `match` for a count of one.
- Use `0 matches` for zero results.
- Append token count when present.
- Keep `path` and `glob` out of the primary summary; render them in the expanded body when present.

Expanded body:

- Render rows as `path:lineNumber: line`.
- For zero results, render `No matches found`.
- Include `path` and `glob` context when present.
- Body is max-height limited and scrollable.
- Match rows remain monospace and may keep the current per-row `truncate` behavior inside the scroll area.

### Interrupted searches

Interrupted Grep/Glob results stay distinct and do not use the normal successful disclosure shell.

Rules:

- Keep the amber interrupted notice visible without requiring expansion.
- Do not present interrupted empty `matches: []` as a normal expandable zero-result success.
- Preserve the current semantic message: the app closed before the search finished.

## Accessibility

- Native `<details>` / `<summary>` should provide baseline keyboard and screen-reader behavior.
- The summary row should remain keyboard-focusable and visibly focusable through the app-wide focus ring.
- The chevron must be decorative if the summary text already communicates the disclosure purpose.
- Expanded body content should be reachable in normal document flow.
- Media previews need meaningful `alt` text or labels based on file name/path.

## Data Flow

No new backend result shape is required.

Read:

- Uses existing `ReadDetail`.
- Text preview uses `detail.outcome.content`.
- Native previews use existing `commands.readAttachedFile(detail.filePath)`.
- `filePath: null` degrades to `Preview unavailable`.

Grep/Glob:

- Use existing `GrepDetail` and `GlobDetail`.
- Count summaries derive from `matches.length`.
- Token metadata uses existing optional `tokenCount`.

MIME detection:

- The existing `read_attached_file` command detects common image types and falls back to `application/octet-stream`.
- Implementation must extend `detect_mime_type` for the supported native-preview extensions that are not already covered: `.svg`, `.mp4`, `.webm`, `.ogg`, `.mov`, `.mp3`, `.wav`, `.m4a`, and `.flac`.
- Add focused Rust tests for the new extension mappings.

## Gallery Updates

`WidgetGallery.tsx` should show collapsed/expandable examples for:

- Read standard text/source file.
- Read native preview candidate in collapsed form. If the gallery example is expanded interactively without a real local fixture path, it should degrade through the normal preview-unavailable path.
- Read failure.
- Glob with files.
- Glob no files.
- Grep with matches.
- Grep no matches.
- Interrupted search if the gallery already includes or can safely add that scenario.

The gallery should no longer imply that Read offload/truncation is a separate visual state.

## Testing

Add or update focused tests.

Shared disclosure:

- Collapsed summary renders.
- Expanded body is hidden until opened.
- Opening reveals the body.
- Body has max-height/overflow styling.
- Chevron/control semantics are present.

ReadWidget:

- Successful collapsed summary includes path, bytes, and optional tokens.
- Offloaded and truncated successful Reads do not render separate rows or labels.
- Opening a text-like Read renders `detail.outcome.content`.
- Markdown/text/source previews are height-limited.
- Native-preview file paths call `commands.readAttachedFile`.
- Native preview success renders the appropriate media element for supported MIME.
- Native preview failure renders inline preview error while keeping the successful summary.
- Unsupported file types render `Preview unavailable`.
- Failure card remains distinct and non-disclosure.

SearchResultsWidget:

- Glob collapsed summary shows file count and optional tokens.
- Opening Glob shows file rows or zero-results message.
- Grep collapsed summary shows match count and optional tokens.
- Opening Grep shows match rows or zero-results message.
- Expanded bodies are height-limited.
- Interrupted search stays distinct and visible without expansion.

Backend tests:

- `read_attached_file` detects any newly supported extensions.
- Unknown extensions still fall back safely.

## Acceptance Criteria

- Successful Read, Grep, and Glob results render collapsed by default.
- Each successful collapsed widget has a right-side chevron.
- Expanding a widget reveals inline details inside a max-height scrollable body.
- Read text previews prefer captured `outcome.content`.
- Read native previews load bytes through the existing attachment read command.
- Offloaded/truncated successful Reads do not look different from standard successful Reads while collapsed.
- Grep/Glob match lists no longer expand the conversation by default.
- Interrupted and failed states remain visibly distinct.
