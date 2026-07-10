# Tool Widgets Shadcn Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recompose the 11 read-only tool-call widgets onto a unified collapsible `WidgetFrame` + `CodeBlock` built from stock shadcn primitives, deleting `ToolDisclosure` and all raw palette colors from app files.

**Architecture:** Two new ui-layer primitives (`widget-frame.tsx` composing Item + Collapsible; `code-block.tsx` for mono/diff rendering) carry all chrome. Each widget becomes content-only composition: header (icon, title, Badges/Spinner) + optional collapsible body. Error branches become `Alert variant="destructive"`.

**Tech Stack:** React 19, shadcn base-nova on Base UI (`Collapsible` = Base UI Collapsible), Tailwind v4 tokens, lucide-react, Vitest + Testing Library (jsdom).

**Spec:** `docs/superpowers/specs/2026-07-10-tool-widgets-shadcn-unify-design.md` — read it first.

## Global Constraints

- Work on `main`, in place (repo convention).
- App widget files (`src/views/chat/tool-widgets/*`) may use ONLY layout utilities (flex/grid, gap, p-_/m-_, sizing, min-w-0, truncate, overflow-\*). NO color/typography/border/shadow/radius utilities, NO arbitrary values/properties, NO palette colors (emerald/sky/amber/red). All visuals come from ui-layer components.
- `UserAskWidget.tsx` and its test: DO NOT TOUCH.
- Tool payload parsing, the `ToolWidget` dispatcher in `TranscriptRow.tsx`, and IPC are frozen. FR-011 holds: unknown/malformed payloads always render a diagnostic widget.
- Status→token mapping: running → `Spinner role="presentation" aria-label={undefined}` + muted text; success → `Badge variant="secondary"`; failed/nonzero exit → `Badge variant="destructive"`; interrupted → `Badge variant="outline"`. Metadata chips (tokens, bytes, counts, +N/−N) → `Badge variant="outline"`.
- Preserve every existing `data-testid` and user-visible text string named in each task; tests keep asserting behavior/structure (data-slots, roles, text), not bespoke classes.
- `npm run format` reformats ~70 unrelated drifted files repo-wide — NEVER run it bare. Format only your task's files: `npx oxfmt <paths>`. Never touch `.superpowers/` report files.
- Commits end with: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Known suites: `npx vitest run src/views/chat/tool-widgets/` runs all widget tests; full gates land in Task 10.

---

### Task 1: `ui/code-block.tsx` primitive

**Files:**

- Create: `src/components/ui/code-block.tsx`
- Test: `src/components/ui/code-block.test.tsx`

**Interfaces:**

- Produces (later tasks import these exactly):
  - `CodeBlock({ tone?: "default" | "destructive", className?, ...props }: React.ComponentProps<"pre"> & …)` → `<pre data-slot="code-block" data-tone={tone}>`
  - `CodeBlockLine({ variant?: "default" | "added" | "removed", ...props }: React.ComponentProps<"div"> & …)` → `<div data-slot="code-block-line" data-variant={variant}>`
  - `CodeInline(props: React.ComponentProps<"code">)` → `<code data-slot="code-inline">`

- [ ] **Step 1: Write the failing test**

Create `src/components/ui/code-block.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CodeBlock, CodeBlockLine, CodeInline } from "./code-block";

describe("CodeBlock", () => {
  it("renders a mono pre with slot and default tone", () => {
    render(<CodeBlock data-testid="cb">hello</CodeBlock>);
    const el = screen.getByTestId("cb");
    expect(el.tagName).toBe("PRE");
    expect(el).toHaveAttribute("data-slot", "code-block");
    expect(el).toHaveAttribute("data-tone", "default");
    expect(el).toHaveTextContent("hello");
  });

  it("renders the destructive tone", () => {
    render(
      <CodeBlock data-testid="cb" tone="destructive">
        boom
      </CodeBlock>,
    );
    expect(screen.getByTestId("cb")).toHaveAttribute("data-tone", "destructive");
  });

  it("renders diff line variants", () => {
    render(
      <CodeBlock>
        <CodeBlockLine data-testid="l1">ctx</CodeBlockLine>
        <CodeBlockLine data-testid="l2" variant="added">
          plus
        </CodeBlockLine>
        <CodeBlockLine data-testid="l3" variant="removed">
          minus
        </CodeBlockLine>
      </CodeBlock>,
    );
    expect(screen.getByTestId("l1")).toHaveAttribute("data-variant", "default");
    expect(screen.getByTestId("l2")).toHaveAttribute("data-variant", "added");
    expect(screen.getByTestId("l3")).toHaveAttribute("data-variant", "removed");
    expect(screen.getByTestId("l2")).toHaveAttribute("data-slot", "code-block-line");
  });

  it("renders inline code", () => {
    render(<CodeInline data-testid="ci">$ ls</CodeInline>);
    const el = screen.getByTestId("ci");
    expect(el.tagName).toBe("CODE");
    expect(el).toHaveAttribute("data-slot", "code-inline");
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/components/ui/code-block.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

Create `src/components/ui/code-block.tsx` (ui layer: token colors; emerald for diff-added is the sanctioned home per spec):

```tsx
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
```

- [ ] **Step 4: Run to verify pass**

Run: `npx vitest run src/components/ui/code-block.test.tsx`
Expected: PASS (4/4).

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npx oxfmt src/components/ui/code-block.tsx src/components/ui/code-block.test.tsx
git add src/components/ui/code-block.tsx src/components/ui/code-block.test.tsx
git commit -m "feat(ui): code-block primitive with diff line variants

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: `ui/widget-frame.tsx` primitive

**Files:**

- Create: `src/components/ui/widget-frame.tsx`
- Test: `src/components/ui/widget-frame.test.tsx`

**Interfaces:**

- Consumes: `Collapsible/CollapsibleTrigger/CollapsibleContent` (Base UI panel; `CollapsibleContent` unmount behavior: Base UI keeps panel mounted hidden — tests use `data-state`/visibility, see Step 1), `Item` family.
- Produces (later tasks import these exactly):
  - `WidgetFrame({ collapsible?: boolean, defaultOpen?: boolean, className?, children, ...props })` — root card. `collapsible=false` (default) renders a plain `<div data-slot="widget-frame">` card; `collapsible` renders a `Collapsible` root with the same slot/chrome.
  - `WidgetFrameHeader({ children, ...props })` — inside a collapsible frame renders a `CollapsibleTrigger` wrapping an `Item size="xs"` (adds a trailing auto-rotating `ChevronRight`); in a plain frame renders the `Item` alone, no chevron. Children are `ItemMedia`/`ItemContent`/`ItemActions` from `@/components/ui/item`.
  - `WidgetFrameContent({ children, ...props })` — `CollapsibleContent` with a top border. Only valid inside `collapsible` frames.

- [ ] **Step 1: Write the failing test**

Create `src/components/ui/widget-frame.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { ItemContent, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "./widget-frame";

describe("WidgetFrame", () => {
  it("renders a header-only card without a trigger", () => {
    render(
      <WidgetFrame data-testid="frame">
        <WidgetFrameHeader>
          <ItemContent>
            <ItemTitle>plain card</ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
      </WidgetFrame>,
    );
    expect(screen.getByTestId("frame")).toHaveAttribute("data-slot", "widget-frame");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByText("plain card")).toBeInTheDocument();
  });

  it("collapsed by default: body hidden until the header is clicked", async () => {
    render(
      <WidgetFrame collapsible data-testid="frame">
        <WidgetFrameHeader>
          <ItemContent>
            <ItemTitle>summary</ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
        <WidgetFrameContent>body text</WidgetFrameContent>
      </WidgetFrame>,
    );
    const trigger = screen.getByRole("button");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("body text")).not.toBeVisible();

    await userEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("body text")).toBeVisible();
  });

  it("defaultOpen renders the body expanded", () => {
    render(
      <WidgetFrame collapsible defaultOpen>
        <WidgetFrameHeader>
          <ItemContent>
            <ItemTitle>summary</ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
        <WidgetFrameContent>open body</WidgetFrameContent>
      </WidgetFrame>,
    );
    expect(screen.getByRole("button")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("open body")).toBeVisible();
  });
});
```

Note: if Base UI's Panel unmounts hidden content instead of hiding it, swap
`not.toBeVisible()` for `not.toBeInTheDocument()` — match the primitive's
real behavior, do not force it.

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/components/ui/widget-frame.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

Create `src/components/ui/widget-frame.tsx`:

```tsx
import * as React from "react";
import { ChevronRight } from "lucide-react";

import { cn } from "@/lib/utils";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Item } from "@/components/ui/item";

const WidgetFrameContext = React.createContext<{ collapsible: boolean }>({
  collapsible: false,
});

const frameClassName = "overflow-hidden rounded-lg border border-border bg-card text-sm";

function WidgetFrame({
  collapsible = false,
  defaultOpen = false,
  className,
  children,
  ...props
}: React.ComponentProps<"div"> & {
  collapsible?: boolean;
  defaultOpen?: boolean;
}) {
  const value = React.useMemo(() => ({ collapsible }), [collapsible]);
  if (!collapsible) {
    return (
      <WidgetFrameContext.Provider value={value}>
        <div data-slot="widget-frame" className={cn(frameClassName, className)} {...props}>
          {children}
        </div>
      </WidgetFrameContext.Provider>
    );
  }
  return (
    <WidgetFrameContext.Provider value={value}>
      <Collapsible
        data-slot="widget-frame"
        defaultOpen={defaultOpen}
        className={cn(frameClassName, className)}
        {...props}
      >
        {children}
      </Collapsible>
    </WidgetFrameContext.Provider>
  );
}

function WidgetFrameHeader({ className, children, ...props }: React.ComponentProps<"div">) {
  const { collapsible } = React.useContext(WidgetFrameContext);
  if (!collapsible) {
    return (
      <Item
        data-slot="widget-frame-header"
        size="xs"
        className={cn("w-full", className)}
        {...props}
      >
        {children}
      </Item>
    );
  }
  return (
    <CollapsibleTrigger
      render={
        <Item
          data-slot="widget-frame-header"
          size="xs"
          className={cn(
            "group/widget-frame w-full cursor-pointer rounded-none hover:bg-accent",
            className,
          )}
          {...props}
        />
      }
    >
      {children}
      <ChevronRight
        aria-hidden="true"
        data-slot="widget-frame-chevron"
        className="ml-auto size-4 shrink-0 text-muted-foreground transition-transform group-aria-expanded/widget-frame:rotate-90"
      />
    </CollapsibleTrigger>
  );
}

function WidgetFrameContent({
  className,
  ...props
}: React.ComponentProps<typeof CollapsibleContent>) {
  return (
    <CollapsibleContent
      data-slot="widget-frame-content"
      className={cn("border-t border-border", className)}
      {...props}
    />
  );
}

export { WidgetFrame, WidgetFrameHeader, WidgetFrameContent };
```

Implementation notes: `CollapsibleTrigger` (Base UI) renders a `button` by
default; the `render={<Item …/>}` composition makes the Item the button (Item
is `useRender`-based and accepts `render`-merged props — same mechanism the
ui layer already uses in `combobox.tsx`). If `Item`'s div cannot receive the
trigger's button semantics, fall back to `render` on Item:
`<CollapsibleTrigger render={<div/>}>` wrapping the Item — keep
`role`/`aria-expanded` on the interactive element and note the deviation in
your report.

- [ ] **Step 4: Run to verify pass**

Run: `npx vitest run src/components/ui/widget-frame.test.tsx`
Expected: PASS (3/3).

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npx oxfmt src/components/ui/widget-frame.tsx src/components/ui/widget-frame.test.tsx
git add src/components/ui/widget-frame.tsx src/components/ui/widget-frame.test.tsx
git commit -m "feat(ui): widget-frame primitive composing Item and Collapsible

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: ViewFullOutput + UnknownToolWidget onto Button/Spinner/CodeBlock/WidgetFrame

**Files:**

- Modify: `src/views/chat/tool-widgets/ViewFullOutput.tsx`
- Modify: `src/views/chat/tool-widgets/UnknownToolWidget.tsx`
- Test: `src/views/chat/tool-widgets/ViewFullOutput.test.tsx`, and the fallback assertions in `src/views/workspace/TranscriptRow.test.tsx` (do not weaken)

**Interfaces:**

- Consumes: `CodeBlock`, `CodeInline` (Task 1), `WidgetFrame`/`WidgetFrameHeader`/`WidgetFrameContent` (Task 2), `Button` (`variant="ghost" size="sm"` — note the custom Button has no `link` variant; ghost + underline comes from… nothing: use ghost, text-only), `Spinner`.
- Produces: `ViewFullOutput({ path })` unchanged signature; testids `view-full-output-button`, `view-full-output-content` preserved. `UnknownToolWidget({ detail })` unchanged; testid `unknown-tool-widget` preserved.

- [ ] **Step 1: Update tests**

`ViewFullOutput.test.tsx`: keep all behavior assertions (button click → IPC fetch → content). Change only chrome-coupled ones: the button is now a `Button` (`data-slot` per the custom button — it has none; assert `screen.getByTestId("view-full-output-button")` has `tagName === "BUTTON"` and text "View full output"), the loaded content asserts `toHaveAttribute("data-slot", "code-block")` on `view-full-output-content`. Loading state: assert the button is disabled and contains a `[data-slot="spinner"]` element while the IPC promise is pending (mirror the existing pending-promise pattern in that file).

- [ ] **Step 2: Verify failures**

Run: `npx vitest run src/views/chat/tool-widgets/ViewFullOutput.test.tsx`
Expected: FAIL on the new structural assertions.

- [ ] **Step 3: Rewrite the two components**

`ViewFullOutput.tsx` — keep imports/state/IPC logic (lines 1–33) identical; replace both return blocks:

```tsx
if (fullText != null) {
  return <CodeBlock data-testid="view-full-output-content">{fullText}</CodeBlock>;
}

return (
  <div className="flex flex-col items-start gap-1 px-3 py-1">
    <Button
      type="button"
      variant="ghost"
      size="sm"
      onClick={load}
      disabled={loading}
      data-testid="view-full-output-button"
    >
      {loading && <Spinner role="presentation" aria-label={undefined} />}
      {loading ? "Loading…" : "View full output"}
    </Button>
    {error && (
      <Alert variant="destructive">
        <AlertDescription>{error}</AlertDescription>
      </Alert>
    )}
  </div>
);
```

with imports:

```tsx
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { CodeBlock } from "@/components/ui/code-block";
import { Spinner } from "@/components/ui/spinner";
```

(The old `border-t border-border`, underline link styling, and `text-xs text-destructive` error line are gone; callers' frames provide separation.)

`UnknownToolWidget.tsx` — full replacement of the returned JSX:

```tsx
import { Wrench } from "lucide-react";
import { CodeBlock } from "@/components/ui/code-block";
import { ItemContent, ItemMedia, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "@/components/ui/widget-frame";
import type { ToolResultDetail, UnknownToolDetail } from "@/lib/ipc";

interface UnknownToolWidgetProps {
  detail: ToolResultDetail | UnknownToolDetail;
}

/**
 * FR-011/SC-004: the fallback for any `toolName` without a dedicated
 * widget (including a completely unrecognized one, or a tool with a
 * dedicated widget that simply hasn't landed yet) — the tool's name plus a
 * readable rendering of its detail payload, never blank or broken.
 */
export default function UnknownToolWidget({ detail }: UnknownToolWidgetProps) {
  return (
    <WidgetFrame collapsible data-testid="unknown-tool-widget">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <Wrench />
        </ItemMedia>
        <ItemContent>
          <ItemTitle>{detail.toolName}</ItemTitle>
        </ItemContent>
      </WidgetFrameHeader>
      <WidgetFrameContent>
        <CodeBlock>{JSON.stringify(detail, null, 2)}</CodeBlock>
      </WidgetFrameContent>
    </WidgetFrame>
  );
}
```

Check `TranscriptRow.test.tsx`'s fallback tests (they assert
`unknown-tool-widget` renders for unrecognized/unparseable payloads) still
pass — the JSON body is now collapsed by default, so if a test asserts on the
JSON text content, expand first with `userEvent.click` on the header button
or assert the header tool name instead. Do not weaken the never-blank
contract.

- [ ] **Step 4: Verify pass**

Run: `npx vitest run src/views/chat/tool-widgets/ViewFullOutput.test.tsx src/views/workspace/TranscriptRow.test.tsx`
Expected: PASS.

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npx oxfmt src/views/chat/tool-widgets/ViewFullOutput.tsx src/views/chat/tool-widgets/UnknownToolWidget.tsx src/views/chat/tool-widgets/ViewFullOutput.test.tsx
git add -A ':!/.superpowers' && git commit -m "refactor(widgets): ViewFullOutput and UnknownToolWidget on stock primitives

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: BashWidget onto the frame

**Files:**

- Modify: `src/views/chat/tool-widgets/BashWidget.tsx`
- Test: `src/views/chat/tool-widgets/BashWidget.test.tsx`

**Interfaces:**

- Consumes: `WidgetFrame`/`WidgetFrameHeader`/`WidgetFrameContent`, `CodeBlock`, `CodeInline`, `Badge`, `Spinner`, `Alert`/`AlertDescription`, `ItemContent`/`ItemMedia`/`ItemTitle`/`ItemActions`, `Terminal` from lucide-react, existing `ViewFullOutput`.
- Produces: same props (`detail: BashDetail`). Testids preserved: `bash-widget`, `bash-status`, `bash-command`, `bash-stdout`, `bash-stderr`, `bash-output-truncated`. Text contracts preserved: "Running…", "Failed to run", "Success", `Failed (exit N)`, "Output truncated", `$ <command>`.

- [ ] **Step 1: Update tests**

In `BashWidget.test.tsx` (163 lines; keep every fixture and behavioral test):

- Drop class assertions on the old chrome (`bg-emerald-500/10`, `text-sky-600`, borders) if present; replace with structural ones:
  - running: `bash-status` contains a `[data-slot="spinner"]` and text "Running…"; the frame is expanded (command visible).
  - success: `bash-status` — assert a `Badge` (`[data-slot="badge"]`… note: the ui Badge sets no data-slot; assert text "Success" inside `screen.getByTestId("bash-status")` and exit/token text `exit 0 · 1.2k tok` per existing fixtures).
  - failure: text `Failed (exit 2)`; spawn-failed branch renders `role="alert"` with the error text.
- Collapsed-by-default: completed Bash renders header (command + status) with `bash-stdout` NOT visible until the header is clicked (`userEvent.click(screen.getByRole("button"))` — match Task 2's real Base UI visibility behavior).
- Running/pending stays expanded: `bash-command` visible without clicks.

- [ ] **Step 2: Verify failures**

Run: `npx vitest run src/views/chat/tool-widgets/BashWidget.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Rewrite the component**

Full new `BashWidget.tsx` (keep the `truncatedLines` helper and header comment verbatim):

```tsx
import { Terminal } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { CodeBlock, CodeInline } from "@/components/ui/code-block";
import { ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item";
import { Spinner } from "@/components/ui/spinner";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "@/components/ui/widget-frame";
import type { BashDetail } from "@/lib/ipc";
import { formatTokenCount } from "@/lib/formatTokenCount";
import ViewFullOutput from "./ViewFullOutput";

interface BashWidgetProps {
  detail: BashDetail;
}

const OUTPUT_LINE_CAP = 50;

function truncatedLines(text: string): { shown: string; truncated: boolean } {
  const lines = text.split("\n");
  if (lines.length <= OUTPUT_LINE_CAP) return { shown: text, truncated: false };
  return { shown: lines.slice(0, OUTPUT_LINE_CAP).join("\n"), truncated: true };
}

/**
 * US2/FR-003: command + output shown together, terminal-style — plain
 * monospace text rather than `xterm.js` (research.md § 6: this is a
 * static, already-complete result, not an interactive terminal).
 */
export default function BashWidget({ detail }: BashWidgetProps) {
  // Pending branch: outcome absent means the command is still running
  if (!detail.outcome) {
    return (
      <WidgetFrame collapsible defaultOpen data-testid="bash-widget">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <Terminal />
          </ItemMedia>
          <ItemContent>
            <ItemTitle data-testid="bash-status">
              <Spinner role="presentation" aria-label={undefined} />
              Running…
            </ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
        <WidgetFrameContent>
          <CodeBlock data-testid="bash-command">$ {detail.command}</CodeBlock>
        </WidgetFrameContent>
      </WidgetFrame>
    );
  }

  if (!detail.outcome.ok) {
    return (
      <WidgetFrame data-testid="bash-widget">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <Terminal />
          </ItemMedia>
          <ItemContent>
            <ItemTitle data-testid="bash-command">
              <CodeInline>$ {detail.command}</CodeInline>
            </ItemTitle>
            <ItemDescription data-testid="bash-status">Failed to run</ItemDescription>
          </ItemContent>
        </WidgetFrameHeader>
        <div className="p-3 pt-0">
          <Alert variant="destructive">
            <AlertDescription>{detail.outcome.error}</AlertDescription>
          </Alert>
        </div>
      </WidgetFrame>
    );
  }

  const { exitCode } = detail.outcome;
  const succeeded = exitCode === 0;
  // New rows only carry a bounded preview (stdoutPreview/stderrPreview);
  // legacy rows persisted before the payload-files design still carry the
  // full stdout/stderr inline.
  const stdout = detail.outcome.stdoutPreview ?? detail.outcome.stdout ?? "";
  const stderr = detail.outcome.stderrPreview ?? detail.outcome.stderr ?? "";
  const payloadPath = detail.payloadRef ?? detail.offloadedTo;
  const stdoutTrunc = truncatedLines(stdout);
  const stderrTrunc = truncatedLines(stderr);

  return (
    <WidgetFrame collapsible data-testid="bash-widget">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <Terminal />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="bash-command">
            <CodeInline>$ {detail.command}</CodeInline>
          </ItemTitle>
        </ItemContent>
        <span className="flex items-center gap-2" data-testid="bash-status">
          <Badge variant={succeeded ? "secondary" : "destructive"}>
            {succeeded ? "Success" : `Failed (exit ${exitCode})`}
          </Badge>
          <Badge variant="outline">
            exit {exitCode}
            {detail.tokenCount != null && ` · ${formatTokenCount(detail.tokenCount)} tok`}
          </Badge>
        </span>
      </WidgetFrameHeader>
      <WidgetFrameContent>
        {stdout && <CodeBlock data-testid="bash-stdout">{stdoutTrunc.shown}</CodeBlock>}
        {stderr && (
          <CodeBlock tone="destructive" data-testid="bash-stderr">
            {stderrTrunc.shown}
          </CodeBlock>
        )}
        {(stdoutTrunc.truncated || stderrTrunc.truncated) && (
          <ItemDescription className="px-3 py-1" data-testid="bash-output-truncated">
            Output truncated
          </ItemDescription>
        )}
        {payloadPath && <ViewFullOutput path={payloadPath} />}
      </WidgetFrameContent>
    </WidgetFrame>
  );
}
```

- [ ] **Step 4: Verify pass**

Run: `npx vitest run src/views/chat/tool-widgets/BashWidget.test.tsx src/views/workspace/TranscriptTurn.test.tsx`
Expected: PASS (TranscriptTurn renders the pending Bash widget — expanded branch must keep working).

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npx oxfmt src/views/chat/tool-widgets/BashWidget.tsx src/views/chat/tool-widgets/BashWidget.test.tsx
git add -A ':!/.superpowers' && git commit -m "refactor(widgets): BashWidget on WidgetFrame and CodeBlock

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: EditDiffWidget onto CodeBlockLine

**Files:**

- Modify: `src/views/chat/tool-widgets/EditDiffWidget.tsx`
- Test: `src/views/chat/tool-widgets/EditDiffWidget.test.tsx`

**Interfaces:**

- Consumes: `WidgetFrame` (collapsible, `defaultOpen` — expanded is the spec'd default), `CodeBlock`/`CodeBlockLine`, `Badge`, `Alert`, `FilePen` from lucide-react, `ItemContent`/`ItemMedia`/`ItemTitle`/`ItemActions`.
- Produces: same props. Testids preserved: `edit-diff`, `edit-failed`, `diff-added`, `diff-removed`.

- [ ] **Step 1: Update tests**

`EditDiffWidget.test.tsx` (43 lines): keep the added/removed content assertions (`diff-added`/`diff-removed` presence and text). Replace any class assertions (`bg-emerald-500/15`) with `data-variant` checks on the contained lines: `screen.getByTestId("diff-added").querySelector('[data-slot="code-block-line"]')` has `data-variant="added"`. Add: header shows the file path and `+N`/`−N` Badges (compute from the fixture's oldString/newString).

- [ ] **Step 2: Verify failures**

Run: `npx vitest run src/views/chat/tool-widgets/EditDiffWidget.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Rewrite**

```tsx
import { diffLines } from "diff";
import { FilePen } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { CodeBlock, CodeBlockLine } from "@/components/ui/code-block";
import { ItemContent, ItemMedia, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "@/components/ui/widget-frame";
import type { EditDetail } from "@/lib/ipc";

interface EditDiffWidgetProps {
  detail: EditDetail;
}

/**
 * US1/FR-002: a real, labeled diff for `Edit` tool calls — computed
 * client-side from the raw `oldString`/`newString` the dispatch layer
 * already captured (research.md § 6/§ 4), not a heavier editor component.
 */
export default function EditDiffWidget({ detail }: EditDiffWidgetProps) {
  if (!detail.outcome.ok) {
    return (
      <WidgetFrame data-testid="edit-failed">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <FilePen />
          </ItemMedia>
          <ItemContent>
            <ItemTitle>{detail.filePath ?? "(no file path)"}</ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
        <div className="p-3 pt-0">
          <Alert variant="destructive">
            <AlertDescription>{detail.outcome.error}</AlertDescription>
          </Alert>
        </div>
      </WidgetFrame>
    );
  }

  const changes = diffLines(detail.oldString, detail.newString);
  const lineCount = (value: string) => value.replace(/\n$/, "").split("\n").length;
  const addedCount = changes.filter((c) => c.added).reduce((n, c) => n + lineCount(c.value), 0);
  const removedCount = changes.filter((c) => c.removed).reduce((n, c) => n + lineCount(c.value), 0);

  return (
    <WidgetFrame collapsible defaultOpen data-testid="edit-diff">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <FilePen />
        </ItemMedia>
        <ItemContent>
          <ItemTitle>{detail.filePath}</ItemTitle>
        </ItemContent>
        <span className="flex items-center gap-2">
          <Badge variant="outline">+{addedCount}</Badge>
          <Badge variant="outline">−{removedCount}</Badge>
        </span>
      </WidgetFrameHeader>
      <WidgetFrameContent>
        <CodeBlock className="p-0 whitespace-pre">
          {changes.map((change, i) => {
            const lines = change.value.replace(/\n$/, "").split("\n");
            const testId = change.added
              ? "diff-added"
              : change.removed
                ? "diff-removed"
                : undefined;
            const prefix = change.added ? "+" : change.removed ? "-" : " ";
            const variant = change.added ? "added" : change.removed ? "removed" : "default";
            return (
              <div key={i} data-testid={testId}>
                {lines.map((line, j) => (
                  <CodeBlockLine key={j} variant={variant}>
                    {prefix} {line}
                  </CodeBlockLine>
                ))}
              </div>
            );
          })}
        </CodeBlock>
      </WidgetFrameContent>
    </WidgetFrame>
  );
}
```

(`p-0 whitespace-pre` on CodeBlock: layout-only overrides so the lines own
their padding and long diff lines scroll horizontally instead of wrapping —
same as the old `<pre>`.)

- [ ] **Step 4: Verify pass**

Run: `npx vitest run src/views/chat/tool-widgets/EditDiffWidget.test.tsx`
Expected: PASS.

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npx oxfmt src/views/chat/tool-widgets/EditDiffWidget.tsx src/views/chat/tool-widgets/EditDiffWidget.test.tsx
git add -A ':!/.superpowers' && git commit -m "refactor(widgets): EditDiffWidget on WidgetFrame and CodeBlockLine

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: WriteWidget, TaskWidget, AskUserQuestionWidget (three small frames)

**Files:**

- Modify: `src/views/chat/tool-widgets/WriteWidget.tsx`, `TaskWidget.tsx`, `AskUserQuestionWidget.tsx`
- Test: their three colocated `.test.tsx` files

**Interfaces:**

- Consumes: frame + Item family + `Badge`, `Spinner`, `Alert`; lucide `FilePlus`, `Bot`, `MessageCircleQuestion`.
- Produces: same props. Testids preserved: `write-widget`, `write-header`, `write-body`; `task-widget`, `task-status`; `question-answered`. Text contracts preserved verbatim, including both "Interrupted — the app closed before this …" strings, "Running…", "Complete", "You chose"/"You replied", "Write · N bytes".

- [ ] **Step 1: Update the three tests**

Replace palette-class assertions (`text-emerald-700`, `text-amber-600`, `text-sky-600`, `border-emerald-500/30`) with structure:

- WriteWidget success: header-only frame (`data-slot="widget-frame"`, no `role="button"`), `write-header` shows path, `write-body` shows "Write · 170 bytes"; failure branch renders `role="alert"` with the error.
- TaskWidget: running → `task-status` contains `[data-slot="spinner"]` + "Running…"; complete → Badge text "Complete"; interrupted → Badge text starting "Interrupted"; prompt text always visible (this frame stays header-only with the prompt as `ItemDescription` — the spec's collapsed-body default applies to widgets WITH bodies; Task's body is one line of text, keep it visible; note this as a spec deviation in your report if you disagree).
- AskUserQuestion: question + answer rows visible without clicks (defaultOpen); interrupted branch keeps its exact string.

- [ ] **Step 2: Verify failures**

Run: `npx vitest run src/views/chat/tool-widgets/WriteWidget.test.tsx src/views/chat/tool-widgets/TaskWidget.test.tsx src/views/chat/tool-widgets/AskUserQuestionWidget.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Rewrite the three components**

`WriteWidget.tsx`:

```tsx
import { FilePlus } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameHeader } from "@/components/ui/widget-frame";
import type { WriteDetail } from "@/lib/ipc";

interface WriteWidgetProps {
  detail: WriteDetail;
}

/** US4/FR-006: distinct from ReadWidget and from a plain reply — a compact file-reference card for a created/overwritten file. */
export default function WriteWidget({ detail }: WriteWidgetProps) {
  if (!detail.outcome.ok) {
    return (
      <WidgetFrame data-testid="write-widget">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <FilePlus />
          </ItemMedia>
          <ItemContent>
            <ItemTitle>Write {detail.filePath}</ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
        <div className="p-3 pt-0">
          <Alert variant="destructive">
            <AlertDescription>{detail.outcome.error}</AlertDescription>
          </Alert>
        </div>
      </WidgetFrame>
    );
  }

  return (
    <WidgetFrame data-testid="write-widget">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <FilePlus />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="write-header">{detail.filePath}</ItemTitle>
          <ItemDescription data-testid="write-body">
            Write · {detail.byteCount} bytes
          </ItemDescription>
        </ItemContent>
        <Badge variant="secondary">Written</Badge>
      </WidgetFrameHeader>
    </WidgetFrame>
  );
}
```

`TaskWidget.tsx` (keep both explanatory comments):

```tsx
import { Bot } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item";
import { Spinner } from "@/components/ui/spinner";
import { WidgetFrame, WidgetFrameHeader } from "@/components/ui/widget-frame";
import type { TaskDetail } from "@/lib/ipc";

interface TaskWidgetProps {
  detail: TaskDetail;
}

/**
 * US4/FR-010: a running/complete status indicator only — the subagent's
 * own intermediate tool calls live on its own conversation row and are
 * never surfaced here (FR-015/SC-008, unchanged by this feature).
 */
export default function TaskWidget({ detail }: TaskWidgetProps) {
  // `interrupted` wins over `state`: a healed crash-orphaned delegation
  // carries state:"complete" (the shape constraint) but never finished —
  // a green Complete badge would be a lie.
  const interrupted = detail.interrupted === true;
  const running = !interrupted && detail.state === "running";
  return (
    <WidgetFrame data-testid="task-widget">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <Bot />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="task-status">
            {running && <Spinner role="presentation" aria-label={undefined} />}
            {interrupted
              ? "Interrupted — the app closed before this finished"
              : running
                ? "Running…"
                : "Complete"}
          </ItemTitle>
          <ItemDescription>{detail.prompt}</ItemDescription>
        </ItemContent>
        {!running && (
          <Badge variant={interrupted ? "outline" : "secondary"}>
            {interrupted ? "Interrupted" : "Complete"}
          </Badge>
        )}
      </WidgetFrameHeader>
    </WidgetFrame>
  );
}
```

Note: `task-status` keeps the full interrupted sentence in the title (text
contract); the Badge is supplementary. If a test asserts the exact status
text once, the title carries it.

`AskUserQuestionWidget.tsx` (keep both header comments verbatim):

```tsx
import { MessageCircleQuestion } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameHeader } from "@/components/ui/widget-frame";
import type { AskUserQuestionDetail } from "@/lib/ipc";

interface AskUserQuestionWidgetProps {
  detail: AskUserQuestionDetail;
}

export default function AskUserQuestionWidget({ detail }: AskUserQuestionWidgetProps) {
  const answer = detail.answer ?? [];
  const isFreeText = !answer.every((a) => detail.options.some((o) => o.label === a));

  return (
    <WidgetFrame data-testid="question-answered">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <MessageCircleQuestion />
        </ItemMedia>
        <ItemContent>
          <ItemDescription>{detail.question}</ItemDescription>
          {detail.interrupted ? (
            <ItemTitle>Interrupted — the app closed before this was answered</ItemTitle>
          ) : (
            <ItemTitle>
              {isFreeText ? "You replied" : "You chose"}: {answer.join(", ")}
            </ItemTitle>
          )}
        </ItemContent>
        {detail.interrupted && <Badge variant="outline">Interrupted</Badge>}
      </WidgetFrameHeader>
    </WidgetFrame>
  );
}
```

(Preserve the original file-header doc comments when rewriting — move them
above the component unchanged.)

- [ ] **Step 4: Verify pass**

Run: `npx vitest run src/views/chat/tool-widgets/WriteWidget.test.tsx src/views/chat/tool-widgets/TaskWidget.test.tsx src/views/chat/tool-widgets/AskUserQuestionWidget.test.tsx`
Expected: PASS.

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npx oxfmt src/views/chat/tool-widgets/WriteWidget.tsx src/views/chat/tool-widgets/TaskWidget.tsx src/views/chat/tool-widgets/AskUserQuestionWidget.tsx src/views/chat/tool-widgets/WriteWidget.test.tsx src/views/chat/tool-widgets/TaskWidget.test.tsx src/views/chat/tool-widgets/AskUserQuestionWidget.test.tsx
git add -A ':!/.superpowers' && git commit -m "refactor(widgets): Write, Task, and AskUserQuestion cards on WidgetFrame

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: ReadWidget + ReadPreview onto the frame; Empty/Spinner/Attachment internals

**Files:**

- Modify: `src/views/chat/tool-widgets/ReadWidget.tsx`, `ReadPreview.tsx`
- Test: `ReadWidget.test.tsx`, `ReadPreview.test.tsx`

**Interfaces:**

- Consumes: frame + `CodeBlock`, `Badge`, `Alert`, `Spinner`, `Empty`/`EmptyHeader`/`EmptyTitle`/`EmptyDescription`, lucide `FileText`; `ViewFullOutput` (Task 3 form).
- Produces: same props; `readPreviewKind` export unchanged. Testids preserved: `read-widget`, `read-summary`, `read-preview`, `read-markdown-preview`, `read-text-preview`, `read-preview-loading`, `read-preview-error`, `read-preview-unavailable`, `read-image-preview`, `read-video-preview`, `read-audio-preview`. Collapsed-by-default (spec contract from 2026-07-09 survives).

- [ ] **Step 1: Update tests**

- `ReadWidget.test.tsx`: the summary line keeps its text (`Read <path> · <bytes>[ · N tok]`) but moves into the frame header — assert via `read-summary` testid text. Collapse contract: `read-preview` body not visible until header click (was ToolDisclosure's `<details>` open toggling; now `aria-expanded` on the trigger button). Failure branch: `role="alert"` + error text.
- `ReadPreview.test.tsx`: loading → `[data-slot="spinner"]` inside `read-preview-loading`; unavailable → `read-preview-unavailable` has `data-slot="empty"` (or contains it) with the same "Preview unavailable…" text; error → keep text assertion, now inside `EmptyDescription`. Markdown/text/media assertions unchanged except `read-text-preview` gains `data-slot="code-block"`.

- [ ] **Step 2: Verify failures**

Run: `npx vitest run src/views/chat/tool-widgets/ReadWidget.test.tsx src/views/chat/tool-widgets/ReadPreview.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Rewrite**

`ReadWidget.tsx`:

```tsx
import { FileText } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { ItemContent, ItemMedia, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "@/components/ui/widget-frame";
import type { ReadDetail } from "@/lib/ipc";
import { formatByteCount } from "@/lib/formatByteCount";
import { formatTokenCount } from "@/lib/formatTokenCount";
import ReadPreview from "./ReadPreview";
import ViewFullOutput from "./ViewFullOutput";

interface ReadWidgetProps {
  detail: ReadDetail;
}

/** US4/FR-005: a compact file-reference card, not a plain-text dump of the file's contents. */
export default function ReadWidget({ detail }: ReadWidgetProps) {
  if (!detail.outcome.ok) {
    return (
      <WidgetFrame data-testid="read-widget">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <FileText />
          </ItemMedia>
          <ItemContent>
            <ItemTitle>Read {detail.filePath}</ItemTitle>
          </ItemContent>
        </WidgetFrameHeader>
        <div className="p-3 pt-0">
          <Alert variant="destructive">
            <AlertDescription>{detail.outcome.error}</AlertDescription>
          </Alert>
        </div>
      </WidgetFrame>
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
    <WidgetFrame collapsible data-testid="read-widget">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <FileText />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="read-summary">Read {detail.filePath}</ItemTitle>
        </ItemContent>
        <span className="flex items-center gap-2">
          <Badge variant="outline">{byteCount}</Badge>
          {tokenCount != null && <Badge variant="outline">{tokenCount}</Badge>}
        </span>
      </WidgetFrameHeader>
      <WidgetFrameContent data-testid="read-preview">
        <div className="max-h-80 overflow-y-auto p-3">
          <ReadPreview detail={detail} />
          {payloadPath && <ViewFullOutput path={payloadPath} />}
        </div>
      </WidgetFrameContent>
    </WidgetFrame>
  );
}
```

`ReadPreview.tsx` — keep everything from the top of the file through
`NativeReadPreview`'s effect unchanged (extension tables, `readPreviewKind`,
data fetching). Replace only the presentational returns:

- text branch: `<CodeBlock data-testid="read-text-preview">{content}</CodeBlock>` (drop the old class string)
- loading: `<p className="flex items-center gap-2" data-testid="read-preview-loading"><Spinner role="presentation" aria-label={undefined} />Loading preview…</p>`
- error:

```tsx
<Empty data-testid="read-preview-error">
  <EmptyHeader>
    <EmptyTitle>Preview failed</EmptyTitle>
    <EmptyDescription>{state.error}</EmptyDescription>
  </EmptyHeader>
</Empty>
```

- `PreviewUnavailable`:

```tsx
<Empty data-testid="read-preview-unavailable">
  <EmptyHeader>
    <EmptyDescription>Preview unavailable{filePath ? ` for ${filePath}` : ""}</EmptyDescription>
  </EmptyHeader>
</Empty>
```

- media branches: keep the native `<img>/<video>/<audio>` elements and their
  testids; replace `rounded-md` classes with none (the frame body clips) and
  keep `max-h-80 max-w-full object-contain` / `max-h-80 w-full` / `w-full`
  (sizing = layout, allowed).

New imports: `CodeBlock` from `@/components/ui/code-block`; `Spinner`;
`Empty, EmptyDescription, EmptyHeader, EmptyTitle` from
`@/components/ui/empty`.

- [ ] **Step 4: Verify pass**

Run: `npx vitest run src/views/chat/tool-widgets/ReadWidget.test.tsx src/views/chat/tool-widgets/ReadPreview.test.tsx`
Expected: PASS.

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npx oxfmt src/views/chat/tool-widgets/ReadWidget.tsx src/views/chat/tool-widgets/ReadPreview.tsx src/views/chat/tool-widgets/ReadWidget.test.tsx src/views/chat/tool-widgets/ReadPreview.test.tsx
git add -A ':!/.superpowers' && git commit -m "refactor(widgets): Read widget and preview on WidgetFrame, CodeBlock, Empty

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 8: SearchResultsWidget onto the frame; DELETE ToolDisclosure

**Files:**

- Modify: `src/views/chat/tool-widgets/SearchResultsWidget.tsx`
- Delete: `src/views/chat/tool-widgets/ToolDisclosure.tsx`, `ToolDisclosure.test.tsx`
- Test: `SearchResultsWidget.test.tsx`

**Interfaces:**

- Consumes: frame + `Badge`, `Empty`, `Item`/`ItemGroup`/`ItemTitle`/`ItemDescription`, `CodeInline`, lucide `Search`.
- Produces: same props. Testids preserved: `search-widget`, `search-summary`, `search-results`, `search-context`, `search-match`, `search-no-matches`, `search-interrupted`. Text contracts preserved: count labels (`N matches`/`N files`), "No files matched"/"No matches found", the interrupted sentence.

- [ ] **Step 1: Update tests**

`SearchResultsWidget.test.tsx`: summary content moves to the header
(`search-summary` testid keeps the same text pieces); collapse contract via
trigger `aria-expanded` + body visibility (same pattern as Task 7); match
rows keep `search-match` testids and text (`path:line: content` for Grep);
`search-no-matches` asserts inside a `data-slot="empty"`; interrupted branch:
`search-interrupted` keeps its sentence, add Badge "Interrupted" presence.
DELETE the `ToolDisclosure.test.tsx` expectations wholesale (file removed).

- [ ] **Step 2: Verify failures**

Run: `npx vitest run src/views/chat/tool-widgets/SearchResultsWidget.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Rewrite + delete**

`SearchResultsWidget.tsx`:

```tsx
import { Search } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { CodeInline } from "@/components/ui/code-block";
import { Empty, EmptyDescription, EmptyHeader } from "@/components/ui/empty";
import { ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item";
import { WidgetFrame, WidgetFrameContent, WidgetFrameHeader } from "@/components/ui/widget-frame";
import type { GlobDetail, GrepDetail } from "@/lib/ipc";
import { formatTokenCount } from "@/lib/formatTokenCount";

interface SearchResultsWidgetProps {
  detail: GlobDetail | GrepDetail;
}

/** US4/FR-007: a match list for Glob (filenames) and Grep (file:line:content), not an undifferentiated data dump. */
export default function SearchResultsWidget({ detail }: SearchResultsWidgetProps) {
  const isGrep = detail.toolName === "Grep";

  if (detail.interrupted) {
    return (
      <WidgetFrame data-testid="search-widget">
        <WidgetFrameHeader>
          <ItemMedia variant="icon">
            <Search />
          </ItemMedia>
          <ItemContent>
            <ItemTitle>
              {detail.toolName} <CodeInline>{detail.pattern}</CodeInline>
            </ItemTitle>
            <ItemDescription data-testid="search-interrupted">
              Interrupted — the app closed before this search finished
            </ItemDescription>
          </ItemContent>
          <Badge variant="outline">Interrupted</Badge>
        </WidgetFrameHeader>
      </WidgetFrame>
    );
  }

  const count = detail.matches.length;
  const countLabel = isGrep ? `${count} ${count === 1 ? "match" : "matches"}` : `${count} files`;

  return (
    <WidgetFrame collapsible data-testid="search-widget">
      <WidgetFrameHeader>
        <ItemMedia variant="icon">
          <Search />
        </ItemMedia>
        <ItemContent>
          <ItemTitle data-testid="search-summary">
            {detail.toolName} <CodeInline>{detail.pattern}</CodeInline>
          </ItemTitle>
        </ItemContent>
        <span className="flex items-center gap-2">
          <Badge variant="outline">{countLabel}</Badge>
          {detail.tokenCount != null && (
            <Badge variant="outline">{formatTokenCount(detail.tokenCount)} tok</Badge>
          )}
        </span>
      </WidgetFrameHeader>
      <WidgetFrameContent data-testid="search-results">
        <div className="max-h-80 space-y-2 overflow-y-auto p-3">
          <SearchContext detail={detail} />
          {isGrep ? (
            <GrepResults detail={detail as GrepDetail} />
          ) : (
            <GlobResults detail={detail as GlobDetail} />
          )}
        </div>
      </WidgetFrameContent>
    </WidgetFrame>
  );
}

function SearchContext({ detail }: { detail: GlobDetail | GrepDetail }) {
  const parts = [
    detail.path ? `path: ${detail.path}` : null,
    detail.toolName === "Grep" && detail.glob ? `glob: ${detail.glob}` : null,
  ].filter(Boolean);

  if (parts.length === 0) return null;

  return (
    <ItemDescription data-testid="search-context">
      <CodeInline>{parts.join(" · ")}</CodeInline>
    </ItemDescription>
  );
}

function GlobResults({ detail }: { detail: GlobDetail }) {
  if (detail.matches.length === 0) {
    return (
      <Empty data-testid="search-no-matches">
        <EmptyHeader>
          <EmptyDescription>No files matched</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <ul className="space-y-0.5">
      {detail.matches.map((path, i) => (
        <li key={i} data-testid="search-match" className="truncate">
          <CodeInline>{path}</CodeInline>
        </li>
      ))}
    </ul>
  );
}

function GrepResults({ detail }: { detail: GrepDetail }) {
  if (detail.matches.length === 0) {
    return (
      <Empty data-testid="search-no-matches">
        <EmptyHeader>
          <EmptyDescription>No matches found</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <ul className="space-y-0.5">
      {detail.matches.map((m, i) => (
        <li key={i} data-testid="search-match" className="truncate">
          <CodeInline>
            {m.path}:{m.lineNumber}: {m.line}
          </CodeInline>
        </li>
      ))}
    </ul>
  );
}
```

Then:

```bash
git rm src/views/chat/tool-widgets/ToolDisclosure.tsx src/views/chat/tool-widgets/ToolDisclosure.test.tsx
grep -rn "ToolDisclosure" src/ tests/   # expected: no matches
```

- [ ] **Step 4: Verify pass**

Run: `npx vitest run src/views/chat/tool-widgets/`
Expected: PASS (all widget suites; ToolDisclosure suite gone).

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npx oxfmt src/views/chat/tool-widgets/SearchResultsWidget.tsx src/views/chat/tool-widgets/SearchResultsWidget.test.tsx
git add -A ':!/.superpowers' && git commit -m "refactor(widgets): search results on WidgetFrame; delete ToolDisclosure

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 9: WidgetGallery pass

**Files:**

- Modify: `src/views/design-system/WidgetGallery.tsx`
- Test: `src/App.test.tsx` only if it smoke-renders the gallery (check first)

**Interfaces:**

- Consumes: nothing new — the gallery imports widgets by their unchanged default exports.

- [ ] **Step 1: Compile-check the gallery**

Run: `npx tsc -b && npx vitest run src/views/design-system/ 2>/dev/null; npx vitest run src/App.test.tsx`
The gallery has no colocated test; `tsc` + App smoke tests are the gate. If imports/props still line up (they should — widget props are unchanged), this task reduces to fixing the stale doc comment at `WidgetGallery.tsx:63` ("the components `MessageContent`" → `TranscriptRow`) and verifying the gallery section headings still describe the rendered states (adjust copy strings only where they name removed visuals, e.g. a caption that says "emerald header" — search the file for such copy).

- [ ] **Step 2: Commit**

```bash
npx oxfmt src/views/design-system/WidgetGallery.tsx
git add src/views/design-system/WidgetGallery.tsx && git commit -m "chore(gallery): align widget gallery copy with unified frames

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

(If nothing needed changing beyond the comment, say so in the report — do not invent work.)

---

### Task 10: Compliance sweep + full gates + runtime check

**Files:**

- Possibly modify: whatever the sweep flags.

- [ ] **Step 1: Compliance sweep**

```bash
FILES=$(ls src/views/chat/tool-widgets/*.tsx | grep -v UserAskWidget | grep -v test)
grep -n "\[[a-z-]*:" $FILES                      # expected: EMPTY
grep -nE "(emerald|sky|amber|red|green|blue)-[0-9]" $FILES   # expected: EMPTY
grep -nE '(^|[ "`])(bg|text|border|shadow|rounded|font)-' $FILES | grep -v "data-testid\|//"   # expected: EMPTY (text-left/right/center alignment hits are fine)
```

Every hit must be fixed (move the visual into the ui layer) — the named
exceptions live in OTHER files (MarkdownPreview) and do not apply here.

- [ ] **Step 2: Full gates**

```bash
rg "@radix-ui|radix-ui" src package.json   # expected: no matches
npm run build && npm test && npm run lint  # expected: all green (ConversationList flake: rerun in isolation if sole failure)
```

- [ ] **Step 3: Runtime check (WidgetGallery)**

Launch the app (`.claude/skills/verify/SKILL.md` has the e2e recipe if a
driven run is wanted; a plain `npm run tauri dev` + eyeballing the widget
gallery view also satisfies this) and confirm: every widget state renders
in the gallery (success/error/interrupted/running), collapsed widgets
expand on click, the diff shows tinted added/removed rows, no visual
regressions that read as bugs (blank cards, missing text). If e2e is used:
`DOCE_E2E_SKIP_WIPE=1 WDIO_SPECS=./specs/tool-call-widgets.spec.ts ./tests/e2e/run-e2e.sh`
after `./tests/e2e/build-for-e2e.sh` — budget for updating that spec's DOM
assertions if it pins old structure.

- [ ] **Step 4: Final commit (if the sweep changed anything)**

```bash
git add -A ':!/.superpowers' && git commit -m "fix(widgets): compliance sweep fixes

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
