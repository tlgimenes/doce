import { ChevronRight, FileText } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Item, ItemContent, ItemMedia, ItemTitle } from "@/components/ui/item";
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
      <div
        data-slot="widget-frame"
        className="overflow-hidden rounded-lg border border-border bg-card text-sm"
        data-testid="read-widget"
      >
        <Item data-slot="widget-frame-header" size="xs" className="w-full">
          <ItemMedia variant="icon">
            <FileText />
          </ItemMedia>
          <ItemContent>
            <ItemTitle>Read {detail.filePath}</ItemTitle>
          </ItemContent>
        </Item>
        <div className="p-3 pt-0">
          <Alert variant="destructive">
            <AlertDescription>{detail.outcome.error}</AlertDescription>
          </Alert>
        </div>
      </div>
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
    <Collapsible
      data-slot="widget-frame"
      className="overflow-hidden rounded-lg border border-border bg-card text-sm"
      data-testid="read-widget"
    >
      <CollapsibleTrigger
        nativeButton={false}
        render={
          <Item
            data-slot="widget-frame-header"
            size="xs"
            className="group/widget-frame w-full cursor-pointer rounded-none hover:bg-accent"
          />
        }
      >
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
        <ChevronRight
          aria-hidden="true"
          data-slot="widget-frame-chevron"
          className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-aria-expanded/widget-frame:rotate-90"
        />
      </CollapsibleTrigger>
      <CollapsibleContent
        data-slot="widget-frame-content"
        className="border-t border-border"
        data-testid="read-preview"
      >
        <div className="max-h-80 overflow-y-auto p-3">
          <ReadPreview detail={detail} />
          {payloadPath && <ViewFullOutput path={payloadPath} />}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}
