import type { JSONContent } from "@tiptap/core";
import type { RichMessageContent, RichTextSegment } from "@/lib/ipc";

/**
 * 009-rich-chat-input: pure, DOM-free logic for turning raw editor
 * input/output into `RichMessageContent`-shaped data. Holds both the
 * paste-collapse threshold decision (T022, User Story 2) and the
 * doc-to-`RichMessageContent` conversion (T024) `RichInput.tsx`'s submit
 * path calls.
 */

// FR-003 / research.md's "Paste-collapse via editorProps.handlePaste"
// decision: a paste collapses into a `pastedText` chip once it exceeds
// *either* threshold, whichever is reached first — mirroring research.md's
// own idiom (`.split("\n").length > 10`).
const PASTE_COLLAPSE_LINE_THRESHOLD = 10;
const PASTE_COLLAPSE_CHAR_THRESHOLD = 500;

export interface PasteCollapseDecision {
  /**
   * Whether this pasted text should collapse into a `pastedText` chip
   * instead of being inserted as plain text.
   */
  shouldCollapse: boolean;
  /**
   * Line count of the pasted text, computed via `text.split("\n").length`
   * (matching data-model.md's `pastedText.lineCount` convention exactly),
   * so a caller that decides to collapse can reuse this value directly as
   * the chip node's `lineCount` attribute without recomputing it.
   */
  lineCount: number;
}

/**
 * Decides whether a raw pasted string should collapse into a compact
 * "<pasted N lines>" chip, per FR-003: text collapses once it exceeds
 * roughly 10 lines OR roughly 500 characters, whichever is reached first.
 *
 * Boundary semantics (spec.md: "exceeds roughly 10 lines or 500
 * characters" / "longer than roughly 10 lines or 500 characters" — both
 * "more than" language, not "at least"): the threshold triggers only when
 * a value is STRICTLY GREATER than the limit. Text at exactly 10 lines and
 * exactly 500 characters does NOT collapse — collapsing requires actually
 * exceeding a threshold, not merely meeting it.
 *
 * Pure and DOM-free: takes the raw clipboard text, returns a decision.
 * Callers (e.g. RichInput's `handlePaste` plugin, T023) own acting on it —
 * this function never touches the editor or the DOM.
 */
export function shouldCollapsePastedText(text: string): PasteCollapseDecision {
  const lineCount = text.split("\n").length;
  const shouldCollapse =
    lineCount > PASTE_COLLAPSE_LINE_THRESHOLD || text.length > PASTE_COLLAPSE_CHAR_THRESHOLD;

  return { shouldCollapse, lineCount };
}

/**
 * Walks a Tiptap/ProseMirror JSON doc (`editor.getJSON()`) and produces the
 * `RichMessageContent` it represents (data-model.md's `RichMessageContent`
 * section), for T024/RichInput's submit path.
 *
 * The doc can contain `text`/`hardBreak` (StarterKit), `pastedText`
 * (pasted-text-node.tsx, US2), `skillMention` (skill-mention.tsx, US3), and
 * `attachment` (attachment-node.tsx, US4) nodes, so every variant of the
 * `RichTextSegment` union (`"text"`/`"pastedText"`/`"skill"`/`"attachment"`)
 * is actually produced here.
 *
 * Consecutive `text`/`hardBreak` runs merge into one `"text"` segment
 * (`hardBreak` contributes `"\n"`, matching its `renderText()` — see
 * pasted-text-node.tsx's doc comment), with a `"\n\n"` paragraph-break
 * inserted between sibling block nodes once something has already been
 * emitted — the same idea as `editor.getText()`'s own
 * `blockSeparator: "\n\n"` default, so a plain-typed message's segments
 * join back into exactly what `editor.getText()` would produce for it. A
 * `pastedText` node flushes any accumulated text into its own segment
 * first, so segment order always matches the doc's left-to-right order.
 */
export function richMessageContentFromDoc(doc: JSONContent): RichMessageContent {
  const segments: RichTextSegment[] = [];
  let buffer = "";
  let sawBlock = false;

  const flushBuffer = () => {
    if (buffer.length > 0) {
      segments.push({ type: "text", text: buffer });
      buffer = "";
    }
  };

  const walk = (nodes: JSONContent[] | undefined) => {
    for (const node of nodes ?? []) {
      if (node.type === "text") {
        buffer += node.text ?? "";
      } else if (node.type === "hardBreak") {
        buffer += "\n";
      } else if (node.type === "pastedText") {
        flushBuffer();
        const attrs = (node.attrs ?? {}) as { id: string; text: string; lineCount: number };
        segments.push({
          type: "pastedText",
          id: attrs.id,
          text: attrs.text,
          lineCount: attrs.lineCount,
        });
      } else if (node.type === "skillMention") {
        flushBuffer();
        const attrs = (node.attrs ?? {}) as { id: string; name: string };
        segments.push({
          type: "skill",
          id: attrs.id,
          name: attrs.name,
        });
      } else if (node.type === "attachment") {
        flushBuffer();
        const attrs = (node.attrs ?? {}) as {
          id: string;
          name: string;
          mimeType: string;
          data: string;
          isImage: boolean;
        };
        segments.push({
          type: "attachment",
          id: attrs.id,
          name: attrs.name,
          mimeType: attrs.mimeType,
          data: attrs.data,
          isImage: attrs.isImage,
        });
      } else if (node.content) {
        // A block-level container (paragraph; list wrappers, should the
        // editor ever produce one via StarterKit's default input rules).
        if (sawBlock) buffer += "\n\n";
        sawBlock = true;
        walk(node.content);
      }
    }
  };

  walk(doc.content);
  flushBuffer();

  return { segments };
}

/**
 * The inverse of `richMessageContentFromDoc`, for `UserMessageContent.tsx`'s
 * (T026) read-only history rendering path: turns a persisted
 * `RichMessageContent` back into a Tiptap/ProseMirror JSON doc, so the same
 * node extensions (`StarterKit` + `pastedText`) render it — a second,
 * `editable: false` editor instance, per contracts/rich-chat-input.md's
 * mesh-mirroring "UserMessageContent component contract" — instead of a
 * hand-written parallel rendering implementation that could drift from
 * `RichInput`'s.
 *
 * Not a byte-exact structural round-trip of `richMessageContentFromDoc`
 * (that function can fold multiple original paragraphs' text together into
 * one `"text"` segment, separated by a `"\n\n"` it inserts into the segment
 * itself — see that function's doc comment), but reconstructing paragraph
 * breaks from `"\n\n"` and line breaks from the remaining `"\n"` reproduces
 * the same visible text and chip placement, which is what read-only
 * rendering needs.
 *
 * An `attachment` segment now reconstructs into an `attachment` node
 * (attachment-node.tsx, US4) the same way `pastedText`/`skill` do above.
 * Note this doesn't by itself make `UserMessageContent.tsx` render it —
 * that component's own editor instance needs the `Attachment` extension
 * registered too (a separate task) before a doc built from this function
 * can mount cleanly there.
 */
export function richMessageContentToDoc(content: RichMessageContent): JSONContent {
  const paragraphs: JSONContent[][] = [[]];
  const currentParagraph = () => paragraphs[paragraphs.length - 1];

  const pushText = (text: string) => {
    const paragraphChunks = text.split("\n\n");
    paragraphChunks.forEach((chunk, chunkIndex) => {
      if (chunkIndex > 0) paragraphs.push([]);
      const lines = chunk.split("\n");
      lines.forEach((line, lineIndex) => {
        if (lineIndex > 0) currentParagraph().push({ type: "hardBreak" });
        if (line.length > 0) currentParagraph().push({ type: "text", text: line });
      });
    });
  };

  for (const segment of content.segments) {
    if (segment.type === "text") {
      pushText(segment.text);
    } else if (segment.type === "pastedText") {
      currentParagraph().push({
        type: "pastedText",
        attrs: { id: segment.id, text: segment.text, lineCount: segment.lineCount },
      });
    } else if (segment.type === "skill") {
      currentParagraph().push({
        type: "skillMention",
        attrs: { id: segment.id, name: segment.name },
      });
    } else if (segment.type === "attachment") {
      currentParagraph().push({
        type: "attachment",
        attrs: {
          id: segment.id,
          name: segment.name,
          mimeType: segment.mimeType,
          data: segment.data,
          isImage: segment.isImage,
        },
      });
    }
  }

  return {
    type: "doc",
    content: paragraphs.map((inline) => ({
      type: "paragraph",
      ...(inline.length > 0 ? { content: inline } : {}),
    })),
  };
}
