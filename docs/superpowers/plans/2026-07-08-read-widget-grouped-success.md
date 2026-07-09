# ReadWidget Grouped Success UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Group successful, truncated, and offloaded Read tool results into one quiet file-reference UI while showing token cost on every new successful read.

**Architecture:** Keep the existing `ReadDetail` data shape and the existing `ViewFullOutput` affordance. The frontend becomes a pure presentation change: `ReadWidget` has one success branch and one failure branch, while `WidgetGallery` stops documenting truncation as a separate state. Backend token-count plumbing is already present through `context::annotate_with_token_count`; implementation verifies those guardrails instead of changing backend contracts.

**Tech Stack:** React 19, TypeScript, Vitest + Testing Library, Tauri/Rust, existing `formatByteCount`, `formatTokenCount`, and `ViewFullOutput`.

## Global Constraints

- Preserve unrelated worktree changes. Do not revert or modify files outside the task file list unless a verification failure proves they are directly required.
- Do not change the backend `ReadDetail` JSON/data model.
- Do not add new IPC commands.
- Do not change how offloaded files are read.
- Do not add file-content previews to `ReadWidget`.
- Do not change `BashWidget` or other tool widgets.
- `outcome.truncated` remains data only; it must not render `Output truncated`, `read-truncated`, a warning badge, or a separate visual state.
- `offloadedTo` remains an affordance only; it must not render an offloaded badge or a separate state header.
- Successful Read metadata order is bytes first, tokens second.
- For older rows with no `tokenCount`, render byte count and omit only the token segment.
- Failure remains the only distinct Read visual state.

---

## File Structure

- Modify `src/views/chat/tool-widgets/ReadWidget.test.tsx`: focused visible-behavior tests for grouped successful reads, offload affordance, token metadata, legacy no-token rows, and failure.
- Modify `src/views/chat/tool-widgets/ReadWidget.tsx`: remove the truncation row; always show byte count for successful reads; show token count when present; keep `ViewFullOutput`.
- Modify `src/views/design-system/WidgetGallery.test.tsx`: guard the Read gallery against reintroducing a separate "Truncated" example.
- Modify `src/views/design-system/WidgetGallery.tsx`: update the Read section description and examples to Standard read / Offloaded read / Failure.
- Verify existing backend token-count guardrails in `src-tauri/src/context/mod.rs` and `src-tauri/src/commands/agent.rs`; no planned backend edits.

---

### Task 1: Group `ReadWidget` Successful States

**Files:**

- Modify: `src/views/chat/tool-widgets/ReadWidget.test.tsx`
- Modify: `src/views/chat/tool-widgets/ReadWidget.tsx`

**Interfaces:**

- Consumes: `ReadDetail` from `src/lib/ipc.ts`, including `outcome`, `offloadedTo?: string | null`, and `tokenCount?: number`.
- Consumes: `formatByteCount(bytes: number): string`.
- Consumes: `formatTokenCount(count: number): string`.
- Consumes: `ViewFullOutput({ path }: { path: string })`.
- Produces: `ReadWidget({ detail }: { detail: ReadDetail })` with one grouped successful-read UI and one failure UI.

- [ ] **Step 1: Write the failing widget tests**

Replace `src/views/chat/tool-widgets/ReadWidget.test.tsx` with:

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import ReadWidget from "./ReadWidget";
import type { ReadDetail } from "@/lib/ipc";

describe("ReadWidget (004-tool-call-widgets, US4)", () => {
  it("renders a compact successful file-reference card with path, bytes, and tokens", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
      tokenCount: 312,
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-widget")).toBeInTheDocument();
    expect(screen.getByTestId("read-widget")).toHaveTextContent(
      "Read /tmp/notes.txt · 11B · 312 tok",
    );
  });

  it("does not present truncation as a separate visible state", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/big.txt",
      offset: null,
      limit: 2000,
      outcome: { ok: true, content: "a lot of content", truncated: true },
      tokenCount: 42,
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.queryByTestId("read-truncated")).not.toBeInTheDocument();
    expect(screen.queryByText("Output truncated")).not.toBeInTheDocument();
    expect(screen.getByTestId("read-widget")).toHaveTextContent("Read /tmp/big.txt · 16B · 42 tok");
  });

  it("renders byte metadata and omits only the token segment for older rows without tokenCount", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/legacy.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-widget")).toHaveTextContent("Read /tmp/legacy.txt · 11B");
    expect(screen.getByTestId("read-widget")).not.toHaveTextContent("tok");
  });

  it("renders a failure state distinctly", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/missing.txt",
      offset: null,
      limit: null,
      outcome: { ok: false, error: "No such file or directory" },
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-widget")).toHaveClass("border-destructive/40");
    expect(screen.getByText(/No such file or directory/)).toBeInTheDocument();
  });

  // --- 010-context-window-management/US3 ---

  it("shows a 'View full output' affordance when the result was offloaded", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/huge.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "preview only...", truncated: true },
      tokenCount: 2048,
      offloadedTo: "/data/tool-outputs/conv1/call1.txt",
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.getByTestId("read-widget")).toHaveTextContent(
      "Read /tmp/huge.txt · 15B · 2.0k tok",
    );
    expect(screen.getByTestId("view-full-output-button")).toBeInTheDocument();
    expect(screen.queryByTestId("read-truncated")).not.toBeInTheDocument();
  });

  it("does not show the full-output affordance when the result was not offloaded", () => {
    const detail: ReadDetail = {
      toolName: "Read",
      filePath: "/tmp/notes.txt",
      offset: null,
      limit: null,
      outcome: { ok: true, content: "hello world", truncated: false },
      tokenCount: 312,
      offloadedTo: null,
    };

    render(<ReadWidget detail={detail} />);

    expect(screen.queryByTestId("view-full-output-button")).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the widget tests and verify they fail for the current UI**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/ReadWidget.test.tsx
```

Expected: FAIL. At least the truncation test should fail because `read-truncated` / `Output truncated` still renders, and the legacy-row test should fail because the old component does not show byte metadata unless `tokenCount` exists.

- [ ] **Step 3: Implement the grouped successful-read UI**

Replace `src/views/chat/tool-widgets/ReadWidget.tsx` with:

```tsx
import type { ReadDetail } from "@/lib/ipc";
import { formatByteCount } from "@/lib/formatByteCount";
import { formatTokenCount } from "@/lib/formatTokenCount";
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

  const byteCount = formatByteCount(detail.outcome.content.length);
  const tokenCount =
    detail.tokenCount != null ? `${formatTokenCount(detail.tokenCount)} tok` : null;

  return (
    <div className="rounded-lg border border-border bg-card p-3 text-sm" data-testid="read-widget">
      <p className="font-mono text-xs text-muted-foreground">
        Read <span>{detail.filePath}</span> · {byteCount}
        {tokenCount != null && <> · {tokenCount}</>}
      </p>
      {detail.offloadedTo && <ViewFullOutput path={detail.offloadedTo} />}
    </div>
  );
}
```

- [ ] **Step 4: Run the widget tests and verify they pass**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/ReadWidget.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit Task 1**

Run:

```bash
git add src/views/chat/tool-widgets/ReadWidget.tsx src/views/chat/tool-widgets/ReadWidget.test.tsx
git commit -m "feat(widgets): group read success states"
```

Expected: one commit containing only `ReadWidget.tsx` and `ReadWidget.test.tsx`.

---

### Task 2: Update `WidgetGallery` Read Examples

**Files:**

- Modify: `src/views/design-system/WidgetGallery.test.tsx`
- Modify: `src/views/design-system/WidgetGallery.tsx`

**Interfaces:**

- Consumes: `ReadWidget` from Task 1.
- Produces: Read gallery examples named `Standard read`, `Offloaded read`, and `Failure`, with no separate `Truncated` example.

- [ ] **Step 1: Write the failing gallery test**

Replace `src/views/design-system/WidgetGallery.test.tsx` with:

```tsx
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import WidgetGallery from "./WidgetGallery";

describe("WidgetGallery", () => {
  it("fills the shell content area instead of forcing viewport height", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(screen.getByTestId("widget-gallery")).toHaveClass("h-full");
    expect(screen.getByTestId("widget-gallery")).not.toHaveClass("h-dvh");
  });

  it("documents Read as grouped successful reads rather than separate truncated state", () => {
    render(<WidgetGallery onClose={vi.fn()} />);

    expect(
      screen.getByText("A minimal file-reference card. Standard / offloaded / failure."),
    ).toBeInTheDocument();
    expect(screen.getByText("Standard read")).toBeInTheDocument();
    expect(screen.getByText("Offloaded read")).toBeInTheDocument();
    expect(screen.queryByText("Truncated")).not.toBeInTheDocument();
    expect(screen.queryByText("Offloaded (large file)")).not.toBeInTheDocument();
    expect(
      screen.queryByText(
        "A file-reference card, not a raw content dump. Success / truncated / offloaded / failure.",
      ),
    ).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the gallery test and verify it fails for the current gallery copy**

Run:

```bash
npx vitest run src/views/design-system/WidgetGallery.test.tsx
```

Expected: FAIL because the Read section still uses the old description and examples.

- [ ] **Step 3: Update the Read section examples**

In `src/views/design-system/WidgetGallery.tsx`, replace the current Read `<Section>` block with:

```tsx
<Section title="Read" description="A minimal file-reference card. Standard / offloaded / failure.">
  <Example label="Standard read">
    <ReadWidget
      detail={{
        toolName: "Read",
        filePath: "src/agent/dispatch.rs",
        offset: null,
        limit: null,
        outcome: { ok: true, content: "pub fn execute(...", truncated: false },
        tokenCount: 312,
      }}
    />
  </Example>
  <Example label="Offloaded read">
    <ReadWidget
      detail={{
        toolName: "Read",
        filePath: "bug_00.txt",
        offset: null,
        limit: null,
        outcome: { ok: true, content: "(truncated preview)", truncated: true },
        tokenCount: 2048,
        offloadedTo: "/tmp/doce/tool-outputs/c1/call-1.txt",
      }}
    />
  </Example>
  <Example label="Failure">
    <ReadWidget
      detail={{
        toolName: "Read",
        filePath: "does/not/exist.txt",
        offset: null,
        limit: null,
        outcome: { ok: false, error: "No such file or directory (os error 2)" },
      }}
    />
  </Example>
</Section>
```

- [ ] **Step 4: Run the gallery test and verify it passes**

Run:

```bash
npx vitest run src/views/design-system/WidgetGallery.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Run both focused frontend tests together**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/ReadWidget.test.tsx src/views/design-system/WidgetGallery.test.tsx
```

Expected: PASS.

- [ ] **Step 6: Commit Task 2**

Run:

```bash
git add src/views/design-system/WidgetGallery.tsx src/views/design-system/WidgetGallery.test.tsx
git commit -m "docs(widgets): group read gallery examples"
```

Expected: one commit containing only `WidgetGallery.tsx` and `WidgetGallery.test.tsx`.

---

### Task 3: Verify Token-Count Backend Guardrails and Frontend Type Safety

**Files:**

- Verify: `src-tauri/src/context/mod.rs`
- Verify: `src-tauri/src/commands/agent.rs`
- Verify: frontend TypeScript project

**Interfaces:**

- Consumes: existing `context::annotate_with_token_count(engine, outcome) -> ToolOutcome`.
- Consumes: existing `wants_token_count("Read") == true` guard.
- Produces: verified implementation with no backend data-model changes.

- [ ] **Step 1: Run TypeScript build**

Run:

```bash
npx tsc -b
```

Expected: PASS with no TypeScript errors.

- [ ] **Step 2: Run focused frontend tests**

Run:

```bash
npx vitest run src/views/chat/tool-widgets/ReadWidget.test.tsx src/views/design-system/WidgetGallery.test.tsx
```

Expected: PASS.

- [ ] **Step 3: Verify the pure Rust token-count guardrails**

Run from the repository root:

```bash
cd src-tauri
cargo test wants_token_count_is_true_only_for_the_four_size_variable_tools
```

Expected: PASS. This confirms `Read` is in the backend set of tools that receive `tokenCount`.

Run:

```bash
cd src-tauri
cargo test merge_token_count_inserts_the_field_into_an_object_detail
```

Expected: PASS. This confirms the backend still inserts `tokenCount` into object-shaped tool details without changing the rest of the detail payload.

- [ ] **Step 4: Verify the model-backed Read token-count path**

Run:

```bash
cd src-tauri
cargo test subagent_backend_tool_result_carries_a_real_token_count_for_read -- --ignored
```

Expected: PASS. This confirms a persisted Read tool result carries a real tokenizer-derived `tokenCount`. If this fails because the local model file is missing, stop and report that exact missing-file error; do not replace this with an estimated frontend token count.

- [ ] **Step 5: Run the full frontend suite**

Run:

```bash
npx vitest run
```

Expected: PASS.

- [ ] **Step 6: Confirm no unintended files are staged**

Run:

```bash
git status --short
```

Expected: the only committed implementation changes are the Task 1 and Task 2 commits. Existing unrelated modified files may still appear in the worktree, but no unrelated files should be staged.

---

## Final Review Checklist

- [ ] `ReadWidget` has no `read-truncated` test id.
- [ ] `ReadWidget` renders bytes for successful reads even when `tokenCount` is absent.
- [ ] `ReadWidget` renders token count when `tokenCount` is present.
- [ ] Offloaded reads still render `view-full-output-button`.
- [ ] Failure still uses destructive styling and renders the backend error string.
- [ ] `WidgetGallery` has no Read example labeled `Truncated`.
- [ ] No backend data shapes or IPC commands changed.
- [ ] Focused frontend tests pass.
- [ ] TypeScript build passes.
- [ ] Backend token-count guardrails pass or a missing local model is reported clearly.
