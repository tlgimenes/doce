import { FilePlus } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameHeader } from "@/components/ui/widget-frame";
import type { WriteDetail } from "@/lib/ipc";

interface WriteWidgetProps {
  detail: WriteDetail;
}

/** US4/FR-006: distinct from ReadWidget and from a plain reply — a compact file-reference card for a created/overwritten file. */
export default function WriteWidget({ detail }: WriteWidgetProps) {
  if (!detail.outcome.ok) {
    return (
      <WidgetFrame data-testid="write-widget">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <FilePlus />
          </ItemMedia>
          <ItemContent>
            <ItemTitle>Write {detail.filePath}</ItemTitle>
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

  return (
    <WidgetFrame data-testid="write-widget">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <FilePlus />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="write-header">{detail.filePath}</ItemTitle>
          <ItemDescription data-testid="write-body">
            Write · {detail.byteCount} bytes
          </ItemDescription>
        </ItemContent>
        <Badge variant="secondary">Written</Badge>
      </WidgetFrameHeader>
    </WidgetFrame>
  );
}
