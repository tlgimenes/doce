import { mergeAttributes, Node, type JSONContent } from "@tiptap/core";
import { NodeViewWrapper, ReactNodeViewRenderer, type ReactNodeViewProps } from "@tiptap/react";

/**
 * 009-rich-chat-input, User Story 2 (T021): the collapsed "<pasted N
 * lines>" chip a large paste turns into (spec.md's US2 acceptance
 * scenarios; research.md's "Atom-node modeling" / "Expandable pasted-text
 * chip" decisions). Modeled as an atomic inline node — same technique as
 * mesh's FileNode/MentionNode — so it deletes as one unit rather than
 * character-by-character. Styled with this codebase's own chip tokens
 * (`rounded-lg border border-border bg-card`, matching
 * `004-tool-call-widgets`' BashWidget/ReadWidget), not mesh's literal
 * amber/violet colors (research.md is explicit this isn't copied
 * verbatim).
 *
 * Clicking the chip replaces itself, in place, with the original pasted
 * text — split into plain text runs joined by `hardBreak` nodes so the
 * exact original line breaks reappear (HardBreak's `renderText` returns
 * "\n", so `editor.getText()` reproduces the original text verbatim after
 * expansion), cursor left at the end of the restored text.
 */

export interface PastedTextAttrs {
  id: string;
  text: string;
  lineCount: number;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    pastedText: {
      /** Replaces the pastedText node spanning [pos, pos + nodeSize) with `text`, editable in place (spec.md's US2 scenario 3 / FR-004). */
      expandPastedText: (pos: number, nodeSize: number, text: string) => ReturnType;
    };
  }
}

/**
 * "line one\nline two" -> [{type:"text",text:"line one"},{type:"hardBreak"},{type:"text",text:"line two"}]
 * — reconstructs the original newlines as hardBreak nodes rather than
 * collapsing them, since a plain-text ProseMirror text node can't itself
 * contain a "\n" character.
 */
function textToInlineContent(text: string): JSONContent[] {
  const lines = text.split("\n");
  return lines.flatMap((line, index) => {
    const nodes: JSONContent[] = line.length > 0 ? [{ type: "text", text: line }] : [];
    if (index < lines.length - 1) nodes.push({ type: "hardBreak" });
    return nodes;
  });
}

function PastedTextChip({ node, editor, getPos }: ReactNodeViewProps) {
  const { text, lineCount } = node.attrs as PastedTextAttrs;

  const handleClick = () => {
    // 009-rich-chat-input, US2 (T026): UserMessageContent.tsx mounts this
    // exact node view in a second, `editable: false` Tiptap instance for
    // read-only history rendering (contracts/rich-chat-input.md). Tiptap's
    // `editable` flag only gates the DOM's native contentEditable behavior
    // — it doesn't stop a NodeView's own React `onClick` from
    // programmatically dispatching a command — so the no-expand-on-click
    // guarantee has to be enforced here explicitly.
    if (!editor.isEditable) return;
    const pos = getPos();
    if (typeof pos !== "number") return;
    editor.chain().focus().expandPastedText(pos, node.nodeSize, text).run();
  };

  return (
    <NodeViewWrapper as="span" contentEditable={false} data-testid="pasted-text-chip">
      <button
        type="button"
        onClick={handleClick}
        className="mx-0.5 inline-flex items-center rounded-lg border border-border bg-card px-1.5 py-0.5 align-baseline text-xs text-muted-foreground hover:bg-accent"
      >
        {`<pasted ${lineCount} lines>`}
      </button>
    </NodeViewWrapper>
  );
}

const PastedText = Node.create({
  name: "pastedText",
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
      text: {
        default: "",
        parseHTML: (element) => element.getAttribute("data-text") ?? "",
        renderHTML: (attributes) => ({ "data-text": attributes.text }),
      },
      lineCount: {
        default: 0,
        parseHTML: (element) => Number(element.getAttribute("data-line-count") ?? 0),
        renderHTML: (attributes) => ({ "data-line-count": String(attributes.lineCount) }),
      },
    };
  },

  parseHTML() {
    return [{ tag: `span[data-type="${this.name}"]` }];
  },

  renderHTML({ HTMLAttributes }) {
    return ["span", mergeAttributes({ "data-type": this.name }, HTMLAttributes)];
  },

  addCommands() {
    return {
      expandPastedText:
        (pos, nodeSize, text) =>
        ({ commands }) =>
          commands.insertContentAt({ from: pos, to: pos + nodeSize }, textToInlineContent(text)),
    };
  },

  addNodeView() {
    return ReactNodeViewRenderer(PastedTextChip);
  },
});

export default PastedText;
