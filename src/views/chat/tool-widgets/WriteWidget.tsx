import { FilePlus } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import type { WriteDetail } from "@/lib/ipc";

interface WriteWidgetProps {
  detail: WriteDetail;
}

/** US4/FR-006: distinct from ReadWidget and from a plain reply — a compact file-reference card for a created/overwritten file. */
export default function WriteWidget({ detail }: WriteWidgetProps) {
  if (!detail.outcome.ok) {
    return (
      <Marker data-testid="write-widget">
        <MarkerIcon>
          <FilePlus />
        </MarkerIcon>
        <MarkerContent className="flex min-w-0 flex-col">
          <span className="truncate" title={detail.filePath ?? undefined}>
            Write {detail.filePath}
          </span>
          <span className="text-xs">{detail.outcome.error}</span>
        </MarkerContent>
        <Badge variant="destructive" className="ml-auto shrink-0">
          Failed
        </Badge>
      </Marker>
    );
  }

  return (
    <Marker data-testid="write-widget">
      <MarkerIcon>
        <FilePlus />
      </MarkerIcon>
      <MarkerContent className="flex min-w-0 flex-col">
        <span data-testid="write-header" className="truncate" title={detail.filePath ?? undefined}>
          {detail.filePath}
        </span>
        <span data-testid="write-body" className="text-xs">
          Write · {detail.byteCount} bytes
        </span>
      </MarkerContent>
      <Badge variant="secondary" className="ml-auto shrink-0">
        Written
      </Badge>
    </Marker>
  );
}
