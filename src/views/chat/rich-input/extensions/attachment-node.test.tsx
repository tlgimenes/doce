import { describe, it, expect } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { useEditor, EditorContent, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import Attachment from "./attachment-node";

/**
 * 009-rich-chat-input, User Story 4 (T042): the "<imagename.png>"-style
 * attachment chip a paste/drag-drop/file-picker attachment turns into
 * (spec.md's US4 acceptance scenarios; research.md's "Atom-node modeling"
 * decision, mesh's `FileNode` pattern). Tier-2 jsdom component test per
 * research.md's Testing strategy — a minimal editor mounting *only* this
 * extension (plus the base document schema), structural assertions only:
 * presence/absence of an `<img>` preview element and its `src`, not real
 * `:hover` triggering (jsdom has no layout engine to drive that
 * meaningfully — see research.md's Testing strategy tier-2 notes).
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
      Attachment,
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

describe("attachment node (009-rich-chat-input, US4)", () => {
  it("renders a compact chip showing the filename for an image attachment", async () => {
    const editor = await setup();

    act(() => {
      editor.commands.insertContent({
        type: "attachment",
        attrs: {
          id: "a1",
          name: "photo.png",
          mimeType: "image/png",
          data: "ZmFrZS1pbWFnZS1ieXRlcw==",
          isImage: true,
        },
      });
    });

    const chip = await screen.findByTestId("attachment-chip");
    expect(chip).toHaveTextContent("photo.png");
  });

  it("shows an <img> preview built from mimeType+data only when isImage is true", async () => {
    const editor = await setup();

    act(() => {
      editor.commands.insertContent({
        type: "attachment",
        attrs: {
          id: "a1",
          name: "photo.png",
          mimeType: "image/png",
          data: "ZmFrZS1pbWFnZS1ieXRlcw==",
          isImage: true,
        },
      });
    });

    const chip = await screen.findByTestId("attachment-chip");
    const img = chip.querySelector("img");
    expect(img).not.toBeNull();
    expect(img).toHaveAttribute("src", "data:image/png;base64,ZmFrZS1pbWFnZS1ieXRlcw==");
  });

  it("shows filename/mimeType as text and no <img> preview when isImage is false", async () => {
    const editor = await setup();

    act(() => {
      editor.commands.insertContent({
        type: "attachment",
        attrs: {
          id: "a2",
          name: "report.pdf",
          mimeType: "application/pdf",
          data: "ZmFrZS1wZGYtYnl0ZXM=",
          isImage: false,
        },
      });
    });

    const chip = await screen.findByTestId("attachment-chip");
    expect(chip).toHaveTextContent("report.pdf");
    expect(chip).toHaveTextContent("application/pdf");
    expect(chip.querySelector("img")).not.toBeInTheDocument();
  });
});
