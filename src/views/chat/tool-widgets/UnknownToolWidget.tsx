import { ChevronRight, Wrench } from "lucide-react";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
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
    <Collapsible data-testid="unknown-tool-widget">
      <CollapsibleTrigger
        nativeButton={false}
        render={<Marker className="group/marker-row cursor-pointer" />}
      >
        <MarkerIcon>
          <Wrench />
        </MarkerIcon>
        <MarkerContent className="min-w-0 truncate">{detail.toolName}</MarkerContent>
        <ChevronRight
          aria-hidden="true"
          className="ml-auto size-4 shrink-0 transition-transform group-aria-expanded/marker-row:rotate-90"
        />
      </CollapsibleTrigger>
      <CollapsibleContent className="pl-6">
        <pre className="overflow-x-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap wrap-break-word text-foreground">
          {JSON.stringify(detail, null, 2)}
        </pre>
      </CollapsibleContent>
    </Collapsible>
  );
}
