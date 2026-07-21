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

  it("renders the composer shell on stock InputGroup", () => {
    const { container } = render(
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
    const group = container.querySelector('[data-slot="input-group"]');

    expect(group).toBeInstanceOf(HTMLElement);
    expect(group).toContainElement(editable);
    expect(editable).toHaveAttribute("data-slot", "input-group-control");
  });

  it("styles the group bubble-gray (matching the user bubble's secondary variant), with no stock focus ring", () => {
    const { container } = render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const group = container.querySelector('[data-slot="input-group"]');
    expect(group).toBeInstanceOf(HTMLElement);
    const className = group?.className ?? "";
    expect(className).toContain("bg-secondary");
    expect(className).toContain("focus-within:shadow-sm");
    // Stock InputGroup ships `dark:bg-input/30`, which otherwise wins on
    // specificity over the plain `bg-secondary` above in dark mode — pin
    // the dark surface explicitly (twMerge dedupes it against the stock
    // class) so the composer doesn't revert to the stock input background.
    expect(className).toContain("dark:bg-secondary");
    // tailwind-merge dedupes the stock has-[…focus-visible] border/ring
    // group in favor of this override — the stock ring must not survive
    // (it's the same has-[] modifier as our ring-0/border-transparent, so
    // twMerge keeps only the later-declared value).
    expect(className).not.toMatch(
      /has-\[\[data-slot=input-group-control\]:focus-visible\]:ring-3(?:\s|$)/,
    );
    expect(className).not.toMatch(
      /has-\[\[data-slot=input-group-control\]:focus-visible\]:border-ring(?:\s|$)/,
    );
    expect(className).toContain("has-[[data-slot=input-group-control]:focus-visible]:ring-0");
    expect(className).toContain(
      "has-[[data-slot=input-group-control]:focus-visible]:border-transparent",
    );
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

  it("does not put the group in its disabled state while merely empty", () => {
    const onSubmit = vi.fn();
    const { container } = render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    // Stock InputGroup styles the whole group via `has-disabled:` off any
    // `:disabled` descendant — an empty (but not composer-disabled)
    // composer must not natively disable the send button, or the entire
    // field renders washed-out gray at rest.
    const group = container.querySelector('[data-slot="input-group"]');
    expect(group?.querySelector(":disabled")).toBeNull();

    const send = screen.getByTestId("test-submit");
    expect(send).toHaveAttribute("aria-disabled", "true");
    expect(send).not.toBeDisabled();
  });

  it("clicking send while empty does not call onSubmit", async () => {
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

    await userEvent.click(screen.getByTestId("test-submit"));

    expect(onSubmit).not.toHaveBeenCalled();
  });
});

/**
 * Generation-cancellation (Task 4.2b): while a turn is generating, the send
 * button swaps to a STOP button (same slot, plain icon swap) that halts the
 * turn; clicking it calls `onStop`. The stop button must stay clickable even
 * though the composer is `disabled` during a turn — it is the one control
 * that must work while generating.
 */
describe("RichInput (generation-cancellation, Task 4.2b — stop button)", () => {
  it("shows the send button (not the stop button) when not generating", () => {
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

    expect(screen.getByTestId("test-submit")).toBeInTheDocument();
    expect(screen.queryByTestId("stop-generation")).not.toBeInTheDocument();
  });

  it("shows the single button as STOP while generating with an empty input, and clicking it calls onStop", async () => {
    const onStop = vi.fn();
    render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        // Editable while a turn runs so a message can be composed to queue.
        disabled={false}
        isGenerating={true}
        onStop={onStop}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    // Empty input + generating → the single button is Stop (no separate send).
    const stop = screen.getByTestId("stop-generation");
    expect(stop).toBeInTheDocument();
    expect(stop).toHaveAccessibleName("Stop generating");
    expect(stop).not.toBeDisabled();
    expect(screen.queryByTestId("test-submit")).not.toBeInTheDocument();

    await userEvent.click(stop);
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("flips the single button from Stop to Send once the user types while generating (text intent wins)", async () => {
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        isGenerating={true}
        onStop={vi.fn()}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    // Empty → Stop.
    expect(screen.getByTestId("stop-generation")).toBeInTheDocument();
    expect(screen.queryByTestId("test-submit")).not.toBeInTheDocument();

    // Type → the button becomes Send (queue); Stop is gone.
    await userEvent.click(screen.getByTestId("test-input"));
    await userEvent.keyboard("queued while busy");
    expect(screen.getByTestId("test-submit")).toBeInTheDocument();
    expect(screen.queryByTestId("stop-generation")).not.toBeInTheDocument();

    // Enter queues it.
    await userEvent.keyboard("{Enter}");
    expect(onSubmit).toHaveBeenCalledWith("queued while busy", undefined);
  });

  it("keeps the group free of any :disabled descendant while generating, so it (and the stop button) stays full-opacity", () => {
    const { container } = render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        // Queue & steer production shape: editable while generating.
        disabled={false}
        isGenerating={true}
        onStop={vi.fn()}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    // With the composer editable (disabled=false) during a turn, NOTHING in the
    // group carries a real HTML `disabled`, so `:has(:disabled)` never matches
    // and the group composites at full opacity. jsdom has no CSS engine, so we
    // assert the root cause is absent rather than reading opacity.
    const group = container.querySelector('[data-slot="input-group"]');
    expect(group?.querySelector(":disabled")).toBeNull();

    // The attach button is fully enabled while generating (compose freely).
    const attach = screen.getByTestId("rich-input-attach");
    expect(attach).not.toBeDisabled();
    expect(attach).toHaveAttribute("aria-disabled", "false");
  });

  it("stays editable while generating so a message can be composed to queue", async () => {
    const onSubmit = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        isGenerating={true}
        onStop={vi.fn()}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    expect(editable).toHaveAttribute("contenteditable", "true");
    await userEvent.click(editable);
    await userEvent.keyboard("queued while busy{Enter}");
    expect(onSubmit).toHaveBeenCalledWith("queued while busy", undefined);
  });

  it("keeps the attach button natively disabled when disabled but NOT generating (no stop button — the composer should read disabled)", () => {
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

    // Unchanged behavior for the non-generating disabled state: a pending
    // tool call shows no stop button, so the whole composer SHOULD dim.
    expect(screen.getByTestId("rich-input-attach")).toBeDisabled();
  });

  it("morphs the single button between Stop (empty + generating) and a disabled Send (idle)", () => {
    const { rerender } = render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        isGenerating={true}
        onStop={vi.fn()}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    // Empty + generating → Stop only.
    expect(screen.getByTestId("stop-generation")).toBeInTheDocument();
    expect(screen.queryByTestId("test-submit")).not.toBeInTheDocument();

    rerender(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        isGenerating={false}
        onStop={vi.fn()}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    // Empty + idle → Send only (disabled), Stop gone.
    expect(screen.queryByTestId("stop-generation")).not.toBeInTheDocument();
    const submit = screen.getByTestId("test-submit");
    expect(submit).toBeInTheDocument();
    expect(submit).toHaveAttribute("aria-disabled", "true");
  });
});

/**
 * Queue & steer: the `recall` prop pops a previously-queued message back into
 * the editor for editing (the queue-row "Edit" action). Keyed on a changing
 * `token`, it clears the editor and prefills text or full rich content.
 */
describe("RichInput (queue & steer — recall prop)", () => {
  it("clears and prefills the editor with recalled text and focuses it", async () => {
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

    rerender(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        recall={{ token: 1, content: "recall me" }}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    const editable = screen.getByTestId("test-input");
    await waitFor(() => expect(editable).toHaveTextContent("recall me"));
    // Focus lands via editor.commands.focus("end"); assert on activeElement
    // (the proven pattern for the autoFocusToken tests) rather than toHaveFocus.
    await waitFor(() => expect(document.activeElement).toBe(editable));
  });

  it("rebuilds recalled rich content into a pastedText chip", async () => {
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

    rerender(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        recall={{
          token: 1,
          content: "",
          richContent: {
            segments: [{ type: "pastedText", id: "p1", text: "a\nb\nc", lineCount: 3 }],
          },
        }}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
      />,
    );

    expect(await screen.findByTestId("pasted-text-chip")).toBeInTheDocument();
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

/**
 * The composer's conversation-goal controls: the ◎ toggle and the "send as
 * goal" submit branch (opt-in via the `goal` prop), plus the `editGoalToken`
 * hook the AgentActivity status line uses to load a goal back for editing. The
 * goal itself is DISPLAYED by that status line above the composer, not by a
 * banner in this component.
 */
describe("RichInput (goal-composer-ui — conversation goal in the composer)", () => {
  it("does not render the goal toggle when the goal prop is omitted", () => {
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

    expect(screen.queryByTestId("rich-input-goal-toggle")).not.toBeInTheDocument();
    expect(screen.queryByTestId("rich-input-goal-banner")).not.toBeInTheDocument();
  });

  it("renders the goal toggle when the goal prop is given, idle (not pressed) by default", () => {
    render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
        goal={{ current: null, onSet: vi.fn(), onSendAsGoal: vi.fn() }}
      />,
    );

    const toggle = screen.getByTestId("rich-input-goal-toggle");
    expect(toggle).toBeInTheDocument();
    expect(toggle).toHaveAttribute("aria-pressed", "false");
  });

  it("clicking the goal toggle enters goal mode: the icon-only toggle shows pressed with an updated accessible label, and the send button's aria-label becomes 'Send as goal'", async () => {
    render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
        goal={{ current: null, onSet: vi.fn(), onSendAsGoal: vi.fn() }}
      />,
    );

    const toggle = screen.getByTestId("rich-input-goal-toggle");
    // Icon-only: no visible text label, the meaning lives in the accessible
    // name (and a hover tooltip).
    expect(toggle).toHaveAccessibleName("Set as goal");
    expect(toggle).not.toHaveTextContent("Goal");

    await userEvent.click(toggle);

    expect(toggle).toHaveAttribute("aria-pressed", "true");
    expect(toggle).toHaveAccessibleName("Exit goal mode");
    expect(screen.getByTestId("test-submit")).toHaveAccessibleName("Send as goal");
  });

  it("submitting (Enter) while in goal mode calls goal.onSendAsGoal with the typed text (persist + start a turn), NOT onSet or onSubmit, clears the editor, and exits goal mode", async () => {
    const onSubmit = vi.fn();
    const onSet = vi.fn();
    const onSendAsGoal = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
        goal={{ current: null, onSet, onSendAsGoal }}
      />,
    );

    await userEvent.click(screen.getByTestId("rich-input-goal-toggle"));

    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);
    await userEvent.type(editable, "Ship the login page{Enter}");

    expect(onSendAsGoal).toHaveBeenCalledTimes(1);
    expect(onSendAsGoal).toHaveBeenCalledWith("Ship the login page");
    expect(onSet).not.toHaveBeenCalled();
    expect(onSubmit).not.toHaveBeenCalled();
    expect(editable).toHaveTextContent("");
    // Goal mode is exited after a successful "send as goal" — the toggle
    // reverts to idle and the send button's label reverts too.
    expect(screen.getByTestId("rich-input-goal-toggle")).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByTestId("test-submit")).toHaveAccessibleName("Send message");
  });

  it("submitting (the send button) while in goal mode also routes to goal.onSendAsGoal instead of onSubmit", async () => {
    const onSubmit = vi.fn();
    const onSet = vi.fn();
    const onSendAsGoal = vi.fn();
    render(
      <RichInput
        onSubmit={onSubmit}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
        goal={{ current: null, onSet, onSendAsGoal }}
      />,
    );

    await userEvent.click(screen.getByTestId("rich-input-goal-toggle"));
    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);
    await userEvent.type(editable, "Ship the login page");
    await userEvent.click(screen.getByTestId("test-submit"));

    expect(onSendAsGoal).toHaveBeenCalledWith("Ship the login page");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("the goal is not rendered as a banner here — only the toggle (display moved to the status line)", () => {
    render(
      <RichInput
        onSubmit={vi.fn()}
        skillsEnabled={false}
        disabled={false}
        placeholder="p"
        inputTestId="test-input"
        submitTestId="test-submit"
        goal={{ current: "Ship the login page", onSet: vi.fn(), onSendAsGoal: vi.fn() }}
      />,
    );

    expect(screen.queryByTestId("rich-input-goal-banner")).not.toBeInTheDocument();
    expect(screen.getByTestId("rich-input-goal-toggle")).toBeInTheDocument();
  });

  it("a changing editGoalToken prefills the editor with the goal text and enters goal mode, but the mount value does not", () => {
    const props = {
      onSubmit: vi.fn(),
      skillsEnabled: false,
      disabled: false,
      placeholder: "p",
      inputTestId: "test-input",
      submitTestId: "test-submit",
      goal: { current: "Ship the login page", onSet: vi.fn(), onSendAsGoal: vi.fn() },
    };
    const { rerender } = render(<RichInput {...props} editGoalToken={0} />);

    // The initial token must NOT enter edit mode on mount.
    expect(screen.getByTestId("test-input")).not.toHaveTextContent("Ship the login page");
    expect(screen.getByTestId("rich-input-goal-toggle")).toHaveAttribute("aria-pressed", "false");

    // A new token (the status line's edit control) loads the goal for editing.
    rerender(<RichInput {...props} editGoalToken={1} />);

    expect(screen.getByTestId("test-input")).toHaveTextContent("Ship the login page");
    expect(screen.getByTestId("rich-input-goal-toggle")).toHaveAttribute("aria-pressed", "true");
  });
});
