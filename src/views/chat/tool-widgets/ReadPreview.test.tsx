import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import ReadPreview, { readPreviewKind } from "./ReadPreview";
import type { ReadDetail } from "@/lib/ipc";
import { commands } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  commands: {
    readAttachedFile: vi.fn(),
  },
}));

function readDetail(filePath: string | null, content = "hello world"): ReadDetail {
  return {
    toolName: "Read",
    filePath,
    offset: null,
    limit: null,
    outcome: { ok: true, content, truncated: false },
  };
}

describe("ReadPreview", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("classifies supported preview kinds from file extension", () => {
    expect(readPreviewKind("/tmp/notes.txt")).toBe("text");
    expect(readPreviewKind("/tmp/README.md")).toBe("markdown");
    expect(readPreviewKind("/tmp/photo.png")).toBe("native");
    expect(readPreviewKind("/tmp/movie.mp4")).toBe("native");
    expect(readPreviewKind("/tmp/sound.mp3")).toBe("native");
    expect(readPreviewKind("/tmp/sound.ogg")).toBe("native");
    expect(readPreviewKind("/tmp/archive.zip")).toBe("unsupported");
    expect(readPreviewKind(null)).toBe("unsupported");
  });

  it("renders captured content for text-like files without reading from disk", () => {
    render(<ReadPreview detail={readDetail("/tmp/notes.txt", "captured text")} />);

    const preview = screen.getByTestId("read-text-preview");
    expect(preview).toHaveTextContent("captured text");
    expect(preview).toHaveAttribute("data-slot", "code-block");
    expect(commands.readAttachedFile).not.toHaveBeenCalled();
  });

  it("renders markdown files with the shared markdown renderer", () => {
    render(<ReadPreview detail={readDetail("/tmp/README.md", "## Title")} />);

    expect(screen.getByRole("heading", { level: 2, name: "Title" })).toBeInTheDocument();
    expect(commands.readAttachedFile).not.toHaveBeenCalled();
  });

  it("shows a loading spinner while a native preview is being read from disk", () => {
    vi.mocked(commands.readAttachedFile).mockReturnValue(new Promise(() => {}));

    render(<ReadPreview detail={readDetail("/tmp/photo.png")} />);

    const loading = screen.getByTestId("read-preview-loading");
    expect(loading.querySelector('[data-slot="spinner"]')).toBeInTheDocument();
    expect(loading).toHaveTextContent("Loading preview…");
  });

  it("loads and renders image previews from disk", async () => {
    vi.mocked(commands.readAttachedFile).mockResolvedValue({
      data: btoa("fake png bytes"),
      mimeType: "image/png",
      name: "photo.png",
    });

    render(<ReadPreview detail={readDetail("/tmp/photo.png")} />);

    await waitFor(() => expect(commands.readAttachedFile).toHaveBeenCalledWith("/tmp/photo.png"));
    const image = await screen.findByTestId("read-image-preview");
    expect(image).toHaveAttribute("src", "data:image/png;base64,ZmFrZSBwbmcgYnl0ZXM=");
    expect(image).toHaveAttribute("alt", "photo.png");
  });

  it("loads and renders video previews from disk", async () => {
    vi.mocked(commands.readAttachedFile).mockResolvedValue({
      data: btoa("fake video bytes"),
      mimeType: "video/mp4",
      name: "movie.mp4",
    });

    render(<ReadPreview detail={readDetail("/tmp/movie.mp4")} />);

    await waitFor(() => expect(commands.readAttachedFile).toHaveBeenCalledWith("/tmp/movie.mp4"));
    expect(await screen.findByTestId("read-video-preview")).toHaveAttribute("controls");
  });

  it("loads and renders audio previews from disk", async () => {
    vi.mocked(commands.readAttachedFile).mockResolvedValue({
      data: btoa("fake audio bytes"),
      mimeType: "audio/ogg",
      name: "sound.ogg",
    });

    render(<ReadPreview detail={readDetail("/tmp/sound.ogg")} />);

    await waitFor(() => expect(commands.readAttachedFile).toHaveBeenCalledWith("/tmp/sound.ogg"));
    expect(await screen.findByTestId("read-audio-preview")).toHaveAttribute("controls");
  });

  it("renders preview unavailable for unsupported file types", () => {
    render(<ReadPreview detail={readDetail("/tmp/archive.zip")} />);

    const unavailable = screen.getByTestId("read-preview-unavailable");
    expect(unavailable).toHaveTextContent("Preview unavailable");
    expect(unavailable).toHaveAttribute("data-slot", "empty");
    expect(commands.readAttachedFile).not.toHaveBeenCalled();
  });

  it("renders an inline preview error when native preview loading fails", async () => {
    vi.mocked(commands.readAttachedFile).mockRejectedValue(new Error("failed to read file"));

    render(<ReadPreview detail={readDetail("/tmp/photo.png")} />);

    const error = await screen.findByTestId("read-preview-error");
    expect(error).toHaveTextContent("failed to read file");
    expect(error).toHaveAttribute("data-slot", "empty");
  });
});
