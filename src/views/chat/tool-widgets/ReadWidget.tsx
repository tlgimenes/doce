import type { ReadDetail } from "@/lib/ipc";
import { formatByteCount } from "@/lib/formatByteCount";
import { formatTokenCount } from "@/lib/formatTokenCount";
import ToolDisclosure from "./ToolDisclosure";
import ReadPreview from "./ReadPreview";

interface ReadWidgetProps {
  detail: ReadDetail;
}

/** US4/FR-005: a compact file-reference card, not a plain-text dump of the file's contents. */
export default function ReadWidget({ detail }: ReadWidgetProps) {
  if (!detail.outcome.ok) {
    return (
      <div
        className="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm"
        data-testid="read-widget"
      >
        <p className="mb-1 font-mono text-xs text-muted-foreground">
          Read <span>{detail.filePath}</span>
        </p>
        <p className="text-destructive">{detail.outcome.error}</p>
      </div>
    );
  }

  const byteCount = formatByteCount(detail.outcome.content.length);
  const tokenCount =
    detail.tokenCount != null ? `${formatTokenCount(detail.tokenCount)} tok` : null;

  return (
    <ToolDisclosure
      testId="read-widget"
      summaryTestId="read-summary"
      bodyTestId="read-preview"
      summary={
        <span className="font-mono text-xs text-muted-foreground">
          Read <span>{detail.filePath}</span> · {byteCount}
          {tokenCount != null && <> · {tokenCount}</>}
        </span>
      }
    >
      <ReadPreview detail={detail} />
    </ToolDisclosure>
  );
}
