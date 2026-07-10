import { FileText } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { ItemContent, ItemMedia, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "@/components/ui/widget-frame";
import type { ReadDetail } from "@/lib/ipc";
import { formatByteCount } from "@/lib/formatByteCount";
import { formatTokenCount } from "@/lib/formatTokenCount";
import ReadPreview from "./ReadPreview";
import ViewFullOutput from "./ViewFullOutput";

interface ReadWidgetProps {
  detail: ReadDetail;
}

/** US4/FR-005: a compact file-reference card, not a plain-text dump of the file's contents. */
export default function ReadWidget({ detail }: ReadWidgetProps) {
  if (!detail.outcome.ok) {
    return (
      <WidgetFrame data-testid="read-widget">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <FileText />
          </ItemMedia>
          <ItemContent>
            <ItemTitle>Read {detail.filePath}</ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
        <div className="p-3 pt-0">
          <Alert variant="destructive">
            <AlertDescription>{detail.outcome.error}</AlertDescription>
          </Alert>
        </div>
      </WidgetFrame>
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

  return (
    <WidgetFrame collapsible data-testid="read-widget">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <FileText />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="read-summary" title={detail.filePath ?? undefined}>
            Read {detail.filePath}
          </ItemTitle>
        </ItemContent>
        <span className="flex items-center gap-2">
          <Badge variant="outline">{byteCount}</Badge>
          {tokenCount != null && <Badge variant="outline">{tokenCount}</Badge>}
        </span>
      </WidgetFrameHeader>
      <WidgetFrameContent data-testid="read-preview">
        <div className="max-h-80 overflow-y-auto p-3">
          <ReadPreview detail={detail} />
          {payloadPath && <ViewFullOutput path={payloadPath} />}
        </div>
      </WidgetFrameContent>
    </WidgetFrame>
  );
}
