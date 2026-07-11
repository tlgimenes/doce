import { useId, useState } from "react";
import { ArrowLeft, SendHorizontal, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldLabel,
  FieldTitle,
} from "@/components/ui/field";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { commands, type AskUserQuestionDetail } from "@/lib/ipc";
import RichInput from "@/views/chat/rich-input/RichInput";
import { runViewTransition } from "@/lib/viewTransition";

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
 * The live, still-unanswered `AskUserQuestion` prompt, rendered in the
 * chat composer slot in place of RichInput while a question is pending
 * (Workspace.tsx). One shared, unboxed header (eyebrow + question, one
 * icon button in the same slot in both modes) sits above a single
 * bordered "module": in options mode, a list of real radio/checkbox rows
 * plus a footer holding the one submit button also used by multi-select
 * and free text; in text mode, a bare RichInput (it already supplies its
 * own matching card -- wrapping it in a second border here would double
 * it up, which is exactly what the old implementation did). Picking an
 * option only selects it, single- or multi-select alike; answering
 * always requires pressing the submit button, which stays disabled until
 * at least one option is selected. The close (X) button swaps to free
 * text instead, whose submission answers the question with the raw
 * typed text -- for whenever the fixed option labels don't cover what
 * the user actually wants to say. Once answered, this component unmounts
 * on its own: Workspace.tsx stops rendering it as soon as the resolved
 * tool_result replaces the pending tool_call as the latest message.
 * (Compare AskUserQuestionWidget, which renders the read-only "already
 * answered" state in message history and never handles a live question.)
 *
 * Both the composer-level mount/unmount of this whole component
 * (Workspace.tsx) and the options<->text mode switch within it ride the
 * app's existing view-transition language (runViewTransition,
 * src/lib/viewTransition.ts) -- see switchMode below; the mode switch
 * degrades gracefully to the root transition now that this component no
 * longer names its own view-transition group.
 */
export default function UserAskWidget({ detail, initialMode = "options" }: UserAskWidgetProps) {
  const [mode, setMode] = useState<Mode>(initialMode);
  const [selected, setSelected] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const questionId = useId();

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
    setSelected((prev) =>
      prev.includes(label) ? prev.filter((l) => l !== label) : [...prev, label],
    );
  };

  const switchMode = (next: Mode) => {
    runViewTransition(() => setMode(next));
  };

  return (
    <div className="flex flex-col gap-1.5" data-testid="user-ask-widget">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          {mode === "options" && detail.header && (
            <p className="mb-0.5 text-xs text-muted-foreground">{detail.header}</p>
          )}
          <p id={questionId} className="text-sm font-medium text-foreground">
            {mode === "options" ? detail.question : `Answering: ${detail.question}`}
          </p>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className="shrink-0 text-muted-foreground hover:bg-transparent"
          disabled={submitting}
          onClick={() => switchMode(mode === "options" ? "text" : "options")}
          aria-label={mode === "options" ? "Close question" : "Back to options"}
          data-testid={mode === "options" ? "question-close" : "question-back-to-options"}
        >
          {mode === "options" ? <X size={14} /> : <ArrowLeft size={14} />}
        </Button>
      </div>

      {mode === "text" ? (
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
      ) : (
        <div className="flex flex-col gap-2 rounded-lg border border-border bg-card px-3 py-2 shadow-xs transition-shadow focus-within:shadow-sm">
          {detail.multiSelect ? (
            <div className="flex flex-col gap-0.5" role="group" aria-labelledby={questionId}>
              {detail.options.map((option, index) => (
                <FieldLabel key={option.label} htmlFor={`${questionId}-option-${index}`}>
                  <Field orientation="horizontal" data-testid="question-option">
                    <Checkbox
                      id={`${questionId}-option-${index}`}
                      checked={selected.includes(option.label)}
                      onCheckedChange={() => toggleOption(option.label)}
                      disabled={submitting}
                    />
                    <FieldContent>
                      <FieldTitle>{option.label}</FieldTitle>
                      {option.description && (
                        <FieldDescription>{option.description}</FieldDescription>
                      )}
                    </FieldContent>
                  </Field>
                </FieldLabel>
              ))}
            </div>
          ) : (
            <RadioGroup
              value={selected[0] ?? null}
              onValueChange={(value) => setSelected(value == null ? [] : [String(value)])}
              aria-labelledby={questionId}
              disabled={submitting}
              className="flex flex-col gap-0.5"
            >
              {detail.options.map((option, index) => (
                <FieldLabel key={option.label} htmlFor={`${questionId}-option-${index}`}>
                  <Field orientation="horizontal" data-testid="question-option">
                    <RadioGroupItem id={`${questionId}-option-${index}`} value={option.label} />
                    <FieldContent>
                      <FieldTitle>{option.label}</FieldTitle>
                      {option.description && (
                        <FieldDescription>{option.description}</FieldDescription>
                      )}
                    </FieldContent>
                  </Field>
                </FieldLabel>
              ))}
            </RadioGroup>
          )}
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs text-muted-foreground">
              {detail.multiSelect && selected.length > 0 ? `${selected.length} selected` : ""}
            </span>
            <Button
              type="button"
              variant="default"
              size="icon"
              disabled={selected.length === 0 || submitting}
              onClick={() => submit(selected)}
              aria-label="Send answer"
              data-testid="question-submit"
            >
              <SendHorizontal size={16} />
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
