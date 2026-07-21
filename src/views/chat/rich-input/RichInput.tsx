import { useEffect, useRef, useState, type ReactNode } from "react";
import { EditorContent, useEditor, useEditorState, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { Placeholder } from "@tiptap/extension-placeholder";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowUp, Plus, Square, Target } from "lucide-react";
import { InputGroup, InputGroupAddon, InputGroupButton } from "@/components/ui/input-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/cn";
import { commands, type RichMessageContent } from "@/lib/ipc";
import PastedText from "./extensions/pasted-text-node";
import SkillMention, { isSkillMentionSuggestionActive } from "./extensions/skill-mention";
import Attachment, { type AttachmentAttrs } from "./extensions/attachment-node";
import {
  richMessageContentFromDoc,
  richMessageContentToDoc,
  shouldCollapsePastedText,
} from "./serialize";

// 009-rich-chat-input, User Story 4 (T044-T047): image/file attachment via
// paste, native OS drag-and-drop, and a file-picker button. A reasonable
// sanity cap on an attachment's size — matching mesh's own 10MB cap
// (research.md's mesh survey). Not enforced server-side (`read_attached_file`,
// T040, already shipped and out of this file's scope), so it's enforced
// here, against whichever of the two byte-acquisition paths below produced
// the attempted attachment.
const ATTACHMENT_MAX_BYTES = 10 * 1024 * 1024;

function isImageMimeType(mimeType: string): boolean {
  return mimeType.startsWith("image/");
}

/**
 * Decoded byte length of a base64 string (no `data:` prefix), computed
 * from the string's own length/padding rather than by actually decoding it
 * — cheap, and avoids materializing a second, decoded copy of a
 * potentially-multi-megabyte payload just to measure it.
 */
function base64ByteLength(base64: string): number {
  if (base64.length === 0) return 0;
  const padding = base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0;
  return Math.floor((base64.length * 3) / 4) - padding;
}

function attachmentSizeErrorMessage(name: string): string {
  return `"${name}" is larger than the 10MB attachment limit.`;
}

/**
 * Reads a browser `File`'s bytes directly, client-side, base64-encoded.
 * The *only* caller of this is the paste path (`attachFromFile`, used from
 * `handlePaste` below) — see that function's doc comment for why paste
 * can never go through `read_attached_file`'s real-path route the way the
 * file-picker/drag-drop flows do.
 */
async function fileToBase64(file: File): Promise<string> {
  const buffer = await file.arrayBuffer();
  const bytes = new Uint8Array(buffer);
  // Chunked rather than a single `String.fromCharCode(...bytes)`: spreading
  // a large (multi-MB) typed array as call arguments risks blowing the JS
  // engine's call-stack/argument-count limit.
  const CHUNK_SIZE = 0x8000;
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += CHUNK_SIZE) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + CHUNK_SIZE));
  }
  return btoa(binary);
}

/**
 * Every clipboard item with `kind === "file"`, resolved to its `File`
 * (mesh's own `FileUploader` idiom, research.md: `clipboardData.items[].kind
 * === "file"`), skipping any item `getAsFile()` couldn't resolve.
 */
function getFilesFromClipboard(clipboardData: DataTransfer | null): File[] {
  if (!clipboardData) return [];
  return Array.from(clipboardData.items)
    .filter((item) => item.kind === "file")
    .map((item) => item.getAsFile())
    .filter((file): file is File => file !== null);
}

export interface RichInputProps {
  /**
   * Mirrors the two-parameter shape the workspace send path accepts
   * (contracts/rich-chat-input.md), so each composing surface can forward
   * these straight into its own submit handler.
   * `richContent` is `undefined` whenever the doc is plain text (the common
   * case has zero storage impact — data-model.md), and populated (via
   * serialize.ts's `richMessageContentFromDoc`) once the doc contains at
   * least one non-text segment — `pastedText` (US2) or `skill` (US3);
   * `attachment` lands in US4.
   */
  onSubmit: (content: string, richContent?: RichMessageContent) => void;
  /**
   * Gates the "/" skill-mention picker (US3, FR-010/FR-011): the
   * `SkillMention` extension is only added to this instance's `extensions`
   * array when `true` (see the `useEditor()` call below for why this is
   * safe despite the editor never being recreated). `false` makes "/" fully
   * inert — no picker, no `commands.listSkills()` call.
   */
  skillsEnabled: boolean;
  /**
   * Toggled via editor.setEditable(), never by remounting/conditionally
   * rendering the editor — preserves in-progress composition (cursor
   * position, undo history) across a streaming/disabled transition, exactly
   * as the plain <textarea>'s existing `disabled` prop does today.
   */
  disabled: boolean;
  /** Per-surface placeholder text, passed straight to the Placeholder extension. */
  placeholder: string;
  /**
   * `data-testid` for the editable surface itself (the contenteditable
   * ProseMirror root), so each composer can preserve its existing testid
   * (`empty-state-input`/`agent-input`).
   */
  inputTestId?: string;
  /** `data-testid` for the submit button, same purpose as `inputTestId`. */
  submitTestId?: string;
  /**
   * Generation-cancellation (Task 4.2b): while a turn is generating, the
   * send button becomes a STOP button that halts the turn. `isGenerating`
   * is the workspace's `turnInFlight` signal; the composer is also
   * `disabled` during a turn (the Tiptap editor stays uneditable — you
   * can't type mid-turn), but the stop button is the one control that MUST
   * stay clickable while generating, so its enabled state is deliberately
   * independent of `disabled`. Omitted (both undefined) on surfaces with no
   * running turn to stop (`EmptyState.tsx`), which then always shows send.
   */
  isGenerating?: boolean;
  /** Fired when the stop button is clicked — halts the running turn. */
  onStop?: () => void;
  /**
   * Imperative focus requests are represented as a changing value rather
   * than a boolean so callers can request focus repeatedly while this
   * component stays mounted.
   */
  autoFocusToken?: number;
  /**
   * Queue "edit" (recall): pops a previously-queued message back into the
   * editor for editing. Modeled as a changing `token` (like `autoFocusToken`)
   * so the same message can be recalled repeatedly while mounted; the effect
   * clears the editor and prefills `content`/`richContent` (full-fidelity via
   * `richMessageContentToDoc`, so chips/attachments survive the round-trip),
   * then focuses the end. Omitted on surfaces with no queue (`EmptyState.tsx`).
   */
  recall?: { token: number; content: string; richContent?: RichMessageContent };
  /**
   * 010-context-window-management (UI refactor): an optional node rendered
   * immediately after the attach button — the composer-integrated context
   * usage gauge, when the caller has a real conversation to report usage
   * for (`Workspace.tsx`; omitted by `EmptyState.tsx`, which has
   * no conversation yet). Kept as an injected node rather than a
   * `conversationId` prop so this file stays decoupled from context-usage
   * concerns — callers own which gauge/component renders here, if any.
   */
  contextGauge?: ReactNode;
  /**
   * Optional conversation-goal control (the composer's ◎ toggle). When
   * present, the composer shows a ◎ Goal toggle in its toolbar (near the
   * attach button); the goal itself is DISPLAYED by the AgentActivity status
   * line above the composer, not here. Omitted on surfaces that have no goal
   * to manage (`EmptyState.tsx`, `UserAskWidget.tsx`).
   */
  goal?: {
    /** The active goal, or `null` if none is set. */
    current: string | null;
    /**
     * Persist the goal WITHOUT starting a turn — used to clear it (`null`) on
     * an empty goal-mode send. Never launches the agent.
     */
    onSet: (goal: string | null) => void;
    /**
     * "Send as goal": persist the goal AND kick off an agent turn to pursue
     * it (the goal text becomes the turn's message). This is what makes
     * setting a goal on an idle conversation actually start work, rather than
     * silently waiting for the next manual message.
     */
    onSendAsGoal: (goal: string) => void;
  };
  /**
   * A changing token requesting that the current goal be loaded back into the
   * composer for editing (goal mode + prefill). Driven by the AgentActivity
   * status line's "edit goal" control, whose display lives outside this
   * component but whose edit action needs this editor. Each distinct value
   * triggers one edit-entry.
   */
  editGoalToken?: number;
}

/**
 * 009-rich-chat-input: the shared rich-text input used by the composers in
 * EmptyState.tsx and Workspace.tsx. A
 * plain, multi-line-aware editor with Enter-to-send/Shift+Enter-for-newline
 * (User Story 1), plus paste-collapse for large pastes (User Story 2, T023/
 * T024): a paste crossing serialize.ts's `shouldCollapsePastedText`
 * threshold is intercepted via `editorProps.handlePaste` and inserted as a
 * `pastedText` chip instead of raw text; submitting builds a
 * `RichMessageContent` from the doc via `richMessageContentFromDoc` and
 * only attaches it when at least one chip is present. User Story 3 (T033)
 * adds the `SkillMention` extension (typing "/" opens a picker of installed
 * skills), gated on `skillsEnabled` at editor-creation time — see the
 * `useEditor()` call's own comment below for why a conditional array entry
 * is correct here despite the editor never being recreated. Attachments
 * land in US4. Focus requests are modeled as a changing `autoFocusToken`
 * and applied through the live Tiptap editor instance, so callers do not
 * reach into this component's DOM.
 *
 * Editor architecture (research.md's adopted mesh pattern): `useEditor()` is
 * called exactly once per instance's lifetime (@tiptap/react's `useEditor`
 * defaults to `deps = []`, confirmed against the installed version) and is
 * never recreated on re-render, even though `extensions`/`editorProps` are
 * fresh object literals every render. Mutable callback/config props used
 * by one-time editor handlers (`onSubmit`, `placeholder`) are stashed in a
 * ref and mirrored via their own effects, then read from the ref inside
 * the stable `handleKeyDown` callback captured once at editor-creation
 * time. `disabled` is applied imperatively via `editor.setEditable()` in
 * its own effect,
 * matching the contract's explicit instruction (not stashed in a ref read
 * by handleKeyDown, since nothing inside handleKeyDown needs to branch on
 * it — the submit button's own `disabled` prop and setEditable already
 * cover blocking input while disabled).
 */
export default function RichInput({
  onSubmit,
  skillsEnabled,
  disabled,
  placeholder,
  inputTestId,
  submitTestId,
  isGenerating,
  onStop,
  autoFocusToken,
  recall,
  contextGauge,
  goal,
  editGoalToken,
}: RichInputProps) {
  const onSubmitRef = useRef(onSubmit);
  const placeholderRef = useRef(placeholder);
  // `goal` mirrors `onSubmit`'s own ref pattern (see the comment on
  // `onSubmitRef` below): `submitCurrentContent` is captured once, at
  // editor-creation time, by `handleKeyDown` inside the `useEditor()` config
  // (deps=[]), so anything it reads that can change across renders — the
  // `goal` prop itself (a fresh object literal each render) and the
  // `goalMode` toggle state — has to come from a ref, not a closed-over
  // binding, or Enter-to-send would forever see this component's first
  // render's values.
  const goalRef = useRef(goal);
  const [goalMode, setGoalModeState] = useState(false);
  const goalModeRef = useRef(goalMode);
  const setGoalMode = (next: boolean) => {
    goalModeRef.current = next;
    setGoalModeState(next);
  };
  // handleKeyDown (below) is part of the config object passed to
  // useEditor(), which is only ever evaluated on the editor's one-time
  // creation — it can't close over the `editor` binding useEditor returns
  // on a given render (that binding isn't stable/current across renders
  // the way a ref is), so it reaches the current editor instance through
  // this ref instead.
  const editorRef = useRef<Editor | null>(null);

  // 009-rich-chat-input, User Story 4: surfaced the same way EmptyState.tsx
  // surfaces its own submit error (a plain inline `<p>` with a stable
  // `data-testid`, `text-destructive`) rather than inventing a new
  // error-UI convention — see this component's render below.
  const [attachmentError, setAttachmentError] = useState<string | null>(null);

  useEffect(() => {
    onSubmitRef.current = onSubmit;
  }, [onSubmit]);

  useEffect(() => {
    placeholderRef.current = placeholder;
  }, [placeholder]);

  useEffect(() => {
    goalRef.current = goal;
  }, [goal]);

  const insertAttachment = (attrs: AttachmentAttrs) => {
    editorRef.current?.chain().focus().insertContent({ type: "attachment", attrs }).run();
  };

  /**
   * Attachment byte acquisition (T045/T047): this app attaches a file's
   * bytes via one of *two* genuinely different code paths, depending on
   * whether a real, absolute filesystem path is obtainable for the file —
   * verified directly against this project's own installed platform (not
   * assumed), rather than treating paste/drop/picker as interchangeable:
   *
   * - **Paste** (`attachFromFile`, below) never has a real path. A pasted
   *   `File` (from `event.clipboardData`) is exactly what the standard
   *   browser Clipboard API exposes — no plugin, Tauri or otherwise,
   *   extends it with a backing filesystem path (many clipboard payloads,
   *   e.g. a screenshot or an image copied from a non-file source, aren't
   *   backed by a file at all). So paste reads the `File`'s own bytes
   *   directly, client-side (`fileToBase64`, `arrayBuffer()` + manual
   *   base64), and never calls `read_attached_file`.
   *
   * - **Drag-and-drop** (wired below, in the `onDragDropEvent` effect) DOES
   *   have a real path here — but only because of a specific, verified
   *   Tauri configuration fact, not because drag-drop is inherently
   *   path-ful: `tauri-utils`' `WindowConfig::drag_drop_enabled` defaults to
   *   `true`, and this project's `tauri.conf.json` doesn't override it.
   *   With it enabled, `wry` (confirmed directly against the installed
   *   `wry` 0.55.1 source, `wkwebview/drag_drop.rs`) intercepts the OS-level
   *   drag operation *natively* (`performDragOperation` reads real file
   *   paths straight off `NSDraggingInfo`'s pasteboard) and only falls
   *   through to the WebView's own HTML5 drag-drop DOM events when the
   *   registered Tauri listener declines the event — which Tauri's runtime
   *   never does while a JS `onDragDropEvent` listener is attached. So a
   *   plain ProseMirror `handleDrop` reading `event.dataTransfer.files`
   *   (mesh's own idiom, and what a naive port of `FileUploader` would do)
   *   would never see a real file in this app: the real payload is only
   *   ever delivered via `getCurrentWebview().onDragDropEvent()`'s `drop`
   *   variant, whose `paths` are genuine absolute filesystem paths — the
   *   exact same shape of input the native file-picker dialog produces.
   *   Drag-drop therefore reuses `attachFromPath`/`read_attached_file`, the
   *   *same* code path as the file-picker button (step 4), not the
   *   client-side `fileToBase64` path paste uses.
   *
   * The two helpers below are that split made concrete: `attachFromFile`
   * (client-side bytes, paste only) vs. `attachFromPath` (a real path,
   * read server-side via `read_attached_file` — drag-drop and the
   * file-picker button both funnel through it).
   */
  const attachFromFile = async (file: File) => {
    if (file.size > ATTACHMENT_MAX_BYTES) {
      setAttachmentError(attachmentSizeErrorMessage(file.name));
      return;
    }
    try {
      const data = await fileToBase64(file);
      insertAttachment({
        id: crypto.randomUUID(),
        name: file.name,
        mimeType: file.type || "application/octet-stream",
        data,
        isImage: isImageMimeType(file.type),
      });
      setAttachmentError(null);
    } catch {
      setAttachmentError(`Couldn't read "${file.name}".`);
    }
  };

  const attachFromPath = async (path: string) => {
    try {
      const attached = await commands.readAttachedFile(path);
      if (base64ByteLength(attached.data) > ATTACHMENT_MAX_BYTES) {
        setAttachmentError(attachmentSizeErrorMessage(attached.name));
        return;
      }
      insertAttachment({
        id: crypto.randomUUID(),
        name: attached.name,
        mimeType: attached.mimeType,
        data: attached.data,
        isImage: isImageMimeType(attached.mimeType),
      });
      setAttachmentError(null);
    } catch {
      setAttachmentError(`Couldn't attach "${path}".`);
    }
  };

  // T047: the file-picker button's handler. `open()`'s exact options are
  // research.md's verified snippet, copied as-is (confirmed against the
  // actually-installed `@tauri-apps/plugin-dialog` 2.7.1's `.d.ts`) — a
  // real path IS available here (the native dialog returns one directly),
  // so this is the flow `read_attached_file` was built for.
  const pickAttachment = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
    });
    if (!selected) return; // cancellation is a no-op (research.md)
    await attachFromPath(selected);
  };

  // T045: native OS drag-and-drop. See `attachFromFile`'s doc comment above
  // for the full, verified reasoning this is based on. `isTauri()` (from
  // `@tauri-apps/api/core`) guards this from running outside of a real
  // Tauri webview — `getCurrentWebview()` reads `window.__TAURI_INTERNALS__`
  // synchronously and throws without it, which is exactly the case for
  // every jsdom test that doesn't explicitly opt in via mocking
  // `@tauri-apps/api/webview` (this project's existing `RichInput.test.tsx`
  // does not, and isn't expected to for this reason).
  useEffect(() => {
    if (!isTauri()) return;

    let disposed = false;
    let unlisten: (() => void) | undefined;

    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        for (const path of event.payload.paths) {
          void attachFromPath(path);
        }
      })
      .then((fn) => {
        if (disposed) {
          fn();
        } else {
          unlisten = fn;
        }
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- registered
    // once per mount, matching every other one-time-registration effect in
    // this file (see the top-of-file architecture doc comment); `attachFromPath`
    // only ever reads `editorRef.current` and calls the stable `setAttachmentError`
    // setter, so capturing this render's closure permanently is safe.
  }, []);

  const submitCurrentContent = () => {
    const editor = editorRef.current;
    if (!editor) return;
    const text = editor.getText().trim();
    const richContent = richMessageContentFromDoc(editor.getJSON());
    // A collapsed pastedText chip contributes nothing to editor.getText()
    // (no textSerializer is registered for it — see serialize.ts), so a
    // message that's *entirely* a collapsed paste has an empty `text` here.
    // The real "is there anything to send" check is whether there's a
    // non-empty flat text OR at least one structured (non-text) segment —
    // matching data-model.md's "segments must not be empty" validation
    // rule, not the flat string's emptiness.
    const hasNonTextSegment = richContent.segments.some((segment) => segment.type !== "text");
    if (!text && !hasNonTextSegment) return;
    // Goal mode (composer relocation of the old topbar GoalBar): submitting
    // while the ◎ toggle is ON re-routes the current content to the goal
    // path instead of the normal `onSubmit` turn path — "send as goal"
    // rather than "send as message". Non-empty text is sent as the goal AND
    // kicks off a turn to pursue it (`onSendAsGoal`); an empty send in goal
    // mode clears the goal (`onSet(null)`). Branches here, before the
    // existing `onSubmitRef.current(...)` call, so every other submit path
    // (Enter, the send button click) is unaffected when goal mode is off.
    if (goalModeRef.current && goalRef.current) {
      if (text) {
        goalRef.current.onSendAsGoal(text);
      } else {
        goalRef.current.onSet(null);
      }
      editor.commands.clearContent(true);
      setGoalMode(false);
      return;
    }
    onSubmitRef.current(text, hasNonTextSegment ? richContent : undefined);
    editor.commands.clearContent(true);
  };

  const toggleGoalMode = () => {
    if (disabled) return;
    setGoalMode(!goalMode);
  };

  const startEditingGoal = () => {
    const editor = editorRef.current;
    if (editor) {
      editor.commands.clearContent(true);
      if (goal?.current) {
        editor.commands.insertContent(goal.current);
      }
      editor.commands.focus("end");
    }
    setGoalMode(true);
  };

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: false,
        blockquote: false,
        codeBlock: false,
        horizontalRule: false,
        dropcursor: false,
        bold: false,
      }),
      Placeholder.configure({
        placeholder: () => placeholderRef.current,
      }),
      PastedText,
      // Unconditional — unlike `SkillMention` below, attachments aren't
      // gated behind a flag (spec.md's FR-006/FR-007 have no such
      // restriction; every composing surface can attach a file).
      Attachment,
      // `useEditor()`'s `extensions` array is only ever evaluated once, at
      // this call's one-time editor-creation time (the doc comment above
      // explains why — `deps = []`), so conditionally including
      // `SkillMention` here based on `skillsEnabled` only takes effect at
      // mount. That's the deliberate choice (over always registering the
      // extension and making its "/" trigger a runtime no-op when disabled):
      // `skillsEnabled` is effectively static per mounted `RichInput`
      // instance in this app (`EmptyState` and `Workspace` pass `true`), so
      // there is no real prop-change case to handle. Omitting the extension
      // entirely when disabled is stronger than a runtime no-op:
      // `commands.listSkills()` is structurally unreachable when skills are
      // disabled.
      ...(skillsEnabled ? [SkillMention] : []),
    ],
    editorProps: {
      attributes: {
        ...(inputTestId ? { "data-testid": inputTestId } : {}),
        "data-slot": "input-group-control",
        class: "min-h-22 w-full px-3 py-2 text-sm leading-6 outline-none [&_p]:m-0",
      },
      handleKeyDown: (view, event) => {
        // `editorProps.handleKeyDown` (this function) is checked by
        // ProseMirror's `EditorView.someProp` *before* any plugin's own
        // `handleKeyDown` — including the `SkillMention` extension's
        // `Suggestion` plugin (confirmed directly against the installed
        // `prosemirror-view` source; see skill-mention.tsx's top-of-file
        // doc comment for the full trace). Left unguarded, Enter here would
        // always win the race and submit the message instead of letting
        // Enter confirm the currently-highlighted item in an open skill
        // picker. Deferring (returning `false`) while the picker is active
        // lets the event fall through to `Suggestion`'s own `handleKeyDown`,
        // which the extension's `onKeyDown` render-hook then handles.
        if (isSkillMentionSuggestionActive(view.state)) {
          return false;
        }
        if (event.key === "Enter" && !event.shiftKey) {
          event.preventDefault();
          submitCurrentContent();
          return true;
        }
        // Returning false (not handled) lets Tiptap/ProseMirror's default
        // Enter/Shift+Enter handling proceed — StarterKit's default
        // Shift+Enter behavior inserts a hard break.
        return false;
      },
      // research.md's "Paste-collapse via editorProps.handlePaste" decision,
      // mirroring mesh's own file-paste interception idiom: read the raw
      // clipboard text directly off the native ClipboardEvent (*before* any
      // insertion happens), decide via serialize.ts's threshold logic, and
      // either take over the paste entirely (collapsed chip) or return
      // `false` and let ProseMirror's default paste pipeline run exactly as
      // today — a short paste stays indistinguishable from typing.
      handlePaste: (_view, event) => {
        // T045: a pasted image/file takes priority over the text-collapse
        // check below — a clipboard item carrying a file has no meaningful
        // "text/plain" data to fall back to. See `attachFromFile`'s doc
        // comment (above, in this component) for why this always reads
        // bytes client-side rather than ever calling `read_attached_file`.
        const pastedFiles = getFilesFromClipboard(event.clipboardData ?? null);
        if (pastedFiles.length > 0) {
          event.preventDefault();
          for (const file of pastedFiles) {
            void attachFromFile(file);
          }
          return true;
        }

        const text = event.clipboardData?.getData("text/plain") ?? "";
        const { shouldCollapse, lineCount } = shouldCollapsePastedText(text);
        if (!shouldCollapse) return false;

        event.preventDefault();
        editorRef.current
          ?.chain()
          .focus()
          .insertContent({
            type: "pastedText",
            attrs: { id: crypto.randomUUID(), text, lineCount },
          })
          .run();
        return true;
      },
    },
  });

  editorRef.current = editor;

  useEffect(() => {
    editor?.setEditable(!disabled);
  }, [editor, disabled]);

  useEffect(() => {
    if (autoFocusToken === undefined) return;
    editor?.commands.focus("end");
  }, [autoFocusToken, editor]);

  // Edit-goal requests from the status line: a changing `editGoalToken` loads
  // the goal back into the composer (goal mode + prefill). A ref-tracked
  // previous value skips the mount run, so the composer only enters edit mode
  // on an actual click, never on first render.
  const editGoalTokenRef = useRef(editGoalToken);
  useEffect(() => {
    if (editGoalToken === editGoalTokenRef.current) return;
    editGoalTokenRef.current = editGoalToken;
    startEditingGoal();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the
    // token alone; `startEditingGoal` closes over the current `goal`/editor
    // and is re-created each render, so including it would re-run this effect
    // every render (the token guard would still gate the action, but the
    // subscription churn is pointless).
  }, [editGoalToken]);

  // Queue "edit"/recall: prefill the editor from a recalled queued message.
  // Keyed on `recall.token` so re-recalling the same message re-fires. Rich
  // content is rebuilt via `richMessageContentToDoc` (chips/attachments
  // intact); plain content is inserted as text. Cleared first so a recall
  // always replaces, never appends to, whatever is being composed.
  useEffect(() => {
    if (recall === undefined || !editor) return;
    editor.commands.clearContent(true);
    if (recall.richContent) {
      editor.commands.setContent(richMessageContentToDoc(recall.richContent));
    } else if (recall.content) {
      editor.commands.insertContent(recall.content);
    }
    editor.commands.focus("end");
    // Only re-run when a NEW recall arrives (its token changes), not on every
    // editor identity change — matching the autoFocusToken effect's intent.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recall?.token, editor]);

  // useEditor() itself doesn't trigger a re-render on every keystroke
  // (shouldRerenderOnTransaction defaults to off) — this is the documented
  // opt-in way to reactively derive UI state (here, whether to disable the
  // submit button) from the editor's content without recreating the editor
  // or forcing a re-render on every single transaction for unrelated
  // reasons.
  const isEmpty = useEditorState({
    editor,
    selector: ({ editor }) => !editor || editor.isEmpty,
  });

  return (
    <div className="flex flex-col gap-1">
      <InputGroup
        className={cn(
          "border-transparent bg-secondary shadow-none focus-within:shadow-sm",
          "has-[[data-slot=input-group-control]:focus-visible]:border-transparent",
          "has-[[data-slot=input-group-control]:focus-visible]:ring-0",
          // Stock InputGroup ships `dark:bg-input/30`, which wins on
          // specificity over the plain `bg-secondary` above in dark mode
          // (verified against the compiled CSS) — pin the dark surface
          // explicitly so the composer doesn't revert to the stock input
          // background. tailwind-merge dedupes this against the stock class.
          "dark:bg-secondary",
        )}
      >
        <div className="flex-1 w-full">
          <EditorContent
            editor={editor}
            className={cn(
              "w-full",
              "[&_.ProseMirror]:!outline-none [&_.ProseMirror:focus-visible]:!outline-none [&_.ProseMirror:focus]:!outline-none",
              "[&_.ProseMirror_p.is-editor-empty:first-child::before]:content-[attr(data-placeholder)]",
              "[&_.ProseMirror_p.is-editor-empty:first-child::before]:text-muted-foreground",
              "[&_.ProseMirror_p.is-editor-empty:first-child::before]:float-left",
              "[&_.ProseMirror_p.is-editor-empty:first-child::before]:pointer-events-none",
              "[&_.ProseMirror_p.is-editor-empty:first-child::before]:h-0",
            )}
          />
        </div>
        <InputGroupAddon align="block-end">
          {/* T047: the file-picker button (paperclip, matching this
              codebase's existing icon-button styling — same shape as the
              submit button below, ghost-default since this is a
              secondary action). */}
          <InputGroupButton
            size="icon-xs"
            className="aria-disabled:opacity-50"
            onClick={() => {
              // While generating, this button is intentionally NOT natively
              // `disabled` (see the `disabled` prop below), so a mid-turn
              // click would otherwise reach here — guard it so the file
              // picker never opens while a turn is in flight.
              if (disabled) return;
              void pickAttachment();
            }}
            // During a turn (`isGenerating`) the composer is `disabled`, but
            // the stop button beside this one must render at FULL opacity. A
            // real `<button disabled>` here makes the whole InputGroup match
            // `:has(:disabled)` (stock `has-disabled:opacity-50`) and
            // composite at 50% — which the stop button, a child, cannot
            // override. So while generating, keep this button OUT of
            // `:disabled` and mark it inert via `aria-disabled` + the onClick
            // guard above, dimming only itself (`aria-disabled:opacity-50`).
            // Every other disabled state (e.g. a pending tool call, not
            // generating) keeps the real `disabled` attribute — no stop
            // button is shown there and the whole composer SHOULD read as
            // disabled.
            disabled={disabled && !isGenerating}
            aria-disabled={disabled}
            aria-label="Attach a file"
            data-testid="rich-input-attach"
          >
            <Plus size={16} />
          </InputGroupButton>
          {/* Composer relocation of the old topbar GoalBar (reference design's
              ◎ toggle): idle = ghost icon; ON = accent-colored (variant
              "default") with a "Goal" label beside the icon, matching the
              reference's second state. Only rendered when the caller opted
              in via the `goal` prop (Workspace.tsx's main composer) — omitted
              entirely on surfaces with no goal to manage. */}
          {goal && (
            <Tooltip>
              <TooltipTrigger
                render={
                  <InputGroupButton
                    size="icon-xs"
                    variant={goalMode ? "default" : "ghost"}
                    className="aria-disabled:opacity-50"
                    onClick={toggleGoalMode}
                    disabled={disabled && !isGenerating}
                    aria-disabled={disabled}
                    aria-pressed={goalMode}
                    aria-label={goalMode ? "Exit goal mode" : "Set as goal"}
                    data-testid="rich-input-goal-toggle"
                  />
                }
              >
                <Target size={16} />
              </TooltipTrigger>
              <TooltipContent data-testid="rich-input-goal-tooltip">
                {goalMode ? "Exit goal mode" : "Set as goal"}
              </TooltipContent>
            </Tooltip>
          )}
          {contextGauge}
          {/* Queue & steer: the submit button ALWAYS renders now (it never
              swaps out for stop) so a message can be composed and QUEUED while
              a turn is generating — submitting mid-turn enqueues (the caller
              routes on its own busy state), so its intent reads "Queue message"
              then. The stop button is rendered ALONGSIDE it during generation
              (it used to replace it), the one control that stays clickable
              regardless of `disabled`. */}
          <InputGroupButton
            variant="default"
            size="icon-sm"
            className="ml-auto aria-disabled:opacity-50"
            onClick={submitCurrentContent}
            disabled={disabled}
            aria-disabled={disabled || isEmpty}
            // Goal mode changes the send button's intent — submitting sets the
            // conversation goal instead of sending; while generating, an
            // ordinary submit queues rather than sends.
            aria-label={
              isGenerating ? "Queue message" : goal && goalMode ? "Send as goal" : "Send message"
            }
            title={isGenerating ? "Queue message" : goal && goalMode ? "Send as goal" : undefined}
            data-testid={submitTestId}
          >
            <ArrowUp size={16} />
          </InputGroupButton>
          {isGenerating && (
            <InputGroupButton
              variant="destructive"
              size="icon-sm"
              onClick={onStop}
              aria-label="Stop generating"
              data-testid="stop-generation"
            >
              <Square size={16} className="fill-current" />
            </InputGroupButton>
          )}
        </InputGroupAddon>
      </InputGroup>
      {/* Inline error surface for an oversized/unreadable attachment —
          same pattern as EmptyState.tsx's own `data-testid="empty-state-error"`
          submit-error paragraph, not a new error-UI convention. */}
      {attachmentError && (
        <p className="px-1 text-xs text-destructive" data-testid="rich-input-attachment-error">
          {attachmentError}
        </p>
      )}
    </div>
  );
}
