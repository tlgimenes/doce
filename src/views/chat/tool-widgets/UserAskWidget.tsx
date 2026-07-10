import { useId, useState } from "react";
import { ArrowLeft, Check, SendHorizontal, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/cn";
import { commands, type AskUserQuestionDetail, type QuestionOption } from "@/lib/ipc";
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

// Identical to RichInput's own send button (RichInput.tsx) -- same size,
// same gradient sheen, same icon -- so single-select, multi-select, and
// free text all answer via one visually consistent affordance.
const SUBMIT_BUTTON_CLASSES =
  "shrink-0 enabled:bg-gradient-to-r enabled:from-[var(--color-primary)] enabled:via-[var(--color-doce-caramel)] enabled:to-[var(--color-doce-cacao)] enabled:hover:from-[var(--color-doce-caramel)] enabled:hover:via-[var(--color-primary)] enabled:hover:to-[var(--color-foreground)]";

/**
 * One option row inside the options module -- a real radio/checkbox
 * control, not a Button pill: a glyph on the left (empty ring/square at
 * rest, filled on selection), the option's label and its description
 * stacked to the right. The description used to be reachable only via a
 * hover `title=` attribute; it's always-visible text now, so keyboard and
 * screen-reader users can read it too.
 */
function OptionRow({
  option,
  selected,
  multiSelect,
  disabled,
  index,
  onSelect,
}: {
  option: QuestionOption;
  selected: boolean;
  multiSelect: boolean;
  disabled: boolean;
  index: number;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role={multiSelect ? "checkbox" : "radio"}
      aria-checked={selected}
      disabled={disabled}
      onClick={onSelect}
      style={{ animationDelay: `${index * 18}ms` }}
      className={cn(
        "doce-ask-option-row-enter flex w-full items-start gap-2.5 rounded-md px-2.5 py-2 text-left text-sm transition-colors",
        selected ? "bg-muted" : "hover:bg-muted",
      )}
    >
      <span
        className={cn(
          "mt-0.5 flex size-4 shrink-0 items-center justify-center border-[1.5px] border-border",
          multiSelect ? "rounded-[4px]" : "rounded-full",
          selected && (multiSelect ? "border-primary bg-primary" : "border-foreground"),
        )}
      >
        {selected &&
          (multiSelect ? (
            <Check size={10} className="text-primary-foreground" strokeWidth={3} />
          ) : (
            <span className="size-2 rounded-full bg-foreground" />
          ))}
      </span>
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className={cn("text-foreground", selected && "font-semibold")}>{option.label}</span>
        {option.description && (
          <span className="text-xs leading-snug text-muted-foreground">{option.description}</span>
        )}
      </span>
    </button>
  );
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
 * src/lib/viewTransition.ts) -- see switchMode below and the
 * [view-transition-name:user-ask-module] wrapper.
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
    if (!detail.multiSelect) {
      setSelected([label]);
      return;
    }
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
          size="icon-sm"
          className="shrink-0 text-muted-foreground hover:bg-transparent"
          disabled={submitting}
          onClick={() => switchMode(mode === "options" ? "text" : "options")}
          aria-label={mode === "options" ? "Close question" : "Back to options"}
          data-testid={mode === "options" ? "question-close" : "question-back-to-options"}
        >
          {mode === "options" ? <X size={14} /> : <ArrowLeft size={14} />}
        </Button>
      </div>

      <div className="[view-transition-name:user-ask-module]">
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
            <div
              className="flex flex-col gap-0.5"
              role={detail.multiSelect ? "group" : "radiogroup"}
              aria-labelledby={questionId}
            >
              {detail.options.map((option, index) => (
                <OptionRow
                  key={option.label}
                  option={option}
                  selected={selected.includes(option.label)}
                  multiSelect={detail.multiSelect}
                  disabled={submitting}
                  index={index}
                  onSelect={() => toggleOption(option.label)}
                />
              ))}
            </div>
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs text-muted-foreground">
                {detail.multiSelect && selected.length > 0 ? `${selected.length} selected` : ""}
              </span>
              <Button
                type="button"
                variant="primary"
                size="icon"
                className={SUBMIT_BUTTON_CLASSES}
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
    </div>
  );
}
