# Research: Rich Chat Input

## Reference implementation survey (`~/code/mesh/apps/mesh`)

Conducted as direct, full-file research against the reference implementation before this spec was drafted (see spec.md's Input). Summary of what's directly reusable vs. what needed fresh design:

### Decision: Editor architecture — adopt mesh's pattern wholesale

**Decision**: `useEditor()` lives in one provider component (`RichInput.tsx`), exposing the editor via `@tiptap/react`'s `EditorContext` so sibling utility components (paste/attachment handling, skill-mention popup) read it via `useCurrentEditor()` rather than prop drilling. Every mutable config value that can change after mount (`onSubmit`, `disabled`, `placeholder`, `skillsEnabled`) is stashed in a `useRef` and mirrored via a dedicated `useEffect`, so `useEditor()` is called exactly once per composing surface's lifetime — never recreated on re-render. `disabled` toggles via `editor.setEditable(!disabled)`, not remount. External→internal content sync (when a surface needs to reset the editor, e.g. after a successful send) diffs before calling `editor.commands.setContent(...)`.

**Rationale**: This is the single pattern responsible for the input feeling responsive and never losing cursor position/undo history/an open skill picker mid-interaction — exactly the failure mode a naive `useEditor({...})` call (recreated every render because its `extensions` array is a fresh literal each time) would produce. Proven in production in `~/code/mesh`; no reason to redesign it.

**Alternatives considered**: A fully-controlled `content`/`onChange` pattern (React owns the doc, editor is a "dumb" renderer) — rejected because ProseMirror's own internal state (selection, decorations, plugin state for the suggestion popup) can't be losslessly round-tripped through a plain JSON prop on every keystroke without exactly the re-creation cost this pattern avoids.

### Decision: `StarterKit` pared down, same as mesh

**Decision**: `StarterKit.configure({ heading: false, blockquote: false, codeBlock: false, horizontalRule: false, dropcursor: false })` — a flat, single-purpose text field (paragraphs, marks, hard breaks, lists), not a general document editor.

**Rationale**: A chat composer is not a document editor; headings/blockquotes/code-blocks/hr inside a chat input would be actively confusing UI surface area with no corresponding requirement in spec.md.

### Decision: Atom-node modeling for every non-text chip

**Decision**: `pastedText`, `attachment`, and the skill mention all model as `Node.create({ group: "inline", inline: true, atom: true })` with a `ReactNodeViewRenderer` for the visual, following mesh's `FileNode`/`MentionNode` exactly. Deletion is a single backspace/select+delete (atomic), not character-by-character.

**Rationale**: Established, working pattern for exactly this kind of "opaque inline chip" UI; no reason to invent a different modeling approach for the two new chip types (`pastedText` has no mesh precedent, but the *modeling technique* — atom node + NodeView — transfers directly).

### Decision: Skill-mention popup — same suggestion/positioning plumbing as mesh's slash/at mentions, none of the MCP-specific data source

**Decision**: `@tiptap/suggestion`'s `Suggestion({...})` plugin factory drives lifecycle (`onStart`/`onUpdate`/`onKeyDown`/`onExit`); a React `useReducer`-backed bridge (mirroring mesh's `useMentionState`) turns plugin callbacks into React state; the popup itself renders through `@floating-ui/react` (`useFloating` with `placement: "bottom-start"`, `middleware: [offset(10), flip(), shift()]`, `whileElementsMounted: autoUpdate`, rendered via `FloatingPortal`) anchored to the `Suggestion` plugin's decoration DOM node. Arrow-key/Enter/Escape navigation via a native `keydown` listener attached directly to `editor.view.dom` with `capture: true` (not routed through Tiptap's keymap), exactly matching mesh's `useMenuNavigation`.

**Data source**: the existing `list_skills` command (`SkillSummary[]` — `name`, `description`) — no MCP prompt/resource concept exists in doce, so mesh's two-level category→agents/resources drill-in and `useSuspenseQuery`/React-Query machinery aren't adopted; a plain `useEffect` fetch (doce has no React Query dependency today, and one flat list from a local, fast Tauri command doesn't need Suspense/caching machinery to feel instant) populates the picker's items, filtered client-side by the typed query against `name`/`description`.

**Rationale**: The popup positioning/keyboard-nav plumbing is genuinely reusable infrastructure independent of what data it's showing; the data-fetching machinery (`useSuspenseQuery`, MCP prompt/resource types) is specific to mesh's actual data source and has no analog here — reusing it would mean adding a React Query dependency for one flat, fast, local list.

**Alternatives considered**: `tippy.js` for popup positioning — mesh itself moved away from this (confirmed: not a dependency in mesh's `package.json`); no reason to introduce it fresh here when Floating UI is the maintained, currently-used choice.

### Decision: No character/length limit on the input itself

**Decision**: Matches mesh — no `CharacterCount` extension, no `maxLength`. FR-017.

**Rationale**: Per spec.md's Assumptions, length/context management is a model/token-budget concern (already handled at the inference layer, including this session's earlier prefill-chunking fix for prompts exceeding the batch capacity), not an input-UI concern — an artificial UI-level cap would be redundant with, and could easily disagree with, the actual constraint.

## Novel design (no mesh precedent)

### Decision: Paste-collapse via `editorProps.handlePaste`, mirroring mesh's file-paste interception idiom

**Decision**: A ProseMirror `Plugin` (registered the same way mesh's `FileUploader` registers its `fileDropHandler` plugin) implements `props.handlePaste(view, event)`: read `event.clipboardData.getData("text/plain")`; if it crosses ~10 lines (`.split("\n").length > 10`) or ~500 characters, call `event.preventDefault()`, insert a `pastedText` node at the current selection with the full text + computed `lineCount`, and return `true` (handled) instead of letting ProseMirror's default paste pipeline insert raw text. Below the threshold, return `false` and let default paste handling proceed exactly as today — a short paste is indistinguishable from typing.

**Rationale**: This is the direct extension of the exact idiom mesh already uses for file paste (`handlePaste` checking `clipboardData.items[].kind === "file"`) to a text-based condition instead of a MIME-kind condition — same interception point, same "let default paste through when it doesn't apply" discipline, so plain short pastes are never at risk of behaving differently.

**Alternatives considered**: Tiptap's `Extension`-level `addPasteRules` (regex/text-pattern-triggered auto-replacement, e.g. how a Markdown-shortcuts extension might turn `**x**` into bold) — rejected because paste rules match *after* text is already inserted into the document and operate on committed text patterns, not on the raw `ClipboardEvent` itself; intercepting *before* insertion (so the raw text never touches the document, avoiding an insert-then-immediately-replace flash) requires the `handlePaste` plugin-prop approach mesh already uses for files.

### Decision: Expandable pasted-text chip re-uses the node's own text, no separate viewer

**Decision**: Clicking a `pastedText` chip transforms it in place — the atom node is replaced with a plain-text node containing the same `text`, at the same document position, putting the cursor at the end of the restored text. Not a modal/popover viewer.

**Rationale**: FR-004/spec.md's Assumptions are explicit that expansion is "fully editable in place," matching the everyday expectation that clicking a "show more" affordance in a text field reveals editable text, not a read-only preview requiring a second interaction to actually edit.

### Decision: Skills wired into the agent loop for the first time — resolved at send/replay time, not selection time

**Decision**: A `skill` segment stores only the skill's `name`; the corresponding `SKILL.md` content is read fresh, backend-side, both when a turn is first sent and every time an earlier turn containing a `skill` segment is replayed into a later turn's history (`load_history`). See data-model.md's Model-Text Expansion for the full mechanism.

**Rationale**: `list_skills`/`skills::discover_skills` already existed but had zero connection to `send_agent_message`/`SYSTEM_PROMPT` before this feature (confirmed directly against the code before drafting spec.md) — this is genuinely new wiring, not an extension of an existing integration point. Resolving fresh (vs. snapshotting content at selection time, which is what mesh does for its prompt mentions) was chosen because spec.md's FR-014 explicitly commits to "can no longer be read **at send time**," and because a skill is a local file the user directly controls — using its current content on every use is more useful than a silently stale snapshot, and doce has no versioning/pinning concept for skills that would make snapshotting meaningfully safer.

### Decision: Image bytes never enter model-facing text, for either turn-time send or history replay

**Decision**: `expand_segments`'s `attachment` handling is unconditional — an image's `data` (base64) is never included in the string built for inference, in either the send-time or `load_history`-replay path. Only `[attached image: {name}]` (or `[attached file: {name}]` for non-image attachments) appears.

**Rationale**: Confirmed via interview — the currently-supported local model (`qwen3-4b-instruct-2507-q4_k_m`) is text-only, and this session's earlier fix for the model's 2048-token context window (a real, already-encountered limit) makes "accidentally include tens of thousands of base64 tokens the model can't even use" a genuine risk worth designing out entirely, not a hypothetical.

## Dependencies

| Package | Version (pinned per `~/code/mesh/apps/mesh/package.json`) | Purpose |
|---|---|---|
| `@tiptap/core` | `3.20.2` | Editor core |
| `@tiptap/react` | `3.20.2` | React bindings (`useEditor`, `EditorContent`, `ReactNodeViewRenderer`, `EditorContext`) |
| `@tiptap/starter-kit` | `3.20.2` | Base document schema (paragraphs, marks, hard breaks, lists) |
| `@tiptap/suggestion` | `3.20.2` | `Suggestion` plugin factory (skill-mention popup) |
| `@tiptap/pm` | `3.20.2` | Re-exported ProseMirror primitives Tiptap extensions build on (`Plugin`, `PluginKey` for the paste-handler and suggestion plugins) |
| `@floating-ui/react` | `^0.27.16` | Skill-mention popup positioning |

No `tippy.js`, no direct `prosemirror-*` dependency (pulled transitively via `@tiptap/pm`), no `uuid` package (`crypto.randomUUID()` covers segment IDs, matching mesh's own choice) — all confirmed absent from mesh's dependency tree and not needed here either.

`@tauri-apps/plugin-dialog` is already installed (`006-chat-empty-state`) — reused for the native image-picker button, not a new dependency.

## Decision: Testing strategy — three tiers, matching mesh's own (undocumented but real) choice

**Decision**: Verified empirically against doce's exact pinned stack (vitest 4.1.9, jsdom 29.1.1, React 19.2.7, `@testing-library/react` 16.3.2, `@tiptap/react` 3.27.1, `@floating-ui/react` 0.27.19) via a real spike, not secondhand reports:

1. **Pure logic, no DOM** — `serialize.ts` (editor doc ↔ `RichMessageContent`), the `expand_segments`-equivalent preview logic if any exists client-side, and any other JSON-in/JSON-out function: plain Vitest unit tests, zero editor/DOM involvement. This is the *only* kind of Tiptap-adjacent test `~/code/mesh` itself has (`build-improve-prompt-doc.test.ts` — asserts on plain doc-JSON shape and a pure `derivePartsFromTiptapDoc` function; never imports `@tiptap/react`).
2. **Component-level jsdom tests, for structural/rendering correctness only** — mounting a minimal `useEditor` with just the extension under test (confirmed working: initial render, mounting a `ReactNodeViewRenderer` chip node, simple linear `userEvent.type()`/`userEvent.keyboard()` append-typing, `@floating-ui/react` popup mounting/positioning). **Do not** assert on real pixel geometry (jsdom's `getBoundingClientRect` is permanently zeroed — no layout engine, by jsdom's own design, not a bug) or simulate Home/End/Arrow-key caret repositioning inside the contenteditable via `userEvent` (confirmed to throw — `@testing-library/user-event` 14.6.1's `setSelectionRange`-for-contenteditable path is unimplemented; drive selection via the editor's own API, `editor.commands.setTextSelection(pos)`, instead).
3. **Real interaction, positioning, and caret navigation** — extend doce's existing WDIO e2e suite (`tests/e2e`, the same infrastructure `004-tool-call-widgets`/`006-chat-empty-state` already added specs to) with a chat-input spec driving the real Tauri webview via actual keystrokes. This is the exact tier mesh itself relies on for its own Tiptap input (`packages/e2e/tests/chat-input-draft.spec.ts` via Playwright, with an explicit code comment: *"Tiptap is contenteditable, so `page.fill()` is unsupported — we have to use real keystrokes"*) — mesh's engineers made the identical three-tier call, just with Playwright where doce has WDIO.

**Setup required**: extend `src/test/setup.ts` (same commented-polyfill convention already used there for `HTMLDialogElement.showModal`/`close`) with three polyfills, each individually confirmed necessary on doce's pinned jsdom 29.1.1 — not speculative: `Range.prototype.getBoundingClientRect`/`getClientRects` (ProseMirror's `EditorView` calls these on effectively every transaction via `coordsAtPos`/`scrollToSelection`; unmocked, they throw an uncaught `TypeError` that doesn't fail an individual `expect()` but *does* flip `vitest run`'s process exit code to 1 — silently poisoning an otherwise-green suite), `document.elementFromPoint` (thrown on mousedown handling), and a no-op `global.ResizeObserver` (not strictly required by the exact extensions tested, but cheap and likely needed once real chip node views are added — jsdom 29 has none of `ResizeObserver`/`IntersectionObserver`/`matchMedia` natively).

**Rationale**: Forcing tier-3-shaped assertions (real caret navigation, real popup geometry) into jsdom would be fighting a documented, permanent jsdom limitation, not a temporary gap — and mesh, the reference implementation this feature is explicitly modeled on, already made and validated this exact split in production. Matching it is lower-risk than inventing a different split.

**Alternatives considered**: `happy-dom` instead of jsdom — rejected; doce's existing suite is jsdom-based project-wide, and switching test environments for one feature would be a much larger, unrelated change; community reports of Tiptap issues under `happy-dom` specifically (not reproduced against doce's actual jsdom setup) are a mild point in jsdom's favor anyway.

**Resolved (T003 spike)**: `@floating-ui/react`'s `useListNavigation` was spiked directly (a minimal `useFloating`+`useListNavigation`+`useInteractions` list, driven via `userEvent.keyboard("{ArrowDown}")`). The first `ArrowDown` correctly advanced `activeIndex` from `null` to `0`, but a second `ArrowDown` immediately after did not advance it to `1` — reproduced consistently, and unaffected by an explicit `await act(async () => {})` flush between key presses (ruling out the usual async-positioning-not-flushed cause). Whether this is a genuine jsdom incompatibility or a subtlety in the spike's own wiring wasn't fully isolated — but the practical, decision-relevant finding is the same either way: sequential arrow-key navigation is not reliably testable in this jsdom setup. **Decision: skill-picker keyboard navigation (T032) targets tier 3 (WDIO e2e), not tier 2 (jsdom)**. Tier 2 remains fine for everything else about the picker (opens on "/", lists/filters items, selection inserts the right segment) — only the specific "press arrow twice, confirm the second item is highlighted" interaction moves to e2e.

## Decision: Native image picker — `@tauri-apps/plugin-dialog`'s existing `open()`, plus one new custom command to read the bytes

**Decision**: Installed version is `2.7.1` (both JS and the Rust crate, confirmed against `package.json`/`Cargo.toml`). Its `open()` signature (verified against the actual installed `.d.ts`, not general Tauri familiarity) supports exactly what's needed:

```ts
import { open } from "@tauri-apps/plugin-dialog";

const selected = await open({
  multiple: false,
  directory: false,
  filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
});
// selected: string | null — an absolute path, or null if the user cancelled
```

— directly mirroring `FolderPicker.tsx`'s existing `browse()` (`open({ directory: true })`), just with `directory: false` and an extension filter. No capability-file change needed: `dialog:allow-open` (already granted transitively by the existing `dialog:default` entry in `capabilities/default.json`) is the single permission gating `open()` regardless of directory/file/filter mode — confirmed against the installed plugin's generated permission schema.

`open()` resolves to a **path only** — it does not read file bytes, and `@tauri-apps/plugin-fs` (which would provide a `readFile` for that) is **not installed** in this project (absent from `package.json`, `node_modules/@tauri-apps`, and `Cargo.toml`). Two ways to get from "selected path" to "base64 bytes for the `attachment` segment": add `@tauri-apps/plugin-fs` (a new dependency plus a new `fs:allow-read-file`-shaped capability entry), or add one small custom Tauri command that takes the path and returns base64 + a detected MIME type — reusing the project's existing hand-written-command convention (`ipc.ts`'s own comment: *"hand-written typed wrappers... this file is the pre-first-run bootstrap"*) instead of introducing a new plugin surface for a single, narrow read.

**Chosen**: the custom-command route (`read_attached_file(path: String) -> Result<{ data: String, mimeType: String, name: String }, String>`, `src-tauri/src/commands/`) — no new dependency, no new capability surface, and the command can enforce the same kind of size/type sanity-checking a dedicated attachment feature should have (rather than a generic fs-read permission that's more powerful than this feature actually needs).

**Rationale**: Consistent with the project's own established preference (every existing IPC surface in `ipc.ts` is a purpose-built command, not a raw plugin passthrough) and avoids the capability-surface expansion a general filesystem-read permission would introduce for what is, in this feature, a single narrow need (read one user-selected file's bytes for local rendering).

**Alternatives considered**: `@tauri-apps/plugin-fs` — rejected per the above; would work, but adds a dependency and a capability grant broader than this feature needs for a benefit (avoiding one small Rust command) that doesn't outweigh the cost.
