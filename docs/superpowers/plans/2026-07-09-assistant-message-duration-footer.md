# Assistant Message Duration Footer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` to execute this plan task-by-task. Implementation must follow `superpowers:test-driven-development`.

**Goal:** Render completed assistant text-message duration beside the assistant output-token count in Workspace transcripts.

**Spec:** `docs/superpowers/specs/2026-07-09-assistant-message-duration-footer-design.md`

## Global Constraints

- Assistant text replies render a muted metadata footer whenever either `durationMs` or `tokenCount` is available.
- Footer combinations:
  - Duration and tokens: `1.2s · ↓ 156 tokens`
  - Duration only: `1.2s`
  - Tokens only: `↓ 156 tokens`
- The footer remains hidden when both values are absent.
- The duration uses persisted `durationMs`; completed messages do not tick.
- Keep `MessageContent` responsible for formatting and rendering assistant message metadata.
- Workspace should enable the duration footer for assistant text messages.
- Do not enable duration metadata for tool widgets, errors, context notices, or user messages.
- User message token meters remain unchanged.
- Tool widgets remain unchanged.
- Do not change backend duration calculation.
- Do not change token-count calculation.
- Do not change database schema or IPC schema.

## Task 1: Render Assistant Duration Metadata In Workspace

**Files:**

- Modify `src/components/MessageContent.tsx`
- Modify `src/components/MessageContent.test.tsx`
- Modify `src/views/workspace/Workspace.tsx`
- Modify `src/views/workspace/Workspace.test.tsx`

**Interfaces:**

- `MessageContent` continues to accept `message` and optional `showTimer`.
- `Timer` remains unchanged.
- Workspace keeps using `MessageContent` for transcript rows.

- [ ] **Step 1: Add failing component tests**

Update `src/components/MessageContent.test.tsx` so assistant text-message metadata covers:

- `durationMs` and `tokenCount` together render `0.5s · ↓ 15.6k tokens`.
- `durationMs` only renders `0.5s`.
- `tokenCount` only renders `↓ 100 tokens`.
- neither value renders no `token-meter`.
- user message token meter behavior is unchanged.
- non-text assistant rows do not gain duration metadata.

Run:

```bash
npx vitest run src/components/MessageContent.test.tsx
```

Expected: FAIL before implementation because duration-only metadata is not rendered without `showTimer`.

- [ ] **Step 2: Add failing Workspace regression**

Update `src/views/workspace/Workspace.test.tsx` with a persisted assistant text reply that has both `durationMs` and `tokenCount`, then assert the transcript footer shows both values.

Run:

```bash
npx vitest run src/views/workspace/Workspace.test.tsx
```

Expected: FAIL before implementation because Workspace renders `MessageContent` without enabling the duration footer.

- [ ] **Step 3: Implement the minimal UI change**

Preferred implementation:

- In `MessageContent`, render the assistant text metadata footer when either `durationMs` or `tokenCount` exists.
- Use `Timer` with `createdAt` and `durationMs` for the duration segment.
- Keep `showTimer` behavior available for callers that intentionally want a live timer.
- Do not add metadata to `tool_result`, `error`, or `context_notice` branches.
- In `Workspace`, pass `showTimer` only for assistant text messages if needed to preserve the existing component boundary.

The implementation may choose either:

- automatic assistant text duration rendering in `MessageContent` when `durationMs` exists, or
- explicit `showTimer` from Workspace for assistant text rows.

It must satisfy the display rules exactly and keep non-text rows unchanged.

- [ ] **Step 4: Run focused tests**

Run:

```bash
npx vitest run src/components/MessageContent.test.tsx src/views/workspace/Workspace.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Run TypeScript**

Run:

```bash
npx tsc -b
```

Expected: PASS.

- [ ] **Step 6: Commit**

Commit only the task-owned code and tests:

```bash
git add src/components/MessageContent.tsx src/components/MessageContent.test.tsx src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat(workspace): show assistant reply duration metadata"
```

## Final Verification

After the task review is clean, run:

```bash
npx tsc -b
npx vitest run src/components/MessageContent.test.tsx src/views/workspace/Workspace.test.tsx
npx vitest run
git status --short
```

The known jsdom `Not implemented: navigation to another Document` warning is acceptable only if the full suite exits 0 and all tests pass.
