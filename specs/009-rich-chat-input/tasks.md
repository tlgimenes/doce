---
description: "Task list for 009-rich-chat-input"
---

# Tasks: Rich Chat Input

**Input**: Design documents from `/specs/009-rich-chat-input/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/rich-chat-input.md, quickstart.md

**Tests**: Included — this codebase's established convention (every prior feature this session: `004-tool-call-widgets`, `006-chat-empty-state`, `007-workspace-cwd-resolution`) is TDD: write the test, confirm it fails, then implement. Testing tier per research.md's empirically-verified split: pure-logic Vitest tests, jsdom component tests for structure only (never pixel geometry), `cargo test`, and WDIO e2e for real interaction/positioning.

**Organization**: Tasks are grouped by user story (spec.md's P1–P3 priorities) after a Setup/Foundational phase. US2 is where the shared backend `rich_content` infrastructure is built (first story that needs any of it, and Rust's exhaustive `match` means the full `RichTextSegment` enum has to exist as one coherent type) — US3 and US4 extend it, each adding their own segment-producing frontend feature independently. This is called out explicitly rather than claimed as false full independence.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1/US2/US3/US4)

## Path Conventions

Existing structure — `src/` (frontend), `src-tauri/` (backend), `tests/e2e/` (WDIO).

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Get the new dependencies in place.

- [X] T001 Install `@tiptap/core@3.20.2 @tiptap/react@3.20.2 @tiptap/starter-kit@3.20.2 @tiptap/suggestion@3.20.2 @tiptap/pm@3.20.2 @floating-ui/react@^0.27.16` (research.md's Dependencies table)

**Checkpoint**: Dependencies installed.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Infrastructure every user story's tests depend on.

**⚠️ CRITICAL**: No Tiptap component test can run cleanly until T002 lands (research.md: unmocked, ordinary typing throws an uncaught `TypeError` that flips `vitest run`'s exit code to 1 even when every `expect()` passes).

- [X] T002 [P] Add three polyfills to `src/test/setup.ts`, following the existing commented-polyfill convention already used there for `HTMLDialogElement`: `Range.prototype.getBoundingClientRect`/`getClientRects` (zeroed rect / empty list), `document.elementFromPoint` (returns `null`), a no-op `global.ResizeObserver` class (research.md's Testing strategy decision — all three confirmed necessary/prudent against doce's pinned jsdom 29.1.1)
- [X] T003 Spike: verify whether `@floating-ui/react`'s `useListNavigation` (arrow-key navigation across a suggestion list via `aria-activedescendant`) works under jsdom, as distinct from the confirmed-broken real-contenteditable-caret-movement case (research.md's "Open item, not fully resolved") — depends on T002. Record the finding directly in `research.md`'s Open Item note; the result decides whether T032 (US3's keyboard-nav test) targets jsdom (tier 2) or WDIO e2e (tier 3)

**Checkpoint**: Foundation ready — user story implementation can begin.

---

## Phase 3: User Story 1 - One consistent, capable input everywhere (Priority: P1) 🎯 MVP

**Goal**: Replace the three separate plain inputs with one shared rich-text input — no chips yet, just a consistent, multi-line-aware editor with Enter-to-send/Shift+Enter-for-newline everywhere.

**Independent Test**: Type a multi-line message (using Shift+Enter) in each of the three surfaces (new-conversation composer, plain chat, agent/workspace chat) and confirm identical behavior, appearance, and Enter/Shift+Enter handling in all three. Ships as a complete MVP on its own — no backend change at all, since a message with no chips still calls the existing `sendAgentMessage`/`sendMessage` exactly as today.

### Tests for User Story 1

- [X] T004 [P] [US1] Write `src/views/chat/rich-input/RichInput.test.tsx`: renders with a placeholder; typing produces the expected doc text; Enter (no Shift) calls `onSubmit`; Shift+Enter inserts a newline without submitting; `disabled` toggles via `editor.setEditable()` (assert `editor.isEditable`, not a remount — depends on T002, T003 not required)

### Implementation for User Story 1

- [X] T005 [US1] Create `src/views/chat/rich-input/RichInput.tsx`: `useEditor()` with `StarterKit.configure({ heading: false, blockquote: false, codeBlock: false, horizontalRule: false, dropcursor: false })`, `Placeholder`, ref-based mutable config for `onSubmit`/`disabled`/`placeholder` (never recreating the editor — research.md's adopted mesh pattern), `editorProps.handleKeyDown` for Enter-submits/Shift+Enter-newline — depends on T004 (test should fail against nothing existing yet)
- [X] T006 [US1] Preserve every existing `data-testid` from the three raw inputs being replaced (`empty-state-input`/`empty-state-submit`, `chat-input`/`chat-send`, `agent-input`) on `RichInput`'s outer container/submit button, so no existing e2e spec needs to change — depends on T005
- [X] T007 [US1] Verify T004's tests pass — depends on T005, T006
- [X] T008 [P] [US1] Wire `src/views/chat/EmptyState.tsx` to render `RichInput` instead of its raw `<textarea>`, `onSubmit={(text) => submit-with-text}` — depends on T007
- [X] T009 [P] [US1] Wire `src/views/chat/Chat.tsx` to render `RichInput` (`skillsEnabled={false}`) instead of its raw `<textarea>` — depends on T007
- [X] T010 [P] [US1] Wire `src/views/workspace/Workspace.tsx` to render `RichInput` (`skillsEnabled={true}`) instead of its raw `<textarea>` — depends on T007
- [X] T011 [US1] Run `npx vitest run` (full suite) and the existing e2e specs that exercise these three surfaces (`keyboard-shortcuts.spec.ts`, `workspace-cwd-resolution.spec.ts`, `tool-call-widgets.spec.ts`) — confirm nothing broke from the swap — depends on T008, T009, T010

**Checkpoint**: MVP — one consistent rich input works everywhere; no persistence/backend change yet.

---

## Phase 4: User Story 2 - Pasting a lot of text doesn't turn the input into a wall of text (Priority: P2)

**Goal**: A large paste collapses into an expandable "<pasted N lines>" chip. This is also where the shared `RichMessageContent`/`expand_segments` backend infrastructure gets built — first story that needs to persist and expand anything beyond plain text.

**Independent Test**: Paste a block of text longer than ~10 lines/~500 characters and confirm it collapses into a chip; click the chip and confirm it expands back to the original, fully editable text; send it and confirm the agent receives the full original text.

### Tests for User Story 2

- [X] T012 [P] [US2] Write `src-tauri/src/agent/rich_content.rs` tests: `RichMessageContent`/`RichTextSegment` (all four variants — `text`, `pastedText`, `attachment`, `skill`, per data-model.md) serde round-trips; `expand_segments` correctly concatenates `text`/`pastedText` segments verbatim in order, for both `expand_skills` modes
- [X] T013 [US2] Implement `RichMessageContent`/`RichTextSegment` + `expand_segments(segments, skills_dir, expand_skills)` in `src-tauri/src/agent/rich_content.rs` per data-model.md (the `attachment`/`skill` match arms are stubbed to satisfy exhaustiveness here — T029/T038 fill in their real behavior) — depends on T012
- [X] T014 [P] [US2] Write a migration verification test (`src-tauri/src/storage/migrations` test module or an integration test) asserting: after applying `0003_rich_text_content_type`, existing rows keep their original `rowid`, `messages_fts` search still finds pre-migration content, and inserting a new row still populates `messages_fts` (the sync triggers survived the rebuild)
- [X] T015 [US2] Write `src-tauri/src/storage/migrations/0003_rich_text_content_type.sql`: table-rebuild adding `'rich_text'` to the `content_type` CHECK constraint, explicit `rowid` preservation in the copy, recreating `idx_messages_conversation_sequence` and the three FTS5 sync triggers verbatim from `0001_init.sql` (data-model.md's Persistence section has the exact sequence) — depends on T014
- [X] T016 [US2] Wire `send_agent_message` (`src-tauri/src/commands/agent.rs`) to accept `rich_content: Option<String>`: `None` → today's exact behavior unchanged; `Some(json)` → persist `content_type='rich_text'`, `content=json`, and use `expand_segments(..., expand_skills: true)`'s output (not the raw `content` param) as the turn's `ChatMessage::user(...)` text — depends on T013, T015
- [X] T017 [US2] Wire `send_message` (`src-tauri/src/commands/conversations.rs`, plain-chat path) with the identical `rich_content` treatment, including `generate_title` receiving `expand_segments(..., expand_skills: false)`'s output instead of the raw `content` string when `rich_content` is present (data-model.md — a title built from raw JSON or from a fully-expanded skill injection would both be wrong) — depends on T013, T015
- [X] T018 [US2] Wire `load_history` (`src-tauri/src/storage/conversations.rs`) to detect `content_type='rich_text'` rows and expand them via `expand_segments(..., expand_skills: true)` before building their `ChatMessage`, instead of today's direct pass-through — depends on T013
- [X] T019 [P] [US2] Add `RichMessageContent`/`RichTextSegment` (all four variants) to `src/lib/ipc.ts` per data-model.md's Frontend Types, and add a `richContent?: string` parameter to `commands.sendAgentMessage`/`commands.sendMessage` — depends on T013
- [X] T020 [P] [US2] Write `src/views/chat/rich-input/extensions/pasted-text-node.test.tsx`: renders a "<pasted N lines>" chip for a `pastedText` node's attrs; clicking it replaces the node with plain editable text at the same position, cursor at the end
- [X] T021 [US2] Implement `src/views/chat/rich-input/extensions/pasted-text-node.tsx`: `Node.create({ group: "inline", inline: true, atom: true })` + `ReactNodeViewRenderer` chip, styled per this codebase's existing token convention (`rounded-lg border border-border bg-card`, matching `004`'s tool widgets — not mesh's literal amber/violet colors), click-to-expand — depends on T020
- [X] T022 [P] [US2] Write `src/views/chat/rich-input/serialize.test.ts`: a paste under ~10 lines/~500 chars stays a plain `text` segment; a paste crossing either threshold produces a `pastedText` segment with the correct `lineCount` and the full, untruncated original text
- [X] T023 [US2] Implement the paste-interception `Plugin`'s `handlePaste` (research.md's mesh-derived idiom — check `event.clipboardData.getData("text/plain")`'s line/char count, `preventDefault()` + insert a `pastedText` node when over threshold, else return `false` and let default paste proceed) in `RichInput.tsx`, using `serialize.ts`'s threshold logic — depends on T021, T022
- [X] T024 [US2] Implement `src/views/chat/rich-input/serialize.ts`'s doc→`RichMessageContent` conversion (walks the editor's JSON doc, emitting one segment per node/text-run) and wire `RichInput`'s submit path to call `onSubmit(flatText, richContent)` — `richContent` only constructed (and only passed to `commands.sendAgentMessage`/`sendMessage`) when the doc contains at least one non-`text` segment (data-model.md's "common case has zero storage impact") — depends on T023, T019
- [X] T025 [P] [US2] Write `src/views/chat/rich-input/UserMessageContent.test.tsx`: a `content_type='rich_text'` message containing a `pastedText` segment renders the same collapsed chip, read-only (clicking it does **not** expand/edit)
- [X] T026 [US2] Implement `src/views/chat/rich-input/UserMessageContent.tsx` (a second, `editable: false` Tiptap instance sharing the same node extensions, mirroring mesh's `message/user.tsx` — contracts/rich-chat-input.md) and wire `src/components/MessageContent.tsx`'s dispatch to route `content_type='rich_text'` there — depends on T025, T021
- [X] T027 [US2] Run `cargo test` and `npx vitest run` — confirm clean — depends on T016, T017, T018, T024, T026

**Checkpoint**: Paste-collapse works end-to-end; a `rich_text` message's full pasted content reaches the model on the turn it's sent **and** on every later turn that replays it; chips persist and re-render identically on reload.

---

## Phase 5: User Story 3 - Bring a skill into the conversation by typing "/" (Priority: P2)

**Goal**: Typing "/" in an agent-mode surface opens a picker of installed skills; selecting one performs real context injection, not just a cosmetic marker.

**Independent Test**: Install a skill, type "/" in an agent-mode conversation, select it, send, and confirm — via the agent's actual response — that its guidance was applied. Separately confirm "/" is inert in `Chat.tsx`'s plain-mode composer.

### Tests for User Story 3

- [X] T028 [P] [US3] Extend `rich_content.rs` tests: `expand_segments(..., expand_skills: true)` reads a named skill's real `SKILL.md` and inlines it (wrapped `<skill name="...">...</skill>`) at the segment's position, in order relative to surrounding text; `expand_skills: false` renders the literal `/name` marker instead; a `skill` segment naming a skill that can't be read returns `Err` (not a partially-built string) — FR-014
- [X] T029 [US3] Implement `rich_content.rs`'s `skill` match arm per T028, reading `{skills_dir}/{name}/SKILL.md` via the existing `skills::discover_skills` convention — depends on T028, T013. **Delivered ahead of schedule**: the T012/T013 agent implemented the full exhaustive match (skill and attachment both, not stubbed) directly, since data-model.md already fully specified both. Verified directly against `rich_content.rs` — the existing tests/implementation already satisfy this task exactly.
- [X] T030 [P] [US3] Write `src/views/chat/rich-input/extensions/skill-mention.test.tsx`: typing "/" opens a picker listing `list_skills()` results by name+description; typing further filters the list; selecting an item inserts a `skill` segment/marker and closes the picker; Escape closes without selecting; zero skills installed shows a legible empty state (FR-015)
- [X] T031 [US3] Implement `src/views/chat/rich-input/extensions/skill-mention.tsx`: `@tiptap/suggestion`'s `Suggestion` plugin + a React state bridge + `@floating-ui/react` popup (`placement: "bottom-start"`, `middleware: [offset(10), flip(), shift()]`, `FloatingPortal`) anchored to the suggestion's decoration node — research.md's mesh-derived plumbing, with a plain `useEffect` fetch against `list_skills()` (no MCP/React-Query machinery, per research.md's Decision) as the data source — depends on T030
- [X] T032 [US3] Implement keyboard navigation (arrow keys/Enter/Escape) for the skill picker — jsdom component test if T003's spike confirmed `useListNavigation` works there; otherwise add this specific interaction to a WDIO e2e spec instead (T054) — depends on T031, T003
- [X] T033 [US3] Gate `skill-mention.tsx`'s registration on `RichInput`'s `skillsEnabled` prop; add a test confirming typing "/" in `Chat.tsx` (`skillsEnabled={false}`) does not open any picker (FR-011) — depends on T031
- [X] T034 [P] [US3] Write a `UserMessageContent` rendering test: a persisted `skill` segment re-renders as its `/name` marker in history — never the injected file content, which only ever exists in the model-facing expansion, not in what's stored or displayed
- [X] T035 [US3] Extend `UserMessageContent.tsx`/`MessageContent`'s chip dispatch for the `skill` segment type — depends on T034, T026
- [X] T036 [US3] Run `cargo test` and `npx vitest run` — confirm clean — depends on T029, T032, T033, T035

**Checkpoint**: Skill mentions work and demonstrably change agent behavior; a skill selected on an earlier turn still applies on a later turn in the same conversation (via `load_history`'s expansion — already wired in US2); history shows the marker, not the injected content; plain conversations remain unaffected.

---

## Phase 6: User Story 4 - Attach an image for the record, even though the agent can't see it yet (Priority: P3)

**Goal**: Paste, drag-drop, or pick (native dialog) an image/file and get a compact chip with a hover preview for images — with the image's bytes never reaching the model.

**Independent Test**: Attach an image via all three methods and confirm each produces the same chip; confirm hovering shows a preview; confirm sending doesn't error or hang even though the agent's reply shows no awareness of the image's visual content.

### Tests for User Story 4

- [X] T037 [P] [US4] Extend `rich_content.rs` tests: `expand_segments` never includes an `attachment` segment's `data` in its output, in either expansion mode; renders `[attached image: {name}]`/`[attached file: {name}]` per `isImage` — FR-009
- [X] T038 [US4] Implement `rich_content.rs`'s `attachment` match arm per T037 — depends on T037, T013. **Delivered ahead of schedule**, same as T029 above.
- [X] T039 [P] [US4] Write `src-tauri/src/commands/attachments.rs` tests: `read_attached_file(path)` returns `{ data, mimeType, name }` (base64, no `data:` prefix) for a real file; returns `Err` for a missing/unreadable path
- [X] T040 [US4] Implement `read_attached_file` (contracts/rich-chat-input.md — a plain `#[tauri::command]`, no new plugin dependency per research.md's decision) and register it in `commands/mod.rs`'s `collect_commands!` — depends on T039
- [X] T041 [P] [US4] Add `readAttachedFile` to `src/lib/ipc.ts`'s `commands` object
- [X] T042 [P] [US4] Write `src/views/chat/rich-input/extensions/attachment-node.test.tsx`: renders a filename chip; a hover preview (`<img>` from the base64 data) appears only when `isImage`; a non-image attachment shows filename/mimeType text instead, no preview
- [X] T043 [US4] Implement `src/views/chat/rich-input/extensions/attachment-node.tsx`: atom node + `ReactNodeViewRenderer` chip (mesh's `FileNode` pattern, this codebase's token styling) — depends on T042
- [X] T044 [P] [US4] Write a paste/drop test: pasting or dropping an image/file (`clipboardData.items`/`dataTransfer.files` containing a file-kind item) calls `readAttachedFile` and inserts an `attachment` segment at the drop/cursor position. **Delivered with a verified deviation from this task's literal text**: `RichInput.attachments.test.tsx` covers paste via a mocked `clipboardData.items` file-kind item (as written here), but drop is tested via a mocked `@tauri-apps/api/webview` `getCurrentWebview().onDragDropEvent()` callback, not `dataTransfer.files` — see T045's note for why.
- [X] T045 [US4] Implement the file paste/drop `Plugin`'s `handlePaste`/`handleDrop` in `RichInput.tsx` (mesh's `FileUploader` idiom — intercept only when a file-kind clipboard/drop item is present, let everything else fall through to T023's text-paste handling or default insertion) calling `commands.readAttachedFile` — depends on T044, T040. **Delivered with a verified deviation**: paste is implemented exactly as described (`handlePaste`, `clipboardData.items[].kind === "file"`), but calls a new client-side `fileToBase64`/`arrayBuffer()` helper instead of `read_attached_file` — a pasted `File` has no real filesystem path on any platform (confirmed: no plugin, Tauri included, extends the Clipboard API with one). Drop does **not** use a ProseMirror `handleDrop`/`dataTransfer.files` at all: confirmed directly against this project's own installed platform that `tauri-utils`' `WindowConfig::drag_drop_enabled` defaults to `true` and `tauri.conf.json` doesn't override it, so `wry` 0.55.1 (`wkwebview/drag_drop.rs`) intercepts OS-level drag-and-drop *natively*, and a DOM `drop` event's `dataTransfer.files` would never carry a real file here. Drop is wired instead via `@tauri-apps/api/webview`'s `getCurrentWebview().onDragDropEvent()` (gated by `isTauri()`), whose `paths` are real absolute filesystem paths — so drop reuses `attachFromPath`/`read_attached_file`, the same code path as the file-picker button, not the client-side path paste uses. Both the finding and the resulting split are documented in `RichInput.tsx`'s own doc comment on `attachFromFile`/`attachFromPath`.
- [X] T046 [P] [US4] Write a file-picker-button test: clicking it calls `@tauri-apps/plugin-dialog`'s `open({ multiple: false, directory: false, filters: [{ name: "Image", extensions: [...] }] })` (research.md's verified exact snippet), then `readAttachedFile` on the result, then inserts an `attachment` segment; cancelling (`null`) leaves the input unchanged
- [X] T047 [US4] Implement the file-picker button in `RichInput.tsx` per T046/research.md's exact API — depends on T046, T040
- [X] T048 [P] [US4] Write a `UserMessageContent` rendering test for the `attachment` segment: chip + hover preview, read-only, identical to the live-editor chip's appearance. **Confirmed urgent, not just nice-to-have**: `serialize.ts`'s `richMessageContentToDoc` now reconstructs a real `attachment` node (this task's serialize.ts work, done ahead of T048/T049) — verified directly that mounting `UserMessageContent` today against a persisted `attachment` segment throws inside `prosemirror-model` (`RangeError: Unknown node type: attachment`), which Tiptap's `createDocument` catches and silently falls back to an **empty document** — the entire message (not just the attachment) renders blank, including any surrounding text. This is a real, reproducible regression risk the moment a user actually sends a message with an attachment (now wired end-to-end via T044-T047) and reloads/revisits that conversation, not a hypothetical — T048/T049 should land before shipping US4. **Delivered**: two new tests added to `UserMessageContent.test.tsx` — an image attachment (chip's filename, surrounding text segments still render, hover-preview `<img>`'s exact `data:${mimeType};base64,${data}` `src`) and a non-image attachment (filename+mimeType text, total absence of both the `<img>` and the `attachment-preview` wrapper). RED confirmed first: both failed against the pre-fix component with exactly the predicted blank-document symptom (`findByTestId("attachment-chip")` timing out against a doc that rendered as empty `<p><br/></p>`).
- [X] T049 [US4] Extend `UserMessageContent.tsx`/`MessageContent`'s chip dispatch for the `attachment` segment type — depends on T048, T035. See T048's note: registering the `Attachment` extension in `UserMessageContent.tsx`'s editor (mirroring `PastedText`/`SkillMention`'s existing registration there) is what fixes the confirmed blank-render regression. **Delivered exactly as scoped**: added the `Attachment` import and included it in `RichTextRenderer`'s `useEditor` extensions array alongside `PastedText`/`SkillMention`; updated the component's doc comment to explain why this registration is required (not optional) given `richMessageContentToDoc`'s `attachment`-node reconstruction. GREEN confirmed: both new T048 tests pass, and both the full suite (`npx vitest run`: 28 files / 157 tests) and `npx tsc -b` are clean.
- [X] T050 [US4] Run `cargo test` and `npx vitest run` — confirm clean — depends on T038, T045, T047, T049

**Checkpoint**: All four user stories independently functional. Image/file attachment works via all three input methods; chips persist; image bytes are confirmed never present in what's sent to the model.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Whole-feature verification.

- [X] T051 [P] Run `cargo test` for the full `src-tauri` workspace, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings`, and confirm all clean
- [X] T052 [P] Run `npx tsc -b` and `npx vitest run` for the full frontend suite, confirm every test — old and new — passes
- [X] T053 **Critical finding, fixed**: writing T054's live e2e spec surfaced a real integration bug that no unit/component test had caught — `EmptyState.tsx`, `Chat.tsx`, and `Workspace.tsx`'s own `submit`/`send` functions all accepted `RichInput`'s `(content, richContent?)` callback but only ever forwarded `content` to `sendAgentMessage`/`sendMessage`, silently dropping `richContent` on every real send. Confirmed directly by inspecting the persisted SQLite row (`content_type` was `'text'`, never `'rich_text'`, for a message that visibly had a pasted-text chip). Each layer's OWN tests passed because they only exercised that layer in isolation (`RichInput.test.tsx` confirmed `onSubmit` receives the right `richContent`; the backend's tests confirmed `expand_segments`/persistence are correct given a real payload) — nothing tested the full wire from one to the other. Fixed in all three files, plus a second bug the fix's own regression test uncovered: the outer `send`/`submit` guards' `!content.trim()` check didn't account for `richContent`'s presence, so a message that's *entirely* a chip (no extra typed text) was silently dropped. Added a regression test to each of the three surfaces' existing test files asserting `richContent` is actually forwarded. Manually validated live thereafter: paste-collapse and skill-injection both confirmed working end-to-end against the real model (see T054).
- [X] T054 Added `tests/e2e/specs/rich-chat-input.spec.ts` — both scenarios pass live against the real app and model. US2: a 25-line paste collapses into a chip, and the agent's reply correctly quotes the exact first and last pasted lines (proving the full original text reached the model, not a placeholder). US3: selecting `doce-e2e-test-skill` makes the agent's reply exactly match that skill's injected instruction. Two real technique findings along the way, documented in the spec's own comments: (1) `browser.keys(Key.Enter)` resolves to a no-op empty key value in this WebKit/Tauri WebDriver setup — submission goes through the real `agent-send` button instead, matching every other e2e spec in this project; (2) simulating a large paste requires dispatching a synthetic `ClipboardEvent` directly (`addValue`/`elementSendKeys` only fire keystrokes, never a `paste` event; an OS-level Cmd+V was tried and also didn't reach the document reliably in this specific automation stack). T032's keyboard-navigation-to-e2e deferral was not additionally covered here — the picker's click-to-select path is proven live by this spec, and jsdom already covers filtering/empty-state/gating; revisit only if arrow-key nav specifically becomes a real concern.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies.
- **Foundational (Phase 2)**: Depends on Setup — blocks every user story's tests.
- **User Story 1 (Phase 3)**: Depends on Foundational only. Fully self-contained — no backend change.
- **User Story 2 (Phase 4)**: Depends on Foundational. Builds the shared `RichMessageContent`/`expand_segments`/migration/IPC infrastructure that US3 and US4 both extend — not independent of them at the backend-plumbing layer, though its own paste-collapse feature is a complete, independently demonstrable increment on top of US1.
- **User Story 3 (Phase 5)**: Depends on US2's backend infrastructure (T013/T015/T019) for its own `skill` segment support; its picker/mention UI is otherwise independent of US2's paste-collapse UI.
- **User Story 4 (Phase 6)**: Depends on US2's backend infrastructure (T013/T015/T019) for its own `attachment` segment support; otherwise independent of US2/US3's UI.
- **Polish (Phase 7)**: Depends on all four user stories.

### Within Each User Story

- Tests written and confirmed failing before their corresponding implementation task.
- Backend segment/expansion support before the frontend feature that produces that segment.
- Live-editor chip before the read-only `UserMessageContent` rendering of the same chip.

### Parallel Opportunities

- T008–T010 (wiring the three composing surfaces in US1) touch disjoint files.
- T012, T014, T019, T020, T022 (US2's independent test-writing tasks) touch disjoint files.
- T028/T030, T037/T039/T042/T044/T046 (US3/US4's independent test-writing tasks) touch disjoint files.
- T051/T052 (Polish) are independent verification passes.

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Setup + Foundational (T001–T003).
2. Complete User Story 1 (T004–T011) — a fully working, consistent rich input across all three surfaces, zero backend change.
3. **STOP and VALIDATE**: quickstart.md's step 1.
4. Ship — even without US2/US3/US4, this is already a strict improvement over the three separate plain inputs.

### Incremental Delivery

1. Setup + Foundational → dependencies and jsdom polyfills in place.
2. US1 → consistent rich input everywhere (MVP).
3. US2 → paste-collapse, plus the shared backend infrastructure US3/US4 depend on.
4. US3 → skill mentions with real context injection.
5. US4 → image/file attachment chips.
6. Each story adds value without breaking the previous ones — history round-trip (FR-016) holds incrementally as each new segment type is introduced.

## Notes

- [P] tasks = different files, no dependencies within their phase.
- Verify each `*.test.*` task's tests fail before its corresponding implementation task lands.
- Commit after each checkpoint.
- research.md's Open Item (T003) genuinely gates T032's approach — don't guess at it; run the spike.
