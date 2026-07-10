import { Wrench } from "lucide-react";
import { CodeBlock } from "@/components/ui/code-block";
import { ItemContent, ItemMedia, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "@/components/ui/widget-frame";
import type { ToolResultDetail, UnknownToolDetail } from "@/lib/ipc";

interface UnknownToolWidgetProps {
  detail: ToolResultDetail | UnknownToolDetail;
}

/**
 * FR-011/SC-004: the fallback for any `toolName` without a dedicated
 * widget (including a completely unrecognized one, or a tool with a
 * dedicated widget that simply hasn't landed yet) — the tool's name plus a
 * readable rendering of its detail payload, never blank or broken.
 */
export default function UnknownToolWidget({ detail }: UnknownToolWidgetProps) {
  return (
    <WidgetFrame collapsible data-testid="unknown-tool-widget">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <Wrench />
        </ItemMedia>
        <ItemContent>
          <ItemTitle>{detail.toolName}</ItemTitle>
        </ItemContent>
      </WidgetFrameHeader>
      <WidgetFrameContent>
        <CodeBlock>{JSON.stringify(detail, null, 2)}</CodeBlock>
      </WidgetFrameContent>
    </WidgetFrame>
  );
}
