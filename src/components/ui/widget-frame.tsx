import * as React from "react";
import { ChevronRight } from "lucide-react";

import { cn } from "@/lib/utils";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Item } from "@/components/ui/item";

const WidgetFrameContext = React.createContext<{ collapsible: boolean }>({
  collapsible: false,
});

const frameClassName = "overflow-hidden rounded-lg border border-border bg-card text-sm";

function WidgetFrame({
  collapsible = false,
  defaultOpen = false,
  className,
  children,
  ...props
}: React.ComponentProps<"div"> & {
  collapsible?: boolean;
  defaultOpen?: boolean;
}) {
  const value = React.useMemo(() => ({ collapsible }), [collapsible]);
  if (!collapsible) {
    return (
      <WidgetFrameContext.Provider value={value}>
        <div data-slot="widget-frame" className={cn(frameClassName, className)} {...props}>
          {children}
        </div>
      </WidgetFrameContext.Provider>
    );
  }
  return (
    <WidgetFrameContext.Provider value={value}>
      <Collapsible
        data-slot="widget-frame"
        defaultOpen={defaultOpen}
        className={cn(frameClassName, className)}
        {...props}
      >
        {children}
      </Collapsible>
    </WidgetFrameContext.Provider>
  );
}

function WidgetFrameHeader({ className, children, ...props }: React.ComponentProps<"div">) {
  const { collapsible } = React.useContext(WidgetFrameContext);
  if (!collapsible) {
    return (
      <Item
        data-slot="widget-frame-header"
        size="xs"
        className={cn("w-full", className)}
        {...props}
      >
        {children}
      </Item>
    );
  }
  return (
    <CollapsibleTrigger
      nativeButton={false}
      render={
        <Item
          data-slot="widget-frame-header"
          size="xs"
          className={cn(
            "group/widget-frame w-full cursor-pointer rounded-none hover:bg-accent",
            className,
          )}
          {...props}
        />
      }
    >
      {children}
      <ChevronRight
        aria-hidden="true"
        data-slot="widget-frame-chevron"
        className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-aria-expanded/widget-frame:rotate-90"
      />
    </CollapsibleTrigger>
  );
}

function WidgetFrameContent({
  className,
  ...props
}: React.ComponentProps<typeof CollapsibleContent>) {
  return (
    <CollapsibleContent
      data-slot="widget-frame-content"
      className={cn("border-t border-border", className)}
      {...props}
    />
  );
}

export { WidgetFrame, WidgetFrameHeader, WidgetFrameContent };
