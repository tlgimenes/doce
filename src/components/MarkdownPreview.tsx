import ReactMarkdown from "react-markdown";
import { cn } from "@/lib/cn";

interface MarkdownPreviewProps {
  children: string;
  className?: string;
  testId?: string;
}

export default function MarkdownPreview({ children, className, testId }: MarkdownPreviewProps) {
  return (
    <div
      className={cn("prose prose-sm dark:prose-invert max-w-none", className)}
      data-testid={testId}
    >
      <ReactMarkdown>{children}</ReactMarkdown>
    </div>
  );
}
