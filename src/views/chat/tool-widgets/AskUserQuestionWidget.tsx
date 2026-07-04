import { useState } from "react";
import { Button } from "@/components/ui/button";
import { commands, type AskUserQuestionDetail } from "@/lib/ipc";

interface AskUserQuestionWidgetProps {
  detail: AskUserQuestionDetail;
}

/**
 * US3/FR-008/FR-009: a real interactive prompt for the agent's pause/
 * resume `AskUserQuestion` tool call — single-select answers immediately
 * on click (matching how a plain choice normally works); multi-select
 * accumulates a selection and requires an explicit confirm, since
 * "clicked once" isn't a complete answer when more than one is allowed.
 * Once `detail.answer` is set (the row was updated in place by
 * `answer_user_question` — data-model.md), this renders a read-only
 * "already answered" state and never re-submits.
 */
export default function AskUserQuestionWidget({ detail }: AskUserQuestionWidgetProps) {
  const [selected, setSelected] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);

  if (detail.answer) {
    return (
      <div
        className="rounded-lg border border-border bg-card p-3 text-sm"
        data-testid="question-answered"
      >
        <p className="mb-1 text-muted-foreground">{detail.question}</p>
        <p className="font-medium">You chose: {detail.answer.join(", ")}</p>
      </div>
    );
  }

  const toggle = (label: string) => {
    if (!detail.multiSelect) {
      submit([label]);
      return;
    }
    setSelected((prev) =>
      prev.includes(label) ? prev.filter((l) => l !== label) : [...prev, label],
    );
  };

  const submit = async (answer: string[]) => {
    if (answer.length === 0 || submitting) return;
    setSubmitting(true);
    try {
      await commands.answerUserQuestion(detail.questionId, answer);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      className="rounded-lg border border-border bg-card p-3 text-sm"
      data-testid="question-widget"
    >
      {detail.header && <p className="mb-1 text-xs text-muted-foreground">{detail.header}</p>}
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
            onClick={() => toggle(option.label)}
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
