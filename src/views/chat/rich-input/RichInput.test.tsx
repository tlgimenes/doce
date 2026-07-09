import { describe, it, expect, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import RichInput from "./RichInput";

/**
 * 009-rich-chat-input, User Story 1 (T004): the shared rich-text input that
 * is used by EmptyState.tsx and Workspace.tsx. Tier-2 jsdom component tests
 * per research.md's Testing strategy — structural/rendering correctness
 * only, driven via userEvent.type()/userEvent.keyboard() on an empty/focused
 * editor. No pixel-geometry assertions.
 */
describe("RichInput (009-rich-chat-input, US1)", () => {
  it("renders with the given placeholder", () => {
    const { container } = render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="Message doce…"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    // Tiptap's Placeholder extension decorates the empty paragraph with a
    // `data-placeholder` attribute (see @tiptap/extensions' placeholder
    // plugin) rather than inserting literal text — jsdom has no layout
    // engine to render the CSS `::before` content itself, so asserting on
    // the attribute is the correct structural check for this tier.
    expect(container.querySelector('[data-placeholder="Message doce…"]')).toBeInTheDocument();
    expect(screen.getByTestId("test-input")).toBeInTheDocument();
    expect(screen.getByTestId("test-submit")).toBeInTheDocument();
  });

  it("only applies the composer shadow while focus is inside the input", () => {
    render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    const frame = editable.parentElement?.parentElement;

    expect(frame).toBeInstanceOf(HTMLElement);
    expect(frame).not.toHaveClass("shadow-xs");
    expect(frame).not.toHaveClass("shadow-sm");
    expect(frame).toHaveClass("focus-within:shadow-sm");
  });

  it("typing produces the expected doc text", async () => {
    render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);
    await userEvent.type(editable, "hello world");

    expect(editable).toHaveTextContent("hello world");
  });

  it("focuses the editor when autoFocusToken changes", async () => {
    const { rerender } = render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    document.body.focus();
    expect(document.activeElement).not.toBe(editable);

    rerender(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
        autoFocusToken={1}
      />,
    );

    await waitFor(() => expect(document.activeElement).toBe(editable));
  });

  it("focuses the editor again when autoFocusToken changes repeatedly", async () => {
    const { rerender } = render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
        autoFocusToken={1}
      />,
    );

    const editable = screen.getByTestId("test-input");
    document.body.focus();
    expect(document.activeElement).not.toBe(editable);

    rerender(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
        autoFocusToken={2}
      />,
    );

    await waitFor(() => expect(document.activeElement).toBe(editable));
  });

  it("Enter (no Shift) calls onSubmit with the typed text and clears the editor", async () => {
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);
    await userEvent.type(editable, "hello there{Enter}");

    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith("hello there", undefined);
    // The input clears after a successful submit, mirroring the existing
    // raw-textarea inputs' setInput("") behavior after a successful send.
    expect(editable).toHaveTextContent("");
  });

  it("Shift+Enter inserts a newline without calling onSubmit", async () => {
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);
    await userEvent.type(editable, "line one{Shift>}{Enter}{/Shift}line two");

    expect(onSubmit).not.toHaveBeenCalled();
    expect(editable.textContent).toContain("line one");
    expect(editable.textContent).toContain("line two");
    // StarterKit's default Shift+Enter behavior inserts a hard break inline
    // (not a new paragraph) — confirms the newline actually landed in the
    // doc, not just that submit was skipped.
    expect(editable.querySelectorAll("br").length).toBeGreaterThan(0);
  });

  it("does not call onSubmit for empty content", async () => {
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);
    await userEvent.keyboard("{Enter}");

    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("does not call onSubmit for whitespace-only content", async () => {
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);
    await userEvent.type(editable, "   {Enter}");

    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("disabled toggles the live editor's editability via editor.setEditable(), without remounting", async () => {
    const { rerender } = render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editableBefore = screen.getByTestId("test-input");
    // ProseMirror's EditorView reflects `editor.isEditable` directly onto
    // its root DOM node's `contenteditable` attribute on every
    // setEditable()/props update (prosemirror-view's `attrs.contenteditable
    // = String(view.editable)`) — asserting on it here is asserting on
    // `editor.isEditable`'s effect, not a remount.
    expect(editableBefore).toHaveAttribute("contenteditable", "true");

    rerender(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={true}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editableAfter = screen.getByTestId("test-input");
    // Same DOM node identity across the disabled transition -> not a
    // remount.
    expect(editableAfter).toBe(editableBefore);
    expect(editableAfter).toHaveAttribute("contenteditable", "false");

    rerender(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    expect(screen.getByTestId("test-input")).toHaveAttribute("contenteditable", "true");
  });

  it("disables the submit button while disabled", () => {
    render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={true}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    expect(screen.getByTestId("test-submit")).toBeDisabled();
  });
});

/**
 * 009-rich-chat-input, User Story 2 (T023/T024): the paste-collapse
 * `editorProps.handlePaste` handler and the submit-time
 * doc-to-`RichMessageContent` wiring (research.md's "Paste-collapse via
 * `editorProps.handlePaste`" decision; data-model.md's `RichMessageContent`
 * section). `userEvent.paste(text)` dispatches a real `paste` DOM event
 * with `clipboardData` populated from the given string (readable via
 * `getData("text/plain")`), which is exactly what ProseMirror's own
 * `handlePaste` plugin-prop hook receives — no need to reach into
 * ProseMirror internals directly.
 */
describe("RichInput (009-rich-chat-input, US2 — paste-collapse)", () => {
  it("a short paste (under the ~10-line/~500-char threshold) is indistinguishable from typing: no chip, and richContent is undefined on submit", async () => {
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);
    await userEvent.paste("short paste");
    await userEvent.keyboard("{Enter}");

    expect(screen.queryByTestId("pasted-text-chip")).not.toBeInTheDocument();
    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith("short paste", undefined);
  });

  it("a paste over the line threshold (>10 lines) collapses into a pastedText chip, and submitting attaches richContent with the full original text and correct lineCount", async () => {
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    const longText = Array.from({ length: 15 }, (_, i) => `line ${i}`).join("\n");

    await userEvent.click(editable);
    await userEvent.paste(longText);

    const chip = await screen.findByTestId("pasted-text-chip");
    expect(chip).toHaveTextContent("<pasted 15 lines>");

    await userEvent.keyboard("{Enter}");

    expect(onSubmit).toHaveBeenCalledTimes(1);
    const [, richContent] = onSubmit.mock.calls[0];
    expect(richContent).toBeDefined();
    expect(richContent!.segments).toEqual([
      { type: "pastedText", id: expect.any(String), text: longText, lineCount: 15 },
    ]);
  });

  it("a paste over the char threshold (>500 chars, single line) also collapses, full text preserved untruncated", async () => {
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    const longText = "x".repeat(600);

    await userEvent.click(editable);
    await userEvent.paste(longText);
    await screen.findByTestId("pasted-text-chip");
    await userEvent.keyboard("{Enter}");

    const [, richContent] = onSubmit.mock.calls[0];
    expect(richContent!.segments).toEqual([
      { type: "pastedText", id: expect.any(String), text: longText, lineCount: 1 },
    ]);
    // The full 600-character original is preserved verbatim, not truncated.
    expect((richContent!.segments[0] as { text: string }).text).toHaveLength(600);
  });

  it("plain text before and after a pasted chip produces ordered text/pastedText/text segments", async () => {
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    const longText = "y".repeat(600);

    await userEvent.click(editable);
    await userEvent.type(editable, "before ");
    await userEvent.paste(longText);
    await screen.findByTestId("pasted-text-chip");
    // Appending text immediately after a just-inserted NodeView via
    // userEvent.type()'s real-keystroke simulation is unreliable in jsdom
    // (research.md's Testing strategy: real caret positioning near a
    // ReactNodeViewRenderer chip is a documented tier-2 limitation,
    // confirmed empirically here too — typed characters land inside the
    // chip's own button instead of the document). A second short paste
    // exercises the identical "plain content lands right after the chip"
    // behavior through `handlePaste`'s real default-passthrough path
    // (below threshold -> `return false`) without depending on jsdom's
    // real-keystroke DOM-selection sync.
    await userEvent.paste("after");
    await userEvent.keyboard("{Enter}");

    expect(onSubmit).toHaveBeenCalledTimes(1);
    const [, richContent] = onSubmit.mock.calls[0];
    expect(richContent!.segments).toEqual([
      { type: "text", text: "before " },
      { type: "pastedText", id: expect.any(String), text: longText, lineCount: 1 },
      { type: "text", text: "after" },
    ]);
  });

  it("a message that is entirely a collapsed paste (no other typed text) still submits", async () => {
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    const longText = "z".repeat(600);

    await userEvent.click(editable);
    await userEvent.paste(longText);
    await screen.findByTestId("pasted-text-chip");
    await userEvent.keyboard("{Enter}");

    expect(onSubmit).toHaveBeenCalledTimes(1);
    const [, richContent] = onSubmit.mock.calls[0];
    expect(richContent!.segments).toEqual([
      { type: "pastedText", id: expect.any(String), text: longText, lineCount: 1 },
    ]);
  });
});
