# Collapsible Read/Search Widgets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render successful Read, Grep, and Glob tool results as collapsed disclosure widgets with inline, height-limited previews.

**Architecture:** Add one shared native `<details>/<summary>` shell (`ToolDisclosure`) and keep tool-specific rendering in each widget. Read previews use a hybrid strategy: captured `outcome.content` for text/markdown, and `commands.readAttachedFile` for browser-native media previews. Search previews move current Grep/Glob result lists into the shared disclosure body while interrupted results remain distinct.

**Tech Stack:** React 19, TypeScript, Vitest + Testing Library + user-event, Tauri/Rust, Phosphor icons, existing `commands.readAttachedFile`, `ReactMarkdown`, `formatByteCount`, and `formatTokenCount`.

## Global Constraints

- Do not change the backend `ReadDetail`, `GrepDetail`, or `GlobDetail` result JSON shape.
- Do not add new IPC commands.
- Do not change model-facing Read/Grep/Glob tool behavior.
- Do not change Bash, Write, Edit, Task, AskUserQuestion, or UnknownTool widgets.
- Successful Read, Grep, and Glob results render collapsed by default.
- Use native `<details>` / `<summary>` semantics for the shared shell.
- Expanded disclosure bodies use `max-h-80 overflow-y-auto`.
- Offloaded and truncated successful Reads must not render separate rows, labels, badges, or different collapsed styling.
- Read text previews use captured `detail.outcome.content`.
- Read native previews load bytes through existing `commands.readAttachedFile(detail.filePath)`.
- `offloadedTo` remains persisted data only in this pass; it does not create visible UI or change preview source.
- Failed Read results and interrupted search results remain visibly distinct and non-disclosure.
- No syntax highlighting, full file browser/sidebar, file editing, persisted disclosure state, or automatic full-text fetching for every text file.
- Preserve unrelated worktree changes. Stage and commit only the files listed in each task.

---

## File Structure

- Create `src/views/chat/tool-widgets/ToolDisclosure.tsx`: shared disclosure shell with summary row, right chevron, and max-height scroll body.
- Create `src/views/chat/tool-widgets/ToolDisclosure.test.tsx`: interaction and styling coverage for the shared shell.
- Create `src/components/MarkdownPreview.tsx`: small markdown renderer wrapper reused by `MessageContent` and Read previews.
- Modify `src/components/MessageContent.tsx`: replace direct `ReactMarkdown` usage with `MarkdownPreview`.
- Modify `src/components/MessageContent.test.tsx`: preserve existing markdown behavior after extraction.
- Create `src/views/chat/tool-widgets/ReadPreview.tsx`: Read expanded-body renderer and file-kind classification.
- Create `src/views/chat/tool-widgets/ReadPreview.test.tsx`: text, markdown, native media, unsupported, and preview-error coverage.
- Modify `src/views/chat/tool-widgets/ReadWidget.tsx`: successful branch uses `ToolDisclosure`; failure branch stays distinct.
- Modify `src/views/chat/tool-widgets/ReadWidget.test.tsx`: collapsed/default behavior, no offload/truncation UI, expansion behavior, failure behavior.
- Modify `src/views/chat/tool-widgets/SearchResultsWidget.tsx`: non-interrupted Grep/Glob use `ToolDisclosure`.
- Modify `src/views/chat/tool-widgets/SearchResultsWidget.test.tsx`: collapsed summaries, expansion, zero states, interrupted state.
- Modify `src-tauri/src/commands/attachments.rs`: extend MIME detection for supported native-preview media extensions.
- Modify `src/views/design-system/WidgetGallery.tsx`: examples reflect collapsed/expandable Read/Grep/Glob behavior.
- Modify `src/views/design-system/WidgetGallery.test.tsx`: gallery copy/labels guard.

---

### Task 1: Extend MIME Detection for Native Read Previews

**Files:**

- Modify: `src-tauri/src/commands/attachments.rs`

**Interfaces:**

- Consumes: `read_attached_file(path: String) -> Result<AttachedFile, String>`
- Produces: `detect_mime_type(path: &Path) -> String` mappings for `.svg`, `.mp4`, `.webm`, `.ogg`, `.mov`, `.mp3`, `.wav`, `.m4a`, and `.flac`

- [ ] **Step 1: Write failing MIME detection tests**

In `src-tauri/src/commands/attachments.rs`, inside the existing `#[cfg(test)] mod tests`, add this test after `reads_a_real_file_as_base64_with_detected_mime_and_basename`:

```rust
    #[test]
    fn detects_native_preview_mime_types_by_extension() {
        let cases = [
            ("diagram.svg", "image/svg+xml"),
            ("clip.mp4", "video/mp4"),
            ("clip.webm", "video/webm"),
            ("clip.ogg", "video/ogg"),
            ("clip.mov", "video/quicktime"),
            ("sound.mp3", "audio/mpeg"),
            ("sound.wav", "audio/wav"),
            ("sound.m4a", "audio/mp4"),
            ("sound.flac", "audio/flac"),
        ];

        for (name, expected_mime) in cases {
            let dir = tempdir().unwrap();
            let file_path = dir.path().join(name);
            fs::write(&file_path, b"preview bytes").unwrap();

            let result = read_attached_file(file_path.to_string_lossy().to_string()).unwrap();

            assert_eq!(result.mime_type, expected_mime, "wrong MIME for {name}");
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd src-tauri
cargo test commands::attachments::tests::detects_native_preview_mime_types_by_extension
```

Expected: FAIL because these extensions currently return `application/octet-stream`.

- [ ] **Step 3: Add the MIME mappings**

In `src-tauri/src/commands/attachments.rs`, replace the `match extension.as_deref()` body in `detect_mime_type` with:

```rust
    match extension.as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ogg") => "video/ogg",
        Some("mov") => "video/quicktime",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("m4a") => "audio/mp4",
        Some("flac") => "audio/flac",
        _ => "application/octet-stream",
    }
    .to_string()
```

- [ ] **Step 4: Run attachment command tests**

Run:

```bash
cd src-tauri
cargo test commands::attachments::tests
```

Expected: PASS, including the new MIME mapping test and existing fallback test.

- [ ] **Step 5: Commit Task 1**

Run:

```bash
git add src-tauri/src/commands/attachments.rs
git commit -m "feat(widgets): detect preview media mime types"
```

Expected: one commit containing only `src-tauri/src/commands/attachments.rs`.

---

### Task 2: Add Shared `ToolDisclosure`

**Files:**

- Create: `src/views/chat/tool-widgets/ToolDisclosure.tsx`
- Create: `src/views/chat/tool-widgets/ToolDisclosure.test.tsx`

**Interfaces:**

- Produces: `ToolDisclosure(props: ToolDisclosureProps): JSX.Element`
- `ToolDisclosureProps.summary: React.ReactNode`
- `ToolDisclosureProps.children: React.ReactNode`
- `ToolDisclosureProps.testId?: string`
- `ToolDisclosureProps.summaryTestId?: string`
- `ToolDisclosureProps.bodyTestId?: string`
- `ToolDisclosureProps.bodyClassName?: string`

- [ ] **Step 1: Write failing disclosure tests**

Create `src/views/chat/tool-widgets/ToolDisclosure.test.tsx`:

```tsx
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ToolDisclosure from "./ToolDisclosure";

describe("ToolDisclosure", () => {
  it("renders collapsed by default and expands inline when the summary is clicked", async () => {
    render(
      <ToolDisclosure
        summary={<span>Read src/App.tsx · 120B</span>}
        testId="tool-disclosure"
        summaryTestId="tool-summary"
        bodyTestId="tool-body"
      >
        <p>expanded preview</p>
      </ToolDisclosure>,
    );

    const disclosure = screen.getByTestId("tool-disclosure");
    expect(disclosure).not.toHaveAttribute("open");
    expect(screen.getByTestId("tool-summary")).toHaveTextContent("Read src/App.tsx");
    expect(screen.queryByTestId("tool-body")).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId("tool-summary"));

    expect(disclosure).toHaveAttribute("open");
    expect(screen.getByTestId("tool-body")).toHaveTextContent("expanded preview");
  });

  it("renders a right-side decorative chevron and height-limited body", async () => {
    render(
      <ToolDisclosure
        summary={<span>Glob *.tsx · 3 files</span>}
        testId="tool-disclosure"
        summaryTestId="tool-summary"
        bodyTestId="tool-body"
      >
        <p>file list</p>
      </ToolDisclosure>,
    );

    await userEvent.click(screen.getByTestId("tool-summary"));

    expect(screen.getByTestId("tool-disclosure-chevron")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByTestId("tool-body")).toHaveClass("max-h-80");
    expect(screen.getByTestId("tool-body")).toHaveClass("overflow-y-auto");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/ToolDisclosure.test.tsx
```

Expected: FAIL because `ToolDisclosure.tsx` does not exist.

- [ ] **Step 3: Implement `ToolDisclosure`**

Create `src/views/chat/tool-widgets/ToolDisclosure.tsx`:

```tsx
import { useState, type ReactNode } from "react";
import { CaretRightIcon } from "@phosphor-icons/react";
import { cn } from "@/lib/cn";

interface ToolDisclosureProps {
  summary: ReactNode;
  children: ReactNode;
  testId?: string;
  summaryTestId?: string;
  bodyTestId?: string;
  bodyClassName?: string;
}

export default function ToolDisclosure({
  summary,
  children,
  testId,
  summaryTestId,
  bodyTestId,
  bodyClassName,
}: ToolDisclosureProps) {
  const [open, setOpen] = useState(false);

  return (
    <details
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
      className="group rounded-lg border border-border bg-card text-sm [&>summary::-webkit-details-marker]:hidden"
      data-testid={testId}
    >
      <summary
        className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 focus-visible:outline-offset-[-2px]"
        data-testid={summaryTestId}
      >
        <span className="min-w-0 flex-1 truncate">{summary}</span>
        <CaretRightIcon
          size={14}
          aria-hidden="true"
          data-testid="tool-disclosure-chevron"
          className="shrink-0 text-muted-foreground transition-transform group-open:rotate-90"
        />
      </summary>
      {open && (
        <div
          className={cn("max-h-80 overflow-y-auto border-t border-border p-3", bodyClassName)}
          data-testid={bodyTestId}
        >
          {children}
        </div>
      )}
    </details>
  );
}
```

- [ ] **Step 4: Run disclosure tests**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/ToolDisclosure.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit Task 2**

Run:

```bash
git add src/views/chat/tool-widgets/ToolDisclosure.tsx src/views/chat/tool-widgets/ToolDisclosure.test.tsx
git commit -m "feat(widgets): add tool disclosure shell"
```

Expected: one commit containing only the shared disclosure component and test.

---

### Task 3: Extract Reusable Markdown Preview

**Files:**

- Create: `src/components/MarkdownPreview.tsx`
- Modify: `src/components/MessageContent.tsx`
- Modify: `src/components/MessageContent.test.tsx`

**Interfaces:**

- Produces: `MarkdownPreview({ children, className }: { children: string; className?: string })`
- Consumes: `ReactMarkdown`
- Later tasks import `MarkdownPreview` for markdown file previews.

- [ ] **Step 1: Add markdown preservation test**

In `src/components/MessageContent.test.tsx`, add this test after `renders a text assistant message as markdown, with Timer only when showTimer is true`:

```tsx
it("continues to render markdown after the markdown renderer is shared", () => {
  render(
    <MessageContent
      message={baseMessage({
        contentType: "text",
        content: "## Heading\n\n- one\n- two",
      })}
    />,
  );

  expect(screen.getByRole("heading", { level: 2, name: "Heading" })).toBeInTheDocument();
  expect(screen.getByText("one")).toBeInTheDocument();
  expect(screen.getByText("two")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run test before extraction**

Run:

```bash
npx vitest run src/components/MessageContent.test.tsx
```

Expected: PASS. This test locks existing behavior before extraction.

- [ ] **Step 3: Create shared markdown renderer**

Create `src/components/MarkdownPreview.tsx`:

```tsx
import ReactMarkdown from "react-markdown";
import { cn } from "@/lib/cn";

interface MarkdownPreviewProps {
  children: string;
  className?: string;
}

export default function MarkdownPreview({ children, className }: MarkdownPreviewProps) {
  return (
    <div className={cn("prose prose-sm dark:prose-invert max-w-none", className)}>
      <ReactMarkdown>{children}</ReactMarkdown>
    </div>
  );
}
```

- [ ] **Step 4: Use `MarkdownPreview` from `MessageContent`**

In `src/components/MessageContent.tsx`, remove:

```tsx
import ReactMarkdown from "react-markdown";
```

Add:

```tsx
import MarkdownPreview from "@/components/MarkdownPreview";
```

Replace the current user-message bubble wrapper:

```tsx
<div className="prose prose-sm dark:prose-invert max-w-none rounded-lg bg-muted p-3 text-foreground">
  {/* 009-rich-chat-input, US2 (T026): a rich_text user message (a
              paste-collapse chip, and eventually attachment/skill chips)
              dispatches to UserMessageContent, mirroring this file's existing
              tool_result -> ToolWidget dispatch — every other user message
              (contentType 'text', today's only other case) renders exactly as
              it always has. */}
  {m.contentType === "rich_text" ? (
    <UserMessageContent content={m.content} />
  ) : (
    <ReactMarkdown>{m.content}</ReactMarkdown>
  )}
</div>
```

with:

```tsx
{
  m.contentType === "rich_text" ? (
    <div className="prose prose-sm dark:prose-invert max-w-none rounded-lg bg-muted p-3 text-foreground">
      {/* 009-rich-chat-input, US2 (T026): a rich_text user message (a
                paste-collapse chip, and eventually attachment/skill chips)
                dispatches to UserMessageContent, mirroring this file's existing
                tool_result -> ToolWidget dispatch — every other user message
                (contentType 'text', today's only other case) renders exactly as
                it always has. */}
      <UserMessageContent content={m.content} />
    </div>
  ) : (
    <MarkdownPreview className="rounded-lg bg-muted p-3 text-foreground">
      {m.content}
    </MarkdownPreview>
  );
}
```

Replace the assistant-message text wrapper:

```tsx
<div className="prose prose-sm dark:prose-invert max-w-none">
  <ReactMarkdown>{m.content}</ReactMarkdown>
</div>
```

with:

```tsx
<MarkdownPreview>{m.content}</MarkdownPreview>
```

- [ ] **Step 5: Run message content tests**

Run:

```bash
npx vitest run src/components/MessageContent.test.tsx
```

Expected: PASS.

- [ ] **Step 6: Commit Task 3**

Run:

```bash
git add src/components/MarkdownPreview.tsx src/components/MessageContent.tsx src/components/MessageContent.test.tsx
git commit -m "refactor(chat): share markdown preview renderer"
```

Expected: one commit containing only the markdown helper and `MessageContent` usage.

---

### Task 4: Add `ReadPreview` and Convert `ReadWidget`

**Files:**

- Create: `src/views/chat/tool-widgets/ReadPreview.tsx`
- Create: `src/views/chat/tool-widgets/ReadPreview.test.tsx`
- Modify: `src/views/chat/tool-widgets/ReadWidget.tsx`
- Modify: `src/views/chat/tool-widgets/ReadWidget.test.tsx`

**Interfaces:**

- Consumes: `ToolDisclosure` from Task 2.
- Consumes: `MarkdownPreview` from Task 3.
- Consumes: `commands.readAttachedFile(path: string)`.
- Produces: `ReadPreview({ detail }: { detail: ReadDetail })`.
- Produces: `readPreviewKind(filePath: string | null): "text" | "markdown" | "native" | "unsupported"`.

- [ ] **Step 1: Write failing `ReadPreview` tests**

Create `src/views/chat/tool-widgets/ReadPreview.test.tsx`:

```tsx
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

    expect(screen.getByTestId("read-text-preview")).toHaveTextContent("captured text");
    expect(commands.readAttachedFile).not.toHaveBeenCalled();
  });

  it("renders markdown files with the shared markdown renderer", () => {
    render(<ReadPreview detail={readDetail("/tmp/README.md", "## Title")} />);

    expect(screen.getByRole("heading", { level: 2, name: "Title" })).toBeInTheDocument();
    expect(commands.readAttachedFile).not.toHaveBeenCalled();
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

    expect(screen.getByTestId("read-preview-unavailable")).toHaveTextContent("Preview unavailable");
    expect(commands.readAttachedFile).not.toHaveBeenCalled();
  });

  it("renders an inline preview error when native preview loading fails", async () => {
    vi.mocked(commands.readAttachedFile).mockRejectedValue(new Error("failed to read file"));

    render(<ReadPreview detail={readDetail("/tmp/photo.png")} />);

    expect(await screen.findByTestId("read-preview-error")).toHaveTextContent(
      "failed to read file",
    );
  });
});
```

- [ ] **Step 2: Run `ReadPreview` tests and verify they fail**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/ReadPreview.test.tsx
```

Expected: FAIL because `ReadPreview.tsx` does not exist.

- [ ] **Step 3: Implement `ReadPreview`**

Create `src/views/chat/tool-widgets/ReadPreview.tsx`:

```tsx
import { useEffect, useState } from "react";
import MarkdownPreview from "@/components/MarkdownPreview";
import { commands } from "@/lib/ipc";
import type { ReadDetail } from "@/lib/ipc";

type PreviewKind = "text" | "markdown" | "native" | "unsupported";

const TEXT_EXTENSIONS = new Set([
  "txt",
  "json",
  "yaml",
  "yml",
  "toml",
  "rs",
  "ts",
  "tsx",
  "js",
  "jsx",
  "css",
  "html",
  "py",
  "sh",
  "sql",
  "xml",
  "csv",
  "log",
  "ini",
  "env",
  "go",
  "java",
  "c",
  "cpp",
  "h",
  "hpp",
  "swift",
  "kt",
  "rb",
  "php",
  "vue",
]);

const MARKDOWN_EXTENSIONS = new Set(["md", "markdown", "mdx"]);
const NATIVE_PREVIEW_EXTENSIONS = new Set([
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "svg",
  "mp4",
  "webm",
  "ogg",
  "mov",
  "mp3",
  "wav",
  "m4a",
  "flac",
]);

function extensionFor(filePath: string | null): string | null {
  if (!filePath) return null;
  const name = filePath.split(/[\\/]/).pop() ?? filePath;
  const dot = name.lastIndexOf(".");
  if (dot < 0 || dot === name.length - 1) return null;
  return name.slice(dot + 1).toLowerCase();
}

export function readPreviewKind(filePath: string | null): PreviewKind {
  const extension = extensionFor(filePath);
  if (!extension) return "unsupported";
  if (MARKDOWN_EXTENSIONS.has(extension)) return "markdown";
  if (TEXT_EXTENSIONS.has(extension)) return "text";
  if (NATIVE_PREVIEW_EXTENSIONS.has(extension)) return "native";
  return "unsupported";
}

interface ReadPreviewProps {
  detail: ReadDetail;
}

type NativeFileState =
  | { status: "loading" }
  | { status: "loaded"; dataUrl: string; mimeType: string; name: string }
  | { status: "error"; error: string };

export default function ReadPreview({ detail }: ReadPreviewProps) {
  if (!detail.outcome.ok) return null;

  const kind = readPreviewKind(detail.filePath);

  if (kind === "markdown") {
    return (
      <div data-testid="read-markdown-preview">
        <MarkdownPreview>{detail.outcome.content}</MarkdownPreview>
      </div>
    );
  }

  if (kind === "text") {
    return (
      <pre
        className="whitespace-pre-wrap break-words font-mono text-xs"
        data-testid="read-text-preview"
      >
        {detail.outcome.content}
      </pre>
    );
  }

  if (kind === "native" && detail.filePath) {
    return <NativeReadPreview path={detail.filePath} />;
  }

  return <PreviewUnavailable filePath={detail.filePath} />;
}

function NativeReadPreview({ path }: { path: string }) {
  const [state, setState] = useState<NativeFileState>({ status: "loading" });

  useEffect(() => {
    let cancelled = false;
    setState({ status: "loading" });
    commands
      .readAttachedFile(path)
      .then((file) => {
        if (cancelled) return;
        setState({
          status: "loaded",
          dataUrl: `data:${file.mimeType};base64,${file.data}`,
          mimeType: file.mimeType,
          name: file.name,
        });
      })
      .catch((error) => {
        if (cancelled) return;
        setState({ status: "error", error: String(error) });
      });
    return () => {
      cancelled = true;
    };
  }, [path]);

  if (state.status === "loading") {
    return (
      <p className="text-xs text-muted-foreground" data-testid="read-preview-loading">
        Loading preview…
      </p>
    );
  }

  if (state.status === "error") {
    return (
      <p className="text-xs text-destructive" data-testid="read-preview-error">
        {state.error}
      </p>
    );
  }

  const mediaKind = nativeMediaKind(state.mimeType);

  if (mediaKind === "image") {
    return (
      <img
        src={state.dataUrl}
        alt={state.name}
        className="max-h-80 max-w-full rounded-md object-contain"
        data-testid="read-image-preview"
      />
    );
  }

  if (mediaKind === "video") {
    return (
      <video
        src={state.dataUrl}
        controls
        className="max-h-80 w-full rounded-md"
        data-testid="read-video-preview"
      >
        {state.name}
      </video>
    );
  }

  if (mediaKind === "audio") {
    return (
      <audio src={state.dataUrl} controls className="w-full" data-testid="read-audio-preview">
        {state.name}
      </audio>
    );
  }

  return <PreviewUnavailable filePath={path} />;
}

function nativeMediaKind(mimeType: string): "image" | "video" | "audio" | null {
  if (mimeType.startsWith("image/")) return "image";
  if (mimeType.startsWith("video/")) return "video";
  if (mimeType.startsWith("audio/")) return "audio";
  return null;
}

function PreviewUnavailable({ filePath }: { filePath: string | null }) {
  return (
    <p className="text-xs text-muted-foreground" data-testid="read-preview-unavailable">
      Preview unavailable{filePath ? ` for ${filePath}` : ""}
    </p>
  );
}
```

- [ ] **Step 4: Run `ReadPreview` tests**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/ReadPreview.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Replace `ReadWidget` success branch tests**

Replace `src/views/chat/tool-widgets/ReadWidget.test.tsx` with:

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ReadWidget from "./ReadWidget";
import type { ReadDetail } from "@/lib/ipc";

describe("ReadWidget (004-tool-call-widgets, US4)", () => {
  it("renders successful reads collapsed with path, bytes, tokens, and a chevron", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
      tokenCount: 312,
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-widget")).not.toHaveAttribute("open");
    expect(screen.getByTestId("read-summary")).toHaveTextContent(
      "Read /tmp/notes.txt · 11B · 312 tok",
    );
    expect(screen.getByTestId("tool-disclosure-chevron")).toBeInTheDocument();
    expect(screen.queryByTestId("read-preview")).not.toBeInTheDocument();
  });

  it("expands inline to show captured text preview", async () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "captured text", truncated: false },
      tokenCount: 20,
    };

    render(<ReadWidget detail={detail} />);
    await userEvent.click(screen.getByTestId("read-summary"));

    expect(screen.getByTestId("read-preview")).toHaveClass("max-h-80");
    expect(screen.getByTestId("read-text-preview")).toHaveTextContent("captured text");
  });

  it("does not present truncation as a separate visible state", async () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/big.txt",
      offset: null,
      limit: 2000,
      outcome: { ok: true, content: "a lot of content", truncated: true },
      tokenCount: 42,
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.queryByTestId("read-truncated")).not.toBeInTheDocument();
    expect(screen.queryByText("Output truncated")).not.toBeInTheDocument();
    expect(screen.getByTestId("read-summary")).toHaveTextContent(
      "Read /tmp/big.txt · 16B · 42 tok",
    );
    await userEvent.click(screen.getByTestId("read-summary"));
    expect(screen.queryByText("Output truncated")).not.toBeInTheDocument();
  });

  it("does not present offload as a separate visible state", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/huge.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "preview only...", truncated: true },
      tokenCount: 2048,
      offloadedTo: "/data/tool-outputs/conv1/call1.txt",
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-summary")).toHaveTextContent(
      "Read /tmp/huge.txt · 15B · 2.0k tok",
    );
    expect(screen.queryByTestId("view-full-output-button")).not.toBeInTheDocument();
    expect(screen.queryByText("View full output")).not.toBeInTheDocument();
  });

  it("renders byte metadata and omits only the token segment for older rows without tokenCount", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/legacy.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-summary")).toHaveTextContent("Read /tmp/legacy.txt · 11B");
    expect(screen.getByTestId("read-summary")).not.toHaveTextContent("tok");
  });

  it("renders a failure state distinctly and not as a disclosure", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/missing.txt",
      offset: null,
      limit: null,
      outcome: { ok: false, error: "No such file or directory" },
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-widget")).toHaveClass("border-destructive/40");
    expect(screen.getByText(/No such file or directory/)).toBeInTheDocument();
    expect(screen.queryByTestId("read-summary")).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 6: Run `ReadWidget` tests and verify they fail**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/ReadWidget.test.tsx
```

Expected: FAIL because `ReadWidget` still renders a plain card and not `ToolDisclosure`.

- [ ] **Step 7: Convert `ReadWidget` success branch**

Replace `src/views/chat/tool-widgets/ReadWidget.tsx` with:

```tsx
import type { ReadDetail } from "@/lib/ipc";
import { formatByteCount } from "@/lib/formatByteCount";
import { formatTokenCount } from "@/lib/formatTokenCount";
import ToolDisclosure from "./ToolDisclosure";
import ReadPreview from "./ReadPreview";

interface ReadWidgetProps {
  detail: ReadDetail;
}

/** US4/FR-005: a compact file-reference card, not a plain-text dump of the file's contents. */
export default function ReadWidget({ detail }: ReadWidgetProps) {
  if (!detail.outcome.ok) {
    return (
      <div
        className="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm"
        data-testid="read-widget"
      >
        <p className="mb-1 font-mono text-xs text-muted-foreground">
          Read <span>{detail.filePath}</span>
        </p>
        <p className="text-destructive">{detail.outcome.error}</p>
      </div>
    );
  }

  const byteCount = formatByteCount(detail.outcome.content.length);
  const tokenCount =
    detail.tokenCount != null ? `${formatTokenCount(detail.tokenCount)} tok` : null;

  return (
    <ToolDisclosure
      testId="read-widget"
      summaryTestId="read-summary"
      bodyTestId="read-preview"
      summary={
        <span className="font-mono text-xs text-muted-foreground">
          Read <span>{detail.filePath}</span> · {byteCount}
          {tokenCount != null && <> · {tokenCount}</>}
        </span>
      }
    >
      <ReadPreview detail={detail} />
    </ToolDisclosure>
  );
}
```

- [ ] **Step 8: Run Read preview/widget tests**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/ReadPreview.test.tsx src/views/chat/tool-widgets/ReadWidget.test.tsx
```

Expected: PASS.

- [ ] **Step 9: Commit Task 4**

Run:

```bash
git add src/views/chat/tool-widgets/ReadPreview.tsx src/views/chat/tool-widgets/ReadPreview.test.tsx src/views/chat/tool-widgets/ReadWidget.tsx src/views/chat/tool-widgets/ReadWidget.test.tsx
git commit -m "feat(widgets): add collapsible read previews"
```

Expected: one commit containing only Read preview and Read widget files.

---

### Task 5: Convert Grep/Glob to `ToolDisclosure`

**Files:**

- Modify: `src/views/chat/tool-widgets/SearchResultsWidget.tsx`
- Modify: `src/views/chat/tool-widgets/SearchResultsWidget.test.tsx`

**Interfaces:**

- Consumes: `ToolDisclosure` from Task 2.
- Consumes: `GlobDetail`, `GrepDetail`.
- Produces: non-interrupted `SearchResultsWidget` results collapsed by default.

- [ ] **Step 1: Replace SearchResultsWidget tests**

Replace `src/views/chat/tool-widgets/SearchResultsWidget.test.tsx` with:

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import SearchResultsWidget from "./SearchResultsWidget";
import type { GlobDetail, GrepDetail } from "@/lib/ipc";

describe("SearchResultsWidget (004-tool-call-widgets, US4: Glob + Grep)", () => {
  it("renders Glob collapsed with file count and expands to show file list", async () => {
    const detail: GlobDetail = {
      toolName: "Glob",
      pattern: "*.rs",
      path: "/tmp/project",
      matches: ["/tmp/project/a.rs", "/tmp/project/b.rs"],
      tokenCount: 42,
    };

    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-widget")).not.toHaveAttribute("open");
    expect(screen.getByTestId("search-summary")).toHaveTextContent("Glob *.rs · 2 files · 42 tok");
    expect(screen.queryByTestId("search-match")).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId("search-summary"));

    expect(screen.getByTestId("search-results")).toHaveClass("max-h-80");
    expect(screen.getAllByTestId("search-match")).toHaveLength(2);
    expect(screen.getByText("/tmp/project/a.rs")).toBeInTheDocument();
    expect(screen.getByTestId("search-context")).toHaveTextContent("/tmp/project");
  });

  it("renders a collapsible zero-files state for Glob", async () => {
    const detail: GlobDetail = { toolName: "Glob", pattern: "*.nope", path: "/tmp", matches: [] };
    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-summary")).toHaveTextContent("Glob *.nope · 0 files");
    expect(screen.queryByTestId("search-no-matches")).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId("search-summary"));

    expect(screen.getByTestId("search-no-matches")).toHaveTextContent("No files matched");
  });

  it("renders Grep collapsed with match count and expands to show match list", async () => {
    const detail: GrepDetail = {
      toolName: "Grep",
      pattern: "TODO",
      path: "/tmp/project",
      glob: "*.rs",
      matches: [{ path: "/tmp/project/a.rs", lineNumber: 12, line: "// TODO: fix this" }],
      tokenCount: 99,
    };
    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-summary")).toHaveTextContent("Grep TODO · 1 match · 99 tok");
    expect(screen.queryByTestId("search-match")).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId("search-summary"));

    const match = screen.getByTestId("search-match");
    expect(match).toHaveTextContent("/tmp/project/a.rs:12: // TODO: fix this");
    expect(screen.getByTestId("search-context")).toHaveTextContent("/tmp/project");
    expect(screen.getByTestId("search-context")).toHaveTextContent("*.rs");
  });

  it("renders a collapsible zero-matches state for Grep", async () => {
    const detail: GrepDetail = {
      toolName: "Grep",
      pattern: "nonexistent",
      path: "/tmp",
      glob: null,
      matches: [],
    };
    render(<SearchResultsWidget detail={detail} />);

    expect(screen.getByTestId("search-summary")).toHaveTextContent("Grep nonexistent · 0 matches");
    expect(screen.queryByTestId("search-no-matches")).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId("search-summary"));

    expect(screen.getByTestId("search-no-matches")).toHaveTextContent("No matches found");
  });

  it("shows no token cost when tokenCount is absent", () => {
    const detail: GlobDetail = {
      toolName: "Glob",
      pattern: "*.rs",
      path: "/tmp/project",
      matches: ["/tmp/project/a.rs"],
    };
    render(<SearchResultsWidget detail={detail} />);
    expect(screen.getByTestId("search-summary")).not.toHaveTextContent("tok");
  });

  it("renders an interrupted notice — never a collapsed zero-result disclosure — for a healed crash-orphaned Grep", () => {
    const detail: GrepDetail = {
      toolName: "Grep",
      pattern: "needle",
      path: "/tmp/project",
      glob: null,
      matches: [],
      interrupted: true,
    };
    render(<SearchResultsWidget detail={detail} />);
    expect(screen.getByTestId("search-interrupted")).toHaveTextContent(/interrupted/i);
    expect(screen.queryByTestId("search-summary")).not.toBeInTheDocument();
    expect(screen.queryByTestId("search-no-matches")).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/SearchResultsWidget.test.tsx
```

Expected: FAIL because current search results render expanded inline by default.

- [ ] **Step 3: Convert `SearchResultsWidget`**

Replace `src/views/chat/tool-widgets/SearchResultsWidget.tsx` with:

```tsx
import type { GlobDetail, GrepDetail } from "@/lib/ipc";
import { formatTokenCount } from "@/lib/formatTokenCount";
import ToolDisclosure from "./ToolDisclosure";

interface SearchResultsWidgetProps {
  detail: GlobDetail | GrepDetail;
}

/** US4/FR-007: a match list for Glob (filenames) and Grep (file:line:content), not an undifferentiated data dump. */
export default function SearchResultsWidget({ detail }: SearchResultsWidgetProps) {
  const isGrep = detail.toolName === "Grep";

  if (detail.interrupted) {
    return (
      <div
        className="rounded-lg border border-border bg-card p-3 text-sm"
        data-testid="search-widget"
      >
        <p className="mb-1 font-mono text-xs text-muted-foreground">
          {detail.toolName} {detail.pattern}
          {detail.tokenCount != null && <span> · {formatTokenCount(detail.tokenCount)} tok</span>}
        </p>
        <p className="text-xs text-amber-600 dark:text-amber-400" data-testid="search-interrupted">
          Interrupted — the app closed before this search finished
        </p>
      </div>
    );
  }

  const count = detail.matches.length;
  const countLabel = isGrep ? `${count} ${count === 1 ? "match" : "matches"}` : `${count} files`;
  const tokenLabel =
    detail.tokenCount != null ? ` · ${formatTokenCount(detail.tokenCount)} tok` : "";

  return (
    <ToolDisclosure
      testId="search-widget"
      summaryTestId="search-summary"
      bodyTestId="search-results"
      summary={
        <span className="font-mono text-xs text-muted-foreground">
          {detail.toolName} {detail.pattern} · {countLabel}
          {tokenLabel}
        </span>
      }
      bodyClassName="space-y-2"
    >
      <SearchContext detail={detail} />
      {isGrep ? <GrepResults detail={detail} /> : <GlobResults detail={detail} />}
    </ToolDisclosure>
  );
}

function SearchContext({ detail }: { detail: GlobDetail | GrepDetail }) {
  const parts = [
    detail.path ? `path: ${detail.path}` : null,
    detail.toolName === "Grep" && detail.glob ? `glob: ${detail.glob}` : null,
  ].filter(Boolean);

  if (parts.length === 0) return null;

  return (
    <p className="font-mono text-xs text-muted-foreground" data-testid="search-context">
      {parts.join(" · ")}
    </p>
  );
}

function GlobResults({ detail }: { detail: GlobDetail }) {
  if (detail.matches.length === 0) {
    return (
      <p className="text-xs text-muted-foreground" data-testid="search-no-matches">
        No files matched
      </p>
    );
  }

  return (
    <ul className="space-y-0.5 font-mono text-xs">
      {detail.matches.map((path, i) => (
        <li key={i} data-testid="search-match" className="truncate">
          {path}
        </li>
      ))}
    </ul>
  );
}

function GrepResults({ detail }: { detail: GrepDetail }) {
  if (detail.matches.length === 0) {
    return (
      <p className="text-xs text-muted-foreground" data-testid="search-no-matches">
        No matches found
      </p>
    );
  }

  return (
    <ul className="space-y-0.5 font-mono text-xs">
      {detail.matches.map((m, i) => (
        <li key={i} data-testid="search-match" className="truncate">
          {m.path}:{m.lineNumber}: {m.line}
        </li>
      ))}
    </ul>
  );
}
```

- [ ] **Step 4: Run search widget tests**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/SearchResultsWidget.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit Task 5**

Run:

```bash
git add src/views/chat/tool-widgets/SearchResultsWidget.tsx src/views/chat/tool-widgets/SearchResultsWidget.test.tsx
git commit -m "feat(widgets): collapse search result lists"
```

Expected: one commit containing only `SearchResultsWidget.tsx` and its test.

---

### Task 6: Update Widget Gallery Examples

**Files:**

- Modify: `src/views/design-system/WidgetGallery.tsx`
- Modify: `src/views/design-system/WidgetGallery.test.tsx`

**Interfaces:**

- Consumes: updated `ReadWidget` and `SearchResultsWidget`.
- Produces: gallery labels/copy that describe collapsed/expandable behavior.

- [ ] **Step 1: Update gallery test**

Replace `src/views/design-system/WidgetGallery.test.tsx` with:

```tsx
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import WidgetGallery from "./WidgetGallery";

describe("WidgetGallery", () => {
  it("fills the shell content area instead of forcing viewport height", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(screen.getByTestId("widget-gallery")).toHaveClass("h-full");
    expect(screen.getByTestId("widget-gallery")).not.toHaveClass("h-dvh");
  });

  it("documents Read as collapsed expandable previews", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(
      screen.getByText("A collapsed file-reference card with inline expandable preview."),
    ).toBeInTheDocument();
    expect(screen.getByText("Text read")).toBeInTheDocument();
    expect(screen.getByText("Native preview candidate")).toBeInTheDocument();
    expect(screen.queryByText("Offloaded read")).not.toBeInTheDocument();
    expect(screen.queryByText("Truncated")).not.toBeInTheDocument();
  });

  it("documents search widgets as collapsed expandable result lists", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(
      screen.getByText("Collapsed search summaries with inline expandable result lists."),
    ).toBeInTheDocument();
    expect(screen.getByText("Glob, with files")).toBeInTheDocument();
    expect(screen.getByText("Glob, no files")).toBeInTheDocument();
    expect(screen.getByText("Grep, with matches")).toBeInTheDocument();
    expect(screen.getByText("Grep, no matches")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run gallery test and verify it fails**

Run:

```bash
npx vitest run src/views/design-system/WidgetGallery.test.tsx
```

Expected: FAIL because the gallery still uses old labels/descriptions.

- [ ] **Step 3: Update Read gallery section**

In `src/views/design-system/WidgetGallery.tsx`, replace the current Read `<Section>` block with:

```tsx
<Section title="Read" description="A collapsed file-reference card with inline expandable preview.">
  <Example label="Text read">
    <ReadWidget
      detail={{
        toolName: "Read",
        filePath: "src/agent/dispatch.rs",
        offset: null,
        limit: null,
        outcome: { ok: true, content: "pub fn execute(...", truncated: false },
        tokenCount: 312,
      }}
    />
  </Example>
  <Example label="Native preview candidate">
    <ReadWidget
      detail={{
        toolName: "Read",
        filePath: "diagram.svg",
        offset: null,
        limit: null,
        outcome: { ok: true, content: "(binary preview candidate)", truncated: false },
        tokenCount: 2048,
      }}
    />
  </Example>
  <Example label="Failure">
    <ReadWidget
      detail={{
        toolName: "Read",
        filePath: "does/not/exist.txt",
        offset: null,
        limit: null,
        outcome: { ok: false, error: "No such file or directory (os error 2)" },
      }}
    />
  </Example>
</Section>
```

- [ ] **Step 4: Update Search gallery section description and labels**

In the Search section of `src/views/design-system/WidgetGallery.tsx`, change the `<Section>` description to:

```tsx
description = "Collapsed search summaries with inline expandable result lists.";
```

Change the Glob labels:

```tsx
          <Example label="Glob, with files">
```

and:

```tsx
          <Example label="Glob, no files">
```

The existing Grep labels can remain `Grep, with matches` and `Grep, no matches`.

- [ ] **Step 5: Run gallery tests**

Run:

```bash
npx vitest run src/views/design-system/WidgetGallery.test.tsx
```

Expected: PASS.

- [ ] **Step 6: Run focused widget suite**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/ToolDisclosure.test.tsx src/views/chat/tool-widgets/ReadPreview.test.tsx src/views/chat/tool-widgets/ReadWidget.test.tsx src/views/chat/tool-widgets/SearchResultsWidget.test.tsx src/views/design-system/WidgetGallery.test.tsx
```

Expected: PASS.

- [ ] **Step 7: Commit Task 6**

Run:

```bash
git add src/views/design-system/WidgetGallery.tsx src/views/design-system/WidgetGallery.test.tsx
git commit -m "docs(widgets): show collapsible read search examples"
```

Expected: one commit containing only gallery files.

---

### Task 7: Final Verification

**Files:**

- Verify: frontend TypeScript project
- Verify: focused widget tests
- Verify: attachment command tests
- Verify: full frontend test suite

**Interfaces:**

- Consumes all prior task outputs.
- Produces verified implementation with no additional code changes unless a verification failure points to a task-owned defect.

- [ ] **Step 1: Run TypeScript build**

Run:

```bash
npx tsc -b
```

Expected: PASS with no TypeScript errors.

- [ ] **Step 2: Run focused widget tests**

Run:

```bash
npx vitest run src/components/MessageContent.test.tsx src/views/chat/tool-widgets/ToolDisclosure.test.tsx src/views/chat/tool-widgets/ReadPreview.test.tsx src/views/chat/tool-widgets/ReadWidget.test.tsx src/views/chat/tool-widgets/SearchResultsWidget.test.tsx src/views/design-system/WidgetGallery.test.tsx
```

Expected: PASS.

- [ ] **Step 3: Run attachment command tests**

Run:

```bash
cd src-tauri
cargo test commands::attachments::tests
```

Expected: PASS.

- [ ] **Step 4: Run full frontend suite**

Run:

```bash
npx vitest run
```

Expected: PASS.

- [ ] **Step 5: Confirm no unintended files are staged or modified**

Run:

```bash
git status --short
```

Expected: only intentional committed changes from Tasks 1-6. If this command shows uncommitted files, inspect them and either commit intentional task files or report unrelated changes without touching them.

---

## Final Review Checklist

- [ ] Successful Read results are collapsed by default.
- [ ] Read summaries show path, bytes, optional tokens, and a right chevron.
- [ ] Read text previews render captured `outcome.content`.
- [ ] Read markdown previews render through shared markdown renderer.
- [ ] Read image/video/audio previews use `commands.readAttachedFile(detail.filePath)`.
- [ ] Read unsupported files render `Preview unavailable`.
- [ ] Read native preview failures render inline preview errors.
- [ ] Offloaded/truncated successful Reads have no separate collapsed styling, labels, or rows.
- [ ] Failed Reads remain destructive non-disclosure cards.
- [ ] Successful Grep/Glob results are collapsed by default.
- [ ] Grep/Glob expanded bodies are `max-h-80 overflow-y-auto`.
- [ ] Interrupted Grep/Glob results remain visible non-disclosure notices.
- [ ] No backend result JSON shape changed.
- [ ] No new IPC command was added.
