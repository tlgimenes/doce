import { ChevronRight, Wrench } from "lucide-react";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Item, ItemContent, ItemMedia, ItemTitle } from "@/components/ui/item";
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
    <Collapsible
      data-slot="widget-frame"
      className="overflow-hidden rounded-lg border border-border bg-card text-sm"
      data-testid="unknown-tool-widget"
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
          <Wrench />
        </ItemMedia>
        <ItemContent>
          <ItemTitle>{detail.toolName}</ItemTitle>
        </ItemContent>
        <ChevronRight
          aria-hidden="true"
          data-slot="widget-frame-chevron"
          className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-aria-expanded/widget-frame:rotate-90"
        />
      </CollapsibleTrigger>
      <CollapsibleContent data-slot="widget-frame-content" className="border-t border-border">
        <pre
          data-slot="code-block"
          data-tone="default"
          className="overflow-x-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap wrap-break-word text-foreground"
        >
          {JSON.stringify(detail, null, 2)}
        </pre>
      </CollapsibleContent>
    </Collapsible>
  );
}
