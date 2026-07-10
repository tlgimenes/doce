import { useState, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/cn";

interface ToolDisclosureProps {
  summary: ReactNode;
  children: ReactNode;
  testId?: string;
  summaryTestId?: string;
  bodyTestId?: string;
  bodyClassName?: string;
}

export default function ToolDisclosure({
  summary,
  children,
  testId,
  summaryTestId,
  bodyTestId,
  bodyClassName,
}: ToolDisclosureProps) {
  const [open, setOpen] = useState(false);

  return (
    <details
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
      className="group overflow-hidden rounded-md border border-border bg-card text-sm shadow-sm [&>summary::-webkit-details-marker]:hidden"
      data-testid={testId}
    >
      <summary
        className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 font-mono text-xs text-muted-foreground focus-visible:outline-offset-[-2px]"
        data-testid={summaryTestId}
      >
        <span className="min-w-0 flex-1 truncate">{summary}</span>
        <ChevronRight
          size={14}
          aria-hidden="true"
          data-testid="tool-disclosure-chevron"
          className="shrink-0 text-muted-foreground transition-transform group-open:rotate-90"
        />
      </summary>
      {open && (
        <div
          className={cn("max-h-80 overflow-y-auto border-t border-border p-3", bodyClassName)}
          data-testid={bodyTestId}
        >
          {children}
        </div>
      )}
    </details>
  );
}
