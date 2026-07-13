import { ChevronRight, FileText } from "lucide-react";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import type { ReadDetail } from "@/lib/ipc";
import { formatByteCount } from "@/lib/formatByteCount";
import { formatTokenCount } from "@/lib/formatTokenCount";
import { pathBasename } from "@/lib/pathBasename";
import ReadPreview from "./ReadPreview";
import ViewFullOutput from "./ViewFullOutput";

interface ReadWidgetProps {
  detail: ReadDetail;
}

/**
 * US4/FR-005: a compact file-reference card, not a plain-text dump of the
 * file's contents. The collapsed row reads as an activity sentence
 * ("Read composer.tsx"); the technical payload — full path, byte size,
 * token count — lives in the expanded panel and the hover title.
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
  const byteCount = formatByteCount(detail.outcome.contentBytes ?? previewLength);
  const tokenCount =
    detail.tokenCount != null ? `${formatTokenCount(detail.tokenCount)} tok` : null;
  const payloadPath = detail.payloadRef ?? detail.offloadedTo;
  const meta = [detail.filePath, byteCount, tokenCount].filter(Boolean).join(" · ");

  return (
    <Collapsible data-testid="read-widget">
      <CollapsibleTrigger
        nativeButton={false}
        render={<Marker className="group/marker-row cursor-pointer" />}
      >
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
        <ChevronRight
          aria-hidden="true"
          className="ml-auto size-4 shrink-0 transition-transform group-aria-expanded/marker-row:rotate-90"
        />
      </CollapsibleTrigger>
      <CollapsibleContent className="pl-6" data-testid="read-preview">
        <div className="max-h-80 overflow-y-auto p-3">
          <ReadPreview detail={detail} />
          {payloadPath && <ViewFullOutput path={payloadPath} />}
          <p data-testid="read-meta" className="mt-2 text-xs text-muted-foreground">
            {meta}
          </p>
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}
