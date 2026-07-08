import { useState } from "react";
import { XIcon } from "@phosphor-icons/react";
import { Button } from "@/components/ui/button";
import { commands, type AskUserQuestionDetail } from "@/lib/ipc";
import RichInput from "@/views/chat/rich-input/RichInput";

type Mode = "options" | "text";

interface UserAskWidgetProps {
  detail: AskUserQuestionDetail;
  /**
   * Seeds which mode the widget starts in. Always omitted (defaults to
   * "options") by the real caller, Workspace.tsx -- only WidgetGallery.tsx
   * passes "text", to preview the free-text fallback state without
   * requiring a click first.
   */
  initialMode?: Mode;
}

/**
 * The live, still-unanswered `AskUserQuestion` prompt (contracts/
 * tool-widgets.md), rendered in the chat composer slot in place of
 * RichInput while a question is pending (Workspace.tsx). Single-select
 * answers immediately on click; multi-select accumulates a selection and
 * requires an explicit confirm. The close (X) button swaps to a full
 * RichInput instead, whose submission answers the question with the raw
 * typed text -- for whenever the fixed option labels don't cover what the
 * user actually wants to say. Once answered, this component unmounts on
 * its own: Workspace.tsx stops rendering it as soon as the resolved
 * tool_result replaces the pending tool_call as the latest message.
 * (Compare AskUserQuestionWidget, which renders the read-only "already
 * answered" state in message history and never handles a live question.)
 */
export default function UserAskWidget({ detail, initialMode = "options" }: UserAskWidgetProps) {
  const [mode, setMode] = useState<Mode>(initialMode);
  const [selected, setSelected] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);

  const submit = async (answer: string[]) => {
    if (answer.length === 0 || submitting) return;
    setSubmitting(true);
    try {
      await commands.answerUserQuestion(detail.questionId, answer);
    } finally {
      setSubmitting(false);
    }
  };

  const toggleOption = (label: string) => {
    if (!detail.multiSelect) {
      submit([label]);
      return;
    }
    setSelected((prev) =>
      prev.includes(label) ? prev.filter((l) => l !== label) : [...prev, label],
    );
  };

  if (mode === "text") {
    return (
      <div
        className="rounded-lg border border-border bg-card p-3 text-sm"
        data-testid="user-ask-widget"
      >
        <div className="mb-2 flex items-center justify-between gap-2">
          <p className="text-xs text-muted-foreground">Answering: {detail.question}</p>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={submitting}
            onClick={() => setMode("options")}
            data-testid="question-back-to-options"
          >
            Back to options
          </Button>
        </div>
        <RichInput
          onSubmit={(content) => {
            if (content.trim()) submit([content]);
          }}
          skillsEnabled={true}
          disabled={submitting}
          placeholder="Type your answer…"
          inputTestId="question-answer-input"
          submitTestId="question-answer-send"
        />
      </div>
    );
  }

  return (
    <div
      className="rounded-lg border border-border bg-card p-3 text-sm"
      data-testid="user-ask-widget"
    >
      <div className="mb-1 flex items-start gap-2">
        {detail.header && <p className="text-xs text-muted-foreground">{detail.header}</p>}
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="ml-auto text-muted-foreground hover:bg-transparent"
          disabled={submitting}
          onClick={() => setMode("text")}
          aria-label="Close question"
          data-testid="question-close"
        >
          <XIcon size={14} />
        </Button>
      </div>
      <p className="mb-2 font-medium">{detail.question}</p>
      {detail.multiSelect && (
        <p className="mb-2 text-xs text-muted-foreground" data-testid="multi-select-indicator">
          Select all that apply
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        {detail.options.map((option) => (
          <Button
            key={option.label}
            type="button"
            variant={selected.includes(option.label) ? "primary" : "secondary"}
            size="sm"
            disabled={submitting}
            onClick={() => toggleOption(option.label)}
            title={option.description}
          >
            {option.label}
          </Button>
        ))}
      </div>
      {detail.multiSelect && (
        <Button
          type="button"
          variant="primary"
          size="sm"
          className="mt-2"
          disabled={selected.length === 0 || submitting}
          onClick={() => submit(selected)}
          data-testid="question-submit"
        >
          Submit
        </Button>
      )}
    </div>
  );
}
