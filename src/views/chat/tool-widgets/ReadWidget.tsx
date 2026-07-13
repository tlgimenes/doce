import { FileText } from "lucide-react";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import type { ReadDetail } from "@/lib/ipc";
import { formatByteCount } from "@/lib/formatByteCount";
import { pathBasename } from "@/lib/pathBasename";

interface ReadWidgetProps {
  detail: ReadDetail;
}

/**
 * US4/FR-005: a compact file-reference line, not a dump of the file's
 * contents — "Read composer.tsx" followed by brief muted size/token info.
 * The full path lives in the hover title.
 */
export default function ReadWidget({ detail }: ReadWidgetProps) {
  const fileLabel = detail.filePath ? pathBasename(detail.filePath) : "file";

  if (!detail.outcome.ok) {
    return (
      <Marker data-testid="read-widget">
        <MarkerIcon>
          <FileText />
        </MarkerIcon>
        <MarkerContent className="flex min-w-0 flex-col">
          <span className="truncate" title={detail.filePath ?? undefined}>
            Couldn&apos;t read {fileLabel}
          </span>
          <span className="text-xs">{detail.outcome.error}</span>
        </MarkerContent>
      </Marker>
    );
  }

  // New rows only carry a bounded preview (contentPreview, capped at 2000
  // chars) + contentBytes (the byte length of that already-capped tool
  // output, NOT the source file's size); legacy rows persisted before the
  // payload-files design still carry the full content inline.
  const previewLength = (detail.outcome.contentPreview ?? detail.outcome.content ?? "").length;
  // Token counts live on the turn accumulator (StreamingStatus + the final
  // reply's footer), not on individual widgets.
  const meta = formatByteCount(detail.outcome.contentBytes ?? previewLength);

  return (
    <Marker data-testid="read-widget">
      <MarkerIcon>
        <FileText />
      </MarkerIcon>
      <MarkerContent
        data-testid="read-summary"
        className="min-w-0 truncate"
        title={detail.filePath ?? undefined}
      >
        Read {fileLabel}
      </MarkerContent>
      <span data-testid="read-meta" className="shrink-0 self-end text-xs text-muted-foreground">
        {meta}
      </span>
    </Marker>
  );
}
