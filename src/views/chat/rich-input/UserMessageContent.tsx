import { useEditor, EditorContent } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import PastedText from "./extensions/pasted-text-node";
import SkillMention from "./extensions/skill-mention";
import Attachment from "./extensions/attachment-node";
import { richMessageContentToDoc } from "./serialize";
import type { RichMessageContent, RichTextSegment } from "@/lib/ipc";

export interface UserMessageContentProps {
  /**
   * A `content_type='rich_text'` message's raw `content` — a JSON string
   * (`JSON.stringify({ segments })`, data-model.md), or (defensively) any
   * other string, which degrades to plain text below.
   */
  content: string;
}

const KNOWN_SEGMENT_TYPES = new Set<RichTextSegment["type"]>([
  "text",
  "pastedText",
  "attachment",
  "skill",
]);

/**
 * Parses `content` as `RichMessageContent`, the same defensively-degrading
 * way `parseToolResultDetail` (`src/lib/ipc.ts`) already does for
 * `tool_result` rows — a JSON parse failure, or a segment with an
 * unrecognized `type`, falls back to `null` (data-model.md's Frontend Types
 * section: "a parse failure or unrecognized segment type falls back to
 * rendering the raw string") rather than throwing into the message list.
 */
function parseRichMessageContent(content: string): RichMessageContent | null {
  try {
    const parsed = JSON.parse(content) as { segments?: unknown };
    if (!parsed || !Array.isArray(parsed.segments)) return null;
    const allSegmentsKnown = parsed.segments.every(
      (segment) =>
        segment !== null &&
        typeof segment === "object" &&
        KNOWN_SEGMENT_TYPES.has((segment as { type?: unknown }).type as RichTextSegment["type"]),
    );
    if (!allSegmentsKnown) return null;
    return parsed as RichMessageContent;
  } catch {
    return null;
  }
}

/**
 * 009-rich-chat-input, User Story 2 (T026) + User Story 3 (T034/T035): the
 * read-only history rendering path for a `content_type='rich_text'` user
 * message (contracts/rich-chat-input.md's "UserMessageContent component
 * contract"). Mounts a second, `editable: false` Tiptap instance sharing
 * `RichInput`'s node extensions (StarterKit pared down the same way, plus
 * `pastedText`, `skillMention`, and `attachment`) — the mesh-mirrored pattern
 * (`~/code/mesh`'s `message/user.tsx`) of rendering chips through the same
 * NodeViews the live editor uses, rather than a second, hand-written
 * rendering implementation that could drift from it.
 * `pastedText`'s own click handler already no-ops when `editor.isEditable`
 * is false (pasted-text-node.tsx), so no extra plumbing is needed here to
 * make that chip non-interactive; `skillMention`'s chip has no click handler
 * at all (skill-mention.tsx — a skill marker has nothing to expand into), so
 * it's inert here for free. `SkillMention` is included unconditionally
 * (unlike `RichInput`'s `skillsEnabled`-gated inclusion) because history
 * rendering must show a "skill" segment regardless of whether the chat this
 * message belongs to currently allows the "/" picker — its
 * `addProseMirrorPlugins`-registered `Suggestion` plugin only reacts to
 * typing, which can't happen on a non-editable instance anyway.
 *
 * `Attachment` (US4, T048/T049) must be registered here too, not just in
 * `RichInput`'s live editor: `serialize.ts`'s `richMessageContentToDoc`
 * reconstructs a real `attachment` node from a persisted segment, and
 * without this extension `prosemirror-model` throws `RangeError: Unknown
 * node type: attachment` on that doc — a throw Tiptap's `createDocument`
 * catches and silently downgrades to an *empty* document, blanking the
 * entire message (not just the attachment chip). Its chip
 * (attachment-node.tsx) has no click handler either, so — like
 * `skillMention` — it's inert here for free; the only US4-specific behavior
 * worth double-checking is the hover-preview `<img>` for `isImage: true`
 * attachments, which is plain CSS (`group-hover`) and needs no read-only
 * gating of its own.
 */
export default function UserMessageContent({ content }: UserMessageContentProps) {
  const parsed = parseRichMessageContent(content);

  if (!parsed) {
    return <p className="whitespace-pre-wrap text-sm">{content}</p>;
  }

  return <RichTextRenderer content={parsed} />;
}

/**
 * Split out from `UserMessageContent` so the parse-failure fallback above
 * never mounts a Tiptap editor at all — `useEditor` can't be called
 * conditionally (Rules of Hooks), so this component's the boundary that
 * keeps the plain-text fallback path free of that cost.
 */
function RichTextRenderer({ content }: { content: RichMessageContent }) {
  const editor = useEditor({
    editable: false,
    extensions: [
      StarterKit.configure({
        heading: false,
        blockquote: false,
        codeBlock: false,
        horizontalRule: false,
        dropcursor: false,
      }),
      PastedText,
      SkillMention,
      Attachment,
    ],
    content: richMessageContentToDoc(content),
  });

  return (
    <EditorContent
      editor={editor}
      className="text-sm leading-6 [&_.ProseMirror]:outline-none [&_p]:m-0"
    />
  );
}
