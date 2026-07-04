import { mergeAttributes, Node } from "@tiptap/core";
import { NodeViewWrapper, ReactNodeViewRenderer, type ReactNodeViewProps } from "@tiptap/react";

/**
 * 009-rich-chat-input, User Story 4 (T043): the "<imagename.png>"-style chip
 * an attached image/file turns into (spec.md's US4 acceptance scenarios;
 * research.md's "Atom-node modeling" decision). Same atom-node + NodeView
 * technique as `pasted-text-node.tsx`/`skill-mention.tsx` (mesh's `FileNode`
 * pattern) so it deletes as one unit rather than character-by-character, and
 * matches `RichTextSegmentAttachment`'s shape (`src/lib/ipc.ts`) exactly —
 * `{ id, name, mimeType, data, isImage }` — so `serialize.ts`'s
 * `richMessageContentFromDoc`/`richMessageContentToDoc` (US4 wiring, a later
 * stage) can round-trip a doc node's `attrs` straight into/out of an
 * `attachment` segment with no shape translation.
 *
 * `data` (base64, no `data:` prefix — data-model.md) never leaves the
 * client: it's used only to build a local `data:${mimeType};base64,${data}`
 * URI for the hover preview below, and per FR-009/`expand_segments`
 * (`rich_content.rs`, already implemented — T037/T038) it's never part of
 * what's sent to the model.
 *
 * Styled with this codebase's own chip tokens (`rounded-lg border
 * border-border bg-card`, matching `pasted-text-node.tsx`/
 * `skill-mention.tsx`), with the image case's hover-reveal built the same
 * CSS-only `group`/`group-hover` way this codebase already uses elsewhere
 * (`ConversationList.tsx`'s hover-revealed action buttons) rather than a
 * JS mouseenter/mouseleave handler — jsdom has no real `:hover`/layout
 * engine to drive (research.md's Testing strategy), so
 * `attachment-node.test.tsx` only asserts the preview `<img>` is present in
 * the DOM (CSS-gated by `group-hover:opacity-100`), not that it's actually
 * visible at a given pointer position.
 */

export interface AttachmentAttrs {
  id: string;
  name: string;
  mimeType: string;
  data: string;
  isImage: boolean;
}

function AttachmentChip({ node }: ReactNodeViewProps) {
  const { name, mimeType, data, isImage } = node.attrs as AttachmentAttrs;

  return (
    <NodeViewWrapper as="span" contentEditable={false} data-testid="attachment-chip">
      <span className="group relative mx-0.5 inline-flex items-center gap-1 rounded-lg border border-border bg-card px-1.5 py-0.5 align-baseline text-xs text-muted-foreground">
        <span>{isImage ? name : `${name} (${mimeType})`}</span>
        {isImage ? (
          <span
            className="pointer-events-none absolute bottom-full left-0 z-10 mb-1 hidden overflow-hidden rounded-lg border border-border bg-card p-1 opacity-0 shadow-lg transition-opacity group-hover:block group-hover:opacity-100"
            data-testid="attachment-preview"
          >
            <img
              src={`data:${mimeType};base64,${data}`}
              alt={name}
              className="max-h-40 max-w-40 rounded"
            />
          </span>
        ) : null}
      </span>
    </NodeViewWrapper>
  );
}

const Attachment = Node.create({
  name: "attachment",
  group: "inline",
  inline: true,
  atom: true,

  addAttributes() {
    return {
      id: {
        default: null,
        parseHTML: (element) => element.getAttribute("data-id"),
        renderHTML: (attributes) => ({ "data-id": attributes.id }),
      },
      name: {
        default: "",
        parseHTML: (element) => element.getAttribute("data-name") ?? "",
        renderHTML: (attributes) => ({ "data-name": attributes.name }),
      },
      mimeType: {
        default: "",
        parseHTML: (element) => element.getAttribute("data-mime-type") ?? "",
        renderHTML: (attributes) => ({ "data-mime-type": attributes.mimeType }),
      },
      data: {
        default: "",
        parseHTML: (element) => element.getAttribute("data-data") ?? "",
        renderHTML: (attributes) => ({ "data-data": attributes.data }),
      },
      isImage: {
        default: false,
        parseHTML: (element) => element.getAttribute("data-is-image") === "true",
        renderHTML: (attributes) => ({ "data-is-image": String(attributes.isImage) }),
      },
    };
  },

  parseHTML() {
    return [{ tag: `span[data-type="${this.name}"]` }];
  },

  renderHTML({ HTMLAttributes }) {
    return ["span", mergeAttributes({ "data-type": this.name }, HTMLAttributes)];
  },

  addNodeView() {
    return ReactNodeViewRenderer(AttachmentChip);
  },
});

export default Attachment;
