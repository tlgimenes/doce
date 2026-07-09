import type { ReadDetail } from "@/lib/ipc";
import { formatByteCount } from "@/lib/formatByteCount";
import { formatTokenCount } from "@/lib/formatTokenCount";
import ToolDisclosure from "./ToolDisclosure";
import ReadPreview from "./ReadPreview";
import ViewFullOutput from "./ViewFullOutput";

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

  // New rows only carry a bounded preview (contentPreview, capped at 2000
  // chars) + contentBytes (the byte length of that already-capped tool
  // output, NOT the source file's size); legacy rows persisted before the
  // payload-files design still carry the full content inline.
  const previewLength = (detail.outcome.contentPreview ?? detail.outcome.content ?? "").length;
  const byteCount = formatByteCount(detail.outcome.contentBytes ?? previewLength);
  const tokenCount =
    detail.tokenCount != null ? `${formatTokenCount(detail.tokenCount)} tok` : null;
  const payloadPath = detail.payloadRef ?? detail.offloadedTo;

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
      {payloadPath && <ViewFullOutput path={payloadPath} />}
    </ToolDisclosure>
  );
}
