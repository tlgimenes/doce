import ReactMarkdown from "react-markdown";
import { cn } from "@/lib/cn";

interface MarkdownPreviewProps {
  children: string;
  className?: string;
}

export default function MarkdownPreview({ children, className }: MarkdownPreviewProps) {
  return (
    <div className={cn("prose prose-sm dark:prose-invert max-w-none", className)}>
      <ReactMarkdown>{children}</ReactMarkdown>
    </div>
  );
}
