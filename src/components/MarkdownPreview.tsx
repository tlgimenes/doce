import ReactMarkdown from "react-markdown";
import type * as React from "react";
import { cn } from "@/lib/cn";

interface MarkdownPreviewProps extends Omit<React.HTMLAttributes<HTMLDivElement>, "children"> {
  children: string;
  className?: string;
  testId?: string;
}

export default function MarkdownPreview({
  children,
  className,
  testId,
  ...divProps
}: MarkdownPreviewProps): React.JSX.Element {
  return (
    <div
      {...divProps}
      className={cn("prose prose-sm dark:prose-invert max-w-none", className)}
      data-testid={testId}
    >
      <ReactMarkdown>{children}</ReactMarkdown>
    </div>
  );
}
