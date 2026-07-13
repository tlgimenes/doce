import { FilePlus } from "lucide-react";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import type { WriteDetail } from "@/lib/ipc";
import { pathBasename } from "@/lib/pathBasename";

interface WriteWidgetProps {
  detail: WriteDetail;
}

/**
 * US4/FR-006: distinct from ReadWidget and from a plain reply — a compact
 * activity sentence for a created/overwritten file ("Created notes.md").
 * The full path and byte count live in the hover title.
 */
export default function WriteWidget({ detail }: WriteWidgetProps) {
  const fileLabel = detail.filePath ? pathBasename(detail.filePath) : "file";

  if (!detail.outcome.ok) {
    return (
      <Marker data-testid="write-widget">
        <MarkerIcon>
          <FilePlus />
        </MarkerIcon>
        <MarkerContent className="flex min-w-0 flex-col">
          <span className="truncate" title={detail.filePath ?? undefined}>
            Couldn&apos;t write {fileLabel}
          </span>
          <span className="text-xs">{detail.outcome.error}</span>
        </MarkerContent>
      </Marker>
    );
  }

  const title = [detail.filePath, `${detail.byteCount} bytes`].filter(Boolean).join(" · ");

  return (
    <Marker data-testid="write-widget">
      <MarkerIcon>
        <FilePlus />
      </MarkerIcon>
      <MarkerContent data-testid="write-header" className="min-w-0 truncate" title={title}>
        Created {fileLabel}
      </MarkerContent>
    </Marker>
  );
}
