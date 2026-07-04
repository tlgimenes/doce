import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import UserMessageContent from "./UserMessageContent";
import type { RichMessageContent } from "@/lib/ipc";

/**
 * 009-rich-chat-input, User Story 2 (T025): the read-only history rendering
 * path for a `content_type='rich_text'` user message (contracts/
 * rich-chat-input.md's "UserMessageContent component contract"). Per the
 * mesh-mirroring contract, this mounts a second, `editable: false` Tiptap
 * instance sharing the identical node extensions RichInput's live editor
 * uses, so the same chip visuals render — non-interactively — rather than a
 * hand-written parallel rendering implementation. Tier-2 jsdom component
 * tests, matching pasted-text-node.test.tsx/RichInput.test.tsx's own
 * conventions for this codebase's first contenteditable-backed component.
 */
describe("UserMessageContent (009-rich-chat-input, US2)", () => {
  it('renders the same "<pasted N lines>" chip a live editor produces, and clicking it does not expand it (read-only)', async () => {
    const richContent: RichMessageContent = {
      segments: [
        { type: "text", text: "before " },
        { type: "pastedText", id: "p1", text: "line one\nline two\nline three", lineCount: 3 },
        { type: "text", text: " after" },
      ],
    };

    render(<UserMessageContent content={JSON.stringify(richContent)} />);

    const chip = await screen.findByTestId("pasted-text-chip");
    expect(chip).toHaveTextContent("<pasted 3 lines>");
    // The full pasted text stays collapsed — not present anywhere in the
    // rendered doc's plain text.
    expect(screen.queryByText(/line one/)).not.toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(chip.querySelector("button")!);

    // Read-only: the click is a no-op — the chip is still collapsed, the
    // original pasted text never becomes visible/editable.
    expect(await screen.findByTestId("pasted-text-chip")).toBeInTheDocument();
    expect(screen.queryByText(/line one/)).not.toBeInTheDocument();
  });

  it("renders a plain text-only rich_text message as plain readable text, not literal JSON", async () => {
    const richContent: RichMessageContent = {
      segments: [{ type: "text", text: "hello world" }],
    };

    render(<UserMessageContent content={JSON.stringify(richContent)} />);

    expect(await screen.findByText("hello world")).toBeInTheDocument();
    // The raw JSON wrapper never leaks into the rendered text.
    expect(screen.queryByText(/"segments"/)).not.toBeInTheDocument();
    expect(screen.queryByText(/"type":"text"/)).not.toBeInTheDocument();
  });

  it("falls back to rendering the raw content string as plain text when JSON parsing fails", async () => {
    render(<UserMessageContent content="not valid json" />);

    expect(await screen.findByText("not valid json")).toBeInTheDocument();
  });

  it('renders a "skill" segment as the same "/name" marker chip a live editor produces, non-interactively, and never leaks the skill\'s injected content (US3, T034/T035)', async () => {
    const richContent: RichMessageContent = {
      segments: [
        { type: "text", text: "please use " },
        { type: "skill", id: "s1", name: "reviewer" },
        { type: "text", text: " on this" },
      ],
    };

    render(<UserMessageContent content={JSON.stringify(richContent)} />);

    const chip = await screen.findByTestId("skill-mention-chip");
    expect(chip).toHaveTextContent("/reviewer");
    expect(screen.getByText("please use", { exact: false })).toBeInTheDocument();
    expect(screen.getByText("on this", { exact: false })).toBeInTheDocument();

    // data-model.md: the injected skill content only ever exists in the
    // model-facing expansion, never in what's persisted or displayed — the
    // "skill" segment only ever carries `{ id, name }`, so there's nothing
    // resembling injected file content to leak, but assert it directly
    // against the rendered DOM anyway.
    expect(screen.queryByText(/injected/i)).not.toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/# |```/);

    // Read-only: clicking the chip is a no-op — no picker, no re-trigger.
    // Unlike the pastedText chip, the skill chip renders no interactive
    // element at all (no <button>), so there's nothing to click into
    // reopening a picker.
    expect(chip.querySelector("button")).toBeNull();
    const user = userEvent.setup();
    await user.click(chip);
    expect(screen.queryByTestId("skill-mention-popup")).not.toBeInTheDocument();
    expect(await screen.findByTestId("skill-mention-chip")).toHaveTextContent("/reviewer");
  });

  it('renders an "attachment" segment as the same image chip + hover preview a live editor produces, read-only (US4, T048/T049)', async () => {
    const richContent: RichMessageContent = {
      segments: [
        { type: "text", text: "look at " },
        {
          type: "attachment",
          id: "a1",
          name: "photo.png",
          mimeType: "image/png",
          data: "ZmFrZS1pbWFnZS1ieXRlcw==",
          isImage: true,
        },
        { type: "text", text: " please" },
      ],
    };

    render(<UserMessageContent content={JSON.stringify(richContent)} />);

    // Confirms the doc mounts cleanly at all: before the `Attachment`
    // extension is registered here, Tiptap's `createDocument` swallows the
    // `RangeError: Unknown node type: attachment` `prosemirror-model` throws
    // and silently falls back to an empty document — surrounding text would
    // vanish too, not just the chip.
    expect(await screen.findByText("look at", { exact: false })).toBeInTheDocument();
    expect(screen.getByText("please", { exact: false })).toBeInTheDocument();

    const chip = await screen.findByTestId("attachment-chip");
    expect(chip).toHaveTextContent("photo.png");

    const preview = screen.getByTestId("attachment-preview");
    const img = preview.querySelector("img");
    expect(img).not.toBeNull();
    expect(img).toHaveAttribute("src", "data:image/png;base64,ZmFrZS1pbWFnZS1ieXRlcw==");
  });

  it('renders a non-image "attachment" segment as filename + mimeType text with no preview, read-only (US4, T048/T049)', async () => {
    const richContent: RichMessageContent = {
      segments: [
        {
          type: "attachment",
          id: "a2",
          name: "report.pdf",
          mimeType: "application/pdf",
          data: "ZmFrZS1wZGYtYnl0ZXM=",
          isImage: false,
        },
      ],
    };

    render(<UserMessageContent content={JSON.stringify(richContent)} />);

    const chip = await screen.findByTestId("attachment-chip");
    expect(chip).toHaveTextContent("report.pdf");
    expect(chip).toHaveTextContent("application/pdf");
    expect(chip.querySelector("img")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attachment-preview")).not.toBeInTheDocument();
  });
});
