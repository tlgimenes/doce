import { useEffect, useState } from "react";
import MarkdownPreview from "@/components/MarkdownPreview";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Spinner } from "@/components/ui/spinner";
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
  // New rows only carry a bounded preview (contentPreview); legacy rows
  // persisted before the payload-files design still carry the full
  // content inline.
  const content = detail.outcome.contentPreview ?? detail.outcome.content ?? "";

  if (kind === "markdown") {
    return (
      <div data-testid="read-markdown-preview">
        <MarkdownPreview>{content}</MarkdownPreview>
      </div>
    );
  }

  if (kind === "text") {
    return (
      <pre
        data-testid="read-text-preview"
        className="overflow-x-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap wrap-break-word text-foreground"
      >
        {content}
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
      <p className="flex items-center gap-2" data-testid="read-preview-loading">
        <Spinner role="presentation" aria-label={undefined} />
        Loading preview…
      </p>
    );
  }

  if (state.status === "error") {
    return (
      <Empty data-testid="read-preview-error">
        <EmptyHeader>
          <EmptyTitle>Preview failed</EmptyTitle>
          <EmptyDescription>{state.error}</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  const mediaKind = nativeMediaKind(state.mimeType);

  if (mediaKind === "image") {
    return (
      <img
        src={state.dataUrl}
        alt={state.name}
        className="max-h-80 max-w-full object-contain"
        data-testid="read-image-preview"
      />
    );
  }

  if (mediaKind === "video") {
    return (
      <video
        src={state.dataUrl}
        controls
        className="max-h-80 w-full"
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
    <Empty data-testid="read-preview-unavailable">
      <EmptyHeader>
        <EmptyDescription>Preview unavailable{filePath ? ` for ${filePath}` : ""}</EmptyDescription>
      </EmptyHeader>
    </Empty>
  );
}
