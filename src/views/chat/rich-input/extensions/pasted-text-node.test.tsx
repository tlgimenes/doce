import { describe, it, expect } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect } from "react";
import { useEditor, EditorContent, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import PastedText from "./pasted-text-node";

/**
 * 009-rich-chat-input, User Story 2 (T020): the "<pasted N lines>" chip a
 * large paste collapses into (spec.md scenarios 1/3, research.md's
 * "Atom-node modeling"/"Expandable pasted-text chip" decisions). Tier-2
 * jsdom component test per research.md's Testing strategy — a minimal
 * editor mounting *only* this extension (plus the base document schema),
 * structural assertions only, no pixel geometry.
 */

function TestHarness({ onReady }: { onReady: (editor: Editor) => void }) {
  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: false,
        blockquote: false,
        codeBlock: false,
        horizontalRule: false,
        dropcursor: false,
      }),
      PastedText,
    ],
  });

  useEffect(() => {
    if (editor) onReady(editor);
  }, [editor, onReady]);

  return <EditorContent editor={editor} data-testid="test-editor" />;
}

/** Renders the harness and resolves once the editor instance is available. */
async function setup(): Promise<Editor> {
  let current: Editor | null = null;
  render(<TestHarness onReady={(editor) => (current = editor)} />);
  await waitFor(() => expect(current).not.toBeNull());
  return current!;
}

describe("pastedText node (009-rich-chat-input, US2)", () => {
  it('renders a "<pasted N lines>" chip for a pastedText node\'s attrs', async () => {
    const editor = await setup();

    act(() => {
      editor.commands.insertContent({
        type: "pastedText",
        attrs: { id: "p1", text: "line one\nline two\nline three", lineCount: 3 },
      });
    });

    const chip = await screen.findByTestId("pasted-text-chip");
    expect(chip).toHaveTextContent("<pasted 3 lines>");

    // Collapsed: the full pasted text is not present in the plain-text
    // representation of the doc — only the chip's own label text is.
    expect(editor.getText()).not.toContain("line one");
  });

  it("clicking the chip expands it into the original, editable text at the same document position", async () => {
    const editor = await setup();

    act(() => {
      editor.commands.insertContent([
        { type: "text", text: "before " },
        {
          type: "pastedText",
          attrs: { id: "p1", text: "line one\nline two\nline three", lineCount: 3 },
        },
        { type: "text", text: " after" },
      ]);
    });

    expect(editor.getText()).toBe("before  after");

    const chip = await screen.findByTestId("pasted-text-chip");
    const user = userEvent.setup();
    await user.click(chip.querySelector("button")!);

    // The atom node is gone from the doc...
    expect(screen.queryByTestId("pasted-text-chip")).not.toBeInTheDocument();
    // ...replaced by its original text, at the same position between
    // "before " and " after", newlines intact.
    expect(editor.getText()).toBe("before line one\nline two\nline three after");

    // Cursor lands in ordinary editable text, not still a node selection.
    expect(editor.state.selection.empty).toBe(true);
  });

  // 009-rich-chat-input, User Story 2 (T025/T026): UserMessageContent.tsx
  // mounts this same node view inside a second, `editable: false` Tiptap
  // instance (contracts/rich-chat-input.md's "UserMessageContent component
  // contract" — "no expand-on-click for pasted text"). The click handler
  // itself has to honor that, since Tiptap's `editable` flag only gates the
  // DOM's native contentEditable behavior — it does not stop a NodeView's
  // own React `onClick` from programmatically dispatching a command.
  it("does not expand the chip when the editor is not editable (read-only rendering)", async () => {
    const editor = await setup();

    act(() => {
      editor.commands.insertContent({
        type: "pastedText",
        attrs: { id: "p1", text: "line one\nline two\nline three", lineCount: 3 },
      });
    });
    act(() => {
      editor.setEditable(false);
    });

    const chip = await screen.findByTestId("pasted-text-chip");
    const user = userEvent.setup();
    await user.click(chip.querySelector("button")!);

    // Still collapsed: clicking a read-only chip is a no-op.
    expect(await screen.findByTestId("pasted-text-chip")).toBeInTheDocument();
    expect(editor.getText()).not.toContain("line one");
  });
});
