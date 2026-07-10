import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const codeBlockVariants = cva(
  "overflow-x-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap wrap-break-word",
  {
    variants: {
      tone: {
        default: "text-foreground",
        destructive: "text-destructive",
      },
    },
    defaultVariants: {
      tone: "default",
    },
  },
);

function CodeBlock({
  className,
  tone = "default",
  ...props
}: React.ComponentProps<"pre"> & VariantProps<typeof codeBlockVariants>) {
  return (
    <pre
      data-slot="code-block"
      data-tone={tone}
      className={cn(codeBlockVariants({ tone }), className)}
      {...props}
    />
  );
}

const codeBlockLineVariants = cva("px-3 py-0.5 whitespace-pre", {
  variants: {
    variant: {
      default: "text-foreground",
      added: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400",
      removed: "bg-destructive/15 text-destructive",
    },
  },
  defaultVariants: {
    variant: "default",
  },
});

function CodeBlockLine({
  className,
  variant = "default",
  ...props
}: React.ComponentProps<"div"> & VariantProps<typeof codeBlockLineVariants>) {
  return (
    <div
      data-slot="code-block-line"
      data-variant={variant}
      className={cn(codeBlockLineVariants({ variant }), className)}
      {...props}
    />
  );
}

function CodeInline({ className, ...props }: React.ComponentProps<"code">) {
  return <code data-slot="code-inline" className={cn("font-mono text-xs", className)} {...props} />;
}

export { CodeBlock, CodeBlockLine, CodeInline };
