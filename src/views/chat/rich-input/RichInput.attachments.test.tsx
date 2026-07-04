import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { commands } from "@/lib/ipc";
import RichInput from "./RichInput";

/**
 * 009-rich-chat-input, User Story 4 (T044/T046 + submit wiring): paste,
 * native drag-and-drop, and the file-picker button all insert an
 * `attachment` chip; submitting produces a richContent `"attachment"`
 * segment. Co-located in its own file (not RichInput.test.tsx) since this
 * is the one area of RichInput.tsx that touches Tauri-specific surfaces
 * (`@tauri-apps/api/webview`, `@tauri-apps/plugin-dialog`, `readAttachedFile`)
 * that the rest of RichInput.test.tsx's tests don't need to mock at all —
 * see RichInput.tsx's own top-of-file doc comment on `attachFromFile`/
 * `attachFromPath` for why paste and drag-drop/the picker button
 * genuinely use two different byte-acquisition code paths.
 */

vi.mock("@/lib/ipc", () => ({
  commands: {
    readAttachedFile: vi.fn(),
  },
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

// `isTauri` defaults to `false` here (the real value in every jsdom test
// that doesn't opt in) so RichInput's drag-drop `useEffect` no-ops without
// needing `getCurrentWebview` to do anything — flipped to `true` per-test
// only in the drag-and-drop describe block below.
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: vi.fn(() => false),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: vi.fn(),
}));

function renderInput(onSubmit = vi.fn()) {
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
  return onSubmit;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(isTauri).mockReturnValue(false);
});

describe("RichInput (009-rich-chat-input, US4 — paste)", () => {
  it("pasting an image file inserts an attachment chip with an image preview, without calling readAttachedFile", async () => {
    renderInput();
    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);

    const file = new File(["hello"], "photo.png", { type: "image/png" });
    fireEvent.paste(editable, {
      clipboardData: {
        items: [{ kind: "file", getAsFile: () => file }],
        getData: () => "",
      },
    });

    const chip = await screen.findByTestId("attachment-chip");
    expect(chip).toHaveTextContent("photo.png");
    const preview = await screen.findByTestId("attachment-preview");
    // "hello" base64-encoded, matching fileToBase64's own encoding.
    expect(preview.querySelector("img")).toHaveAttribute(
      "src",
      "data:image/png;base64,aGVsbG8=",
    );
    expect(commands.readAttachedFile).not.toHaveBeenCalled();
  });

  it("pasting a non-image file shows filename/mimeType text, no preview", async () => {
    renderInput();
    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);

    const file = new File(["hi"], "notes.txt", { type: "text/plain" });
    fireEvent.paste(editable, {
      clipboardData: {
        items: [{ kind: "file", getAsFile: () => file }],
        getData: () => "",
      },
    });

    const chip = await screen.findByTestId("attachment-chip");
    expect(chip).toHaveTextContent("notes.txt");
    expect(chip).toHaveTextContent("text/plain");
    expect(chip.querySelector("img")).not.toBeInTheDocument();
  });

  it("a paste with both text and a file kind item treats it as a file paste (no plain text/pastedText fallback)", async () => {
    renderInput();
    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);

    const file = new File(["hello"], "photo.png", { type: "image/png" });
    fireEvent.paste(editable, {
      clipboardData: {
        items: [{ kind: "file", getAsFile: () => file }],
        getData: () => "some text",
      },
    });

    await screen.findByTestId("attachment-chip");
    expect(screen.queryByTestId("pasted-text-chip")).not.toBeInTheDocument();
  });

  it("an oversized pasted file is rejected with an inline error and no chip is inserted", async () => {
    renderInput();
    const editable = screen.getByTestId("test-input");
    await userEvent.click(editable);

    const oversized = new File([new Uint8Array(10 * 1024 * 1024 + 1)], "big.png", {
      type: "image/png",
    });
    fireEvent.paste(editable, {
      clipboardData: {
        items: [{ kind: "file", getAsFile: () => oversized }],
        getData: () => "",
      },
    });

    await waitFor(() => {
      expect(screen.getByTestId("rich-input-attachment-error")).toHaveTextContent(
        /10MB attachment limit/,
      );
    });
    expect(screen.queryByTestId("attachment-chip")).not.toBeInTheDocument();
  });
});

describe("RichInput (009-rich-chat-input, US4 — file-picker button)", () => {
  it("clicking the attach button opens the native dialog, then reads the picked path via readAttachedFile and inserts a chip", async () => {
    vi.mocked(open).mockResolvedValue("/Users/tester/photo.png");
    vi.mocked(commands.readAttachedFile).mockResolvedValue({
      data: "ZmFrZS1pbWFnZS1ieXRlcw==",
      mimeType: "image/png",
      name: "photo.png",
    });

    renderInput();
    await userEvent.click(screen.getByTestId("rich-input-attach"));

    await waitFor(() => {
      expect(open).toHaveBeenCalledWith({
        multiple: false,
        directory: false,
        filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
      });
    });
    expect(commands.readAttachedFile).toHaveBeenCalledWith("/Users/tester/photo.png");

    const chip = await screen.findByTestId("attachment-chip");
    expect(chip).toHaveTextContent("photo.png");
  });

  it("cancelling the dialog (open() resolves null) is a no-op — no readAttachedFile call, no chip, no error", async () => {
    vi.mocked(open).mockResolvedValue(null);

    renderInput();
    await userEvent.click(screen.getByTestId("rich-input-attach"));

    await waitFor(() => expect(open).toHaveBeenCalledTimes(1));
    expect(commands.readAttachedFile).not.toHaveBeenCalled();
    expect(screen.queryByTestId("attachment-chip")).not.toBeInTheDocument();
    expect(screen.queryByTestId("rich-input-attachment-error")).not.toBeInTheDocument();
  });

  it("an oversized file returned by readAttachedFile is rejected with an inline error, no chip inserted", async () => {
    vi.mocked(open).mockResolvedValue("/Users/tester/big.png");
    // ~10MB + a bit of base64 payload (length chosen so the decoded byte
    // count exceeds ATTACHMENT_MAX_BYTES).
    const oversizedBase64 = "A".repeat(Math.ceil(((10 * 1024 * 1024 + 4096) * 4) / 3));
    vi.mocked(commands.readAttachedFile).mockResolvedValue({
      data: oversizedBase64,
      mimeType: "image/png",
      name: "big.png",
    });

    renderInput();
    await userEvent.click(screen.getByTestId("rich-input-attach"));

    await waitFor(() => {
      expect(screen.getByTestId("rich-input-attachment-error")).toHaveTextContent(
        /10MB attachment limit/,
      );
    });
    expect(screen.queryByTestId("attachment-chip")).not.toBeInTheDocument();
  });
});

describe("RichInput (009-rich-chat-input, US4 — native drag-and-drop)", () => {
  it("a real Tauri drop event (via onDragDropEvent, not a DOM dataTransfer) reads the dropped path via readAttachedFile and inserts a chip", async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    let dropHandler: ((event: { payload: unknown }) => void) | undefined;
    const unlisten = vi.fn();
    vi.mocked(getCurrentWebview).mockReturnValue({
      onDragDropEvent: vi.fn((handler: (event: { payload: unknown }) => void) => {
        dropHandler = handler;
        return Promise.resolve(unlisten);
      }),
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
    } as any);
    vi.mocked(commands.readAttachedFile).mockResolvedValue({
      data: "ZmFrZS1pbWFnZS1ieXRlcw==",
      mimeType: "image/png",
      name: "dropped.png",
    });

    renderInput();

    await waitFor(() => expect(dropHandler).toBeDefined());
    dropHandler!({
      payload: { type: "drop", paths: ["/Users/tester/dropped.png"], position: { x: 0, y: 0 } },
    });

    expect(await screen.findByTestId("attachment-chip")).toHaveTextContent("dropped.png");
    expect(commands.readAttachedFile).toHaveBeenCalledWith("/Users/tester/dropped.png");
  });

  it("a non-'drop' drag event (e.g. 'over') is ignored — no readAttachedFile call", async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    let dropHandler: ((event: { payload: unknown }) => void) | undefined;
    vi.mocked(getCurrentWebview).mockReturnValue({
      onDragDropEvent: vi.fn((handler: (event: { payload: unknown }) => void) => {
        dropHandler = handler;
        return Promise.resolve(vi.fn());
      }),
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
    } as any);

    renderInput();

    await waitFor(() => expect(dropHandler).toBeDefined());
    dropHandler!({ payload: { type: "over", position: { x: 0, y: 0 } } });

    expect(commands.readAttachedFile).not.toHaveBeenCalled();
  });

  it("outside of a real Tauri webview (isTauri() false), getCurrentWebview is never called", () => {
    renderInput();
    expect(getCurrentWebview).not.toHaveBeenCalled();
  });
});

describe("RichInput (009-rich-chat-input, US4 — submit wiring)", () => {
  it("submitting a message with an attachment chip produces a richContent containing an 'attachment' segment", async () => {
    vi.mocked(open).mockResolvedValue("/Users/tester/photo.png");
    vi.mocked(commands.readAttachedFile).mockResolvedValue({
      data: "ZmFrZS1pbWFnZS1ieXRlcw==",
      mimeType: "image/png",
      name: "photo.png",
    });

    const onSubmit = renderInput();
    await userEvent.click(screen.getByTestId("rich-input-attach"));
    await screen.findByTestId("attachment-chip");

    // The attach button (not the editable) has focus after the click
    // above — refocus the editor before Enter, same as every other
    // submit-via-keyboard test in this suite/RichInput.test.tsx.
    await userEvent.click(screen.getByTestId("test-input"));
    await userEvent.keyboard("{Enter}");

    expect(onSubmit).toHaveBeenCalledTimes(1);
    const [, richContent] = onSubmit.mock.calls[0];
    expect(richContent).toBeDefined();
    expect(richContent!.segments).toEqual([
      {
        type: "attachment",
        id: expect.any(String),
        name: "photo.png",
        mimeType: "image/png",
        data: "ZmFrZS1pbWFnZS1ieXRlcw==",
        isImage: true,
      },
    ]);
  });
});
