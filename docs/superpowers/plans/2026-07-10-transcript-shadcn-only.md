# Transcript Shadcn-Only Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate the chat transcript surface (Workspace scroller, turns, message rows, streaming status, plan tracker, topbar contents) to compose stock shadcn components only, deleting `StickyUserMessage`, `UserMessageBubble`, and `ContextUsageGauge`.

**Architecture:** `src/components/ui/` is the shadcn layer and the only place visual identity lives; app transcript files compose those primitives with layout-only wrappers. The custom `use-stick-to-bottom` scroller is replaced by the already-installed `ui/message-scroller.tsx` (wrapping `@shadcn/react`'s headless scroller). The row dispatcher `src/components/MessageContent.tsx` is renamed to `src/views/workspace/TranscriptRow.tsx`.

**Tech Stack:** React 19, Tailwind v4 (CSS-first, `src/styles/theme.css`), shadcn base-nova style on Base UI, `@shadcn/react` headless primitives, Vitest + Testing Library (jsdom), oxlint/oxfmt.

**Spec:** `docs/superpowers/specs/2026-07-10-transcript-shadcn-only-design.md` — read it before starting.

## Global Constraints

- Work on `main`, in place (no worktrees) — this repo's convention.
- In-scope app files may use ONLY layout utilities: `flex`/`grid`/`flex-col`/`items-*`/`justify-*`/`gap-*`, `p-*`/`m-*`/`mx-auto`/`space-*`, `w-*`/`h-*`/`min-w-0`/`max-w-3xl`/`flex-1`/`shrink-0`/`size-*`, `overflow-*`/`relative`/`absolute`/`inset-*`/`z-*`/`truncate`, `@container`/`@5xl:*`. NO color, typography, border, shadow, radius utilities; NO arbitrary values (`text-[13px]`) or arbitrary properties (`[mask-image:...]`); NO new `theme.css` rules.
- Named exceptions (single-location each): (1) `MarkdownPreview`'s `prose prose-sm dark:prose-invert max-w-none`; (2) `tabular-nums` on the streaming chron and `Timer` output; (3) `[view-transition-name:chat-composer]` on the composer shell in `Workspace.tsx` (composer is OUT of scope — do not touch the composer shell block or its `border-t border-border` conditional); (4) Tauri drag-region plumbing in `WorkspaceTopbar` (`pointer-events-none`/`pointer-events-auto`, `data-topbar-no-drag`).
- Frozen contracts: no IPC/backend changes; `groupTranscriptTurns` untouched; tool-widget components untouched; Tiptap untouched; no Radix (`rg "@radix-ui|radix-ui" src package.json` must stay clean).
- Preserve testids used by e2e: `context-usage-gauge` (asserted in `tests/e2e/specs/context-window-management.spec.ts`), `agent-thinking`, `chat-message`, `workspace-error`.
- Tests target behavior and stable structure (roles, `data-slot`, `data-testid`, text) — do NOT port class-name assertions that pinned bespoke styling.
- Format with `npm run format` (oxfmt — NOT prettier) before every commit.
- Commit messages end with: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Full gates (Task 7): `npm run build`, `npm test`, `npm run lint`, `npm run format:check`.

---

### Task 1: Workspace scroller swap (StickToBottom → MessageScroller) + error Alert

**Files:**

- Modify: `src/test/setup.ts` (~line 87, next to the ResizeObserver stub)
- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx`

**Interfaces:**

- Consumes: `MessageScrollerProvider/MessageScroller/MessageScrollerViewport/MessageScrollerContent/MessageScrollerButton` from `@/components/ui/message-scroller`; `useMessageScroller` from `@shadcn/react/message-scroller` (returns `{ scrollToEnd(options?), scrollToMessage, scrollToStart }`); `Alert`, `AlertDescription` from `@/components/ui/alert`.
- Produces: transcript DOM structure later tasks' tests rely on — `data-testid="workspace-scroll-container"` on the Viewport, `data-testid="workspace-transcript-content"` on the Content, `data-testid="scroll-to-bottom"` on the self-managing button. `TranscriptTurn` props are unchanged.

- [ ] **Step 1: Add an IntersectionObserver stub to test setup**

The `@shadcn/react` message-scroller uses IntersectionObserver + ResizeObserver + scroll listeners; jsdom has neither observer. `src/test/setup.ts` already stubs ResizeObserver (~line 87). Add below it:

```ts
// jsdom has no IntersectionObserver either; the @shadcn/react
// message-scroller needs one to exist. Inert stub — autoscroll behavior is
// not testable in jsdom and is covered by browser-level verification.
if (typeof globalThis.IntersectionObserver === "undefined") {
  globalThis.IntersectionObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
}
```

- [ ] **Step 2: Update Workspace.test.tsx — rewrite scroller-coupled tests**

Three areas change (write these first; they should fail until Step 3):

(a) At the top of the file, partial-mock the headless package so the send-re-arm test can observe `scrollToEnd`:

```tsx
const scrollToEndSpy = vi.hoisted(() => vi.fn());
vi.mock("@shadcn/react/message-scroller", async (importOriginal) => {
  const original = await importOriginal<typeof import("@shadcn/react/message-scroller")>();
  return {
    ...original,
    useMessageScroller: () => ({
      scrollToEnd: scrollToEndSpy,
      scrollToMessage: vi.fn(),
      scrollToStart: vi.fn(),
    }),
  };
});
```

Add `scrollToEndSpy.mockClear()` in the file's existing top-level `beforeEach`.

(b) The test at ~line 226 ("keeps the transcript content wrapper sticky-safe and makes the latest turn viewport-height") asserted `min-h-[100cqh]`, `overflow-x-clip`, and `[container-type:size]` — all deleted with the sticky UX. Replace it with:

```tsx
it("renders the transcript inside the shadcn message scroller", async () => {
  vi.mocked(commands.listMessages).mockResolvedValue([]);
  render(<Workspace conversationId="conv-1" />);
  const viewport = await screen.findByTestId("workspace-scroll-container");
  const content = screen.getByTestId("workspace-transcript-content");
  expect(viewport).toHaveAttribute("data-slot", "message-scroller-viewport");
  expect(content).toHaveAttribute("data-slot", "message-scroller-content");
  expect(viewport.closest('[data-slot="message-scroller"]')).not.toBeNull();
});
```

(This file has no shared render helper — tests render `<Workspace conversationId="conv-1" />` inline; the top-level `beforeEach` already mocks all `commands`/`events`.)

Also fix the test at ~line 177 that navigated `getByTestId("workspace-scroll-container").parentElement?.parentElement` — re-anchor it on `screen.getByTestId("workspace-scroll-container").closest('[data-slot="message-scroller"]')`.

Do NOT touch the sticky-anchor test at ~line 184 yet (Task 2 rewrites it; it still passes after this task).

(c) Replace the two scroll-button behavior tests (~lines 1932–2000, "shows the scroll-to-bottom button after scrolling up…" and "scrolls to bottom and hides…") and the now-unused `setScrollMetrics` helper (~line 56) with structural + wiring tests. The primitive owns show/hide behavior (verified in-browser, not jsdom):

```tsx
it("renders the self-managing scroll-to-end button inside the scroller", async () => {
  vi.mocked(commands.listMessages).mockResolvedValue([]);
  render(<Workspace conversationId="conv-1" />);
  const button = await screen.findByTestId("scroll-to-bottom");
  expect(button).toHaveAttribute("data-slot", "message-scroller-button");
});

it("re-arms autoscroll by scrolling to the end when a message is sent", async () => {
  vi.mocked(commands.listMessages).mockResolvedValue([]);
  vi.mocked(commands.sendAgentMessage).mockResolvedValue("ok");
  render(<Workspace conversationId="conv-1" />);
  await screen.findByTestId("agent-input");

  await userEvent.type(screen.getByTestId("agent-input"), "hello");
  await userEvent.click(screen.getByTestId("agent-send"));

  expect(scrollToEndSpy).toHaveBeenCalled();
});
```

(This is the same inline render + `agent-input`/`agent-send` pattern the file's existing send tests use — mirror their `sendAgentMessage` mock return shape if it differs.)

- [ ] **Step 3: Run the updated tests to verify they fail**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx`
Expected: FAIL — new assertions can't find `data-slot="message-scroller-*"` (component still renders StickToBottom).

- [ ] **Step 4: Rewrite Workspace.tsx's scroller block**

Imports — remove `StickToBottom`/`StickToBottomContext` (line 3), `ArrowDown` (line 2), and `Button` (line 6, unused after this change); add:

```tsx
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  MessageScroller,
  MessageScrollerButton,
  MessageScrollerContent,
  MessageScrollerProvider,
  MessageScrollerViewport,
  useMessageScroller,
} from "@/components/ui/message-scroller";
```

(`ui/message-scroller.tsx` re-exports `useMessageScroller` from `@shadcn/react/message-scroller`; the test's `vi.mock` of the package still intercepts it through the re-export.)

Above the `Workspace` component, add the headless bridge (logic-only, no styling — compliant):

```tsx
/**
 * useMessageScroller must be called under MessageScrollerProvider, but the
 * send() callback lives in Workspace (the Provider's renderer). This inert
 * bridge hands the scrollToEnd handle up via a ref.
 */
function ScrollToEndBridge({
  scrollToEndRef,
}: {
  scrollToEndRef: { current: (() => void) | null };
}) {
  const { scrollToEnd } = useMessageScroller();
  useEffect(() => {
    scrollToEndRef.current = () => {
      scrollToEnd({ behavior: "smooth" });
    };
    return () => {
      scrollToEndRef.current = null;
    };
  }, [scrollToEnd, scrollToEndRef]);
  return null;
}
```

Inside `Workspace`: replace `const stickToBottomContextRef = useRef<StickToBottomContext | null>(null);` (line 175) with `const scrollToEndRef = useRef<(() => void) | null>(null);` and in `send()` replace `void stickToBottomContextRef.current?.scrollToBottom();` (line 413, keep the load-bearing comment above it, reworded for the new primitive) with `scrollToEndRef.current?.();`.

Drop the `previousTurns`/`lastTurn` derivation (lines 334–335) — a single map renders all turns. Replace the entire `<StickToBottom …>…</StickToBottom>` block (lines 466–544, including both stale comments about keeping use-stick-to-bottom) with:

```tsx
<MessageScrollerProvider key={conversationId} autoScroll defaultScrollPosition="end">
  <ScrollToEndBridge scrollToEndRef={scrollToEndRef} />
  <MessageScroller className="h-auto min-h-0 flex-1 @container">
    <MessageScrollerViewport className="p-4" data-testid="workspace-scroll-container">
      <MessageScrollerContent data-testid="workspace-transcript-content">
        <div className="mx-auto w-full max-w-3xl">
          {transcriptTurns.map((turn, index) => {
            const isLastTurn = index === transcriptTurns.length - 1;
            return (
              <TranscriptTurn
                key={turn.id}
                turn={turn}
                isLastTurn={isLastTurn}
                pendingWidget={isLastTurn ? pendingTurnWidget : null}
                error={isLastTurn ? error : null}
              />
            );
          })}
          {transcriptTurns.length === 0 && error && (
            <Alert variant="destructive" className="mb-6" data-testid="workspace-error">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
        </div>
      </MessageScrollerContent>
    </MessageScrollerViewport>
    <MessageScrollerButton data-testid="scroll-to-bottom" />
    <PlanTracker conversationId={conversationId} />
  </MessageScroller>
</MessageScrollerProvider>
```

Notes: `key` on the Provider resets scroll state per conversation (old `key={conversationId}` behavior). `@container` (standard inline-size container) replaces `[container-type:size]` for PlanTracker's `@5xl:` queries. `h-auto` neutralizes the Root's default `size-full` height inside the flex column. The root `<div className="flex h-full flex-col bg-background text-foreground">` (line 464): drop `bg-background text-foreground` (color utilities). First `grep -n "chat-surface\|bg-background" src/App.tsx` — if the main-pane container App.tsx renders around Workspace does not already paint `bg-background`, add `bg-background` to THAT App.tsx container (App.tsx is outside the strict scope) so the window background doesn't show through.

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run src/views/workspace/Workspace.test.tsx`
Expected: PASS (all — including untouched send/composer/pending-widget suites). If the scroller primitive throws in jsdom, fix the Step 1 stub first, not the component.

- [ ] **Step 6: Typecheck, format, commit**

```bash
npx tsc -b && npm run format
git add -A && git commit -m "refactor(workspace): swap StickToBottom for shadcn MessageScroller

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: TranscriptTurn — drop sticky UX, MessageGroup wrapper, error Alert

**Files:**

- Modify: `src/views/workspace/TranscriptTurn.tsx`
- Delete: `src/views/workspace/StickyUserMessage.tsx`, `src/views/workspace/StickyUserMessage.test.tsx`
- Modify: `src/views/workspace/TranscriptTurn.test.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx` (sticky-anchor test ~line 184)

**Interfaces:**

- Consumes: `MessageGroup` from `@/components/ui/message`; `Alert`/`AlertDescription` from `@/components/ui/alert`; the existing default export of `@/components/MessageContent` (user branch renders the user bubble — Task 3 restyles its internals; testid `user-message-bubble` appears after Task 3, so do NOT assert it here yet).
- Produces: `TranscriptTurn` props unchanged (`turn`, `isLastTurn`, `pendingWidget`, `error`). Turn DOM: `data-testid="transcript-turn"` on a `MessageGroup` (`data-slot="message-group"`), user row rendered via `MessageContent`, `data-testid="transcript-turn-body"` intact, error as `Alert` with `data-testid="workspace-error"`.

- [ ] **Step 1: Rewrite TranscriptTurn.test.tsx**

Replace the two sticky tests ("renders a sticky user header above assistant rows", "re-anchors the owning turn when the sticky user bubble is focused") and update the other assertions:

```tsx
it("renders the user message as a chat row above assistant rows", () => {
  render(<TranscriptTurn turn={turn({})} />);

  const transcriptTurn = screen.getByTestId("transcript-turn");
  const body = screen.getByTestId("transcript-turn-body");

  expect(transcriptTurn).toHaveAttribute("data-slot", "message-group");
  expect(screen.queryByTestId("sticky-user-background")).not.toBeInTheDocument();
  expect(document.querySelector('[data-sticky-user-message="true"]')).toBeNull();
  expect(within(transcriptTurn).getByText("run the tests")).toBeInTheDocument();
  expect(within(body).getByText("done")).toBeInTheDocument();
  // The user row precedes the body in DOM order.
  const userRow = within(transcriptTurn).getByRole("group", { name: "You said" });
  expect(userRow.compareDocumentPosition(body) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

it("renders assistant-only turns without a user row", () => {
  render(
    <TranscriptTurn
      turn={turn({
        id: "a0",
        user: null,
        rows: [message({ id: "a0", role: "assistant", content: "welcome" })],
      })}
    />,
  );

  expect(screen.getByTestId("transcript-turn")).toHaveTextContent("welcome");
  expect(screen.queryByRole("group", { name: "You said" })).not.toBeInTheDocument();
});

it("renders error content as a destructive alert inside the turn", () => {
  render(<TranscriptTurn turn={turn({})} error="send failed" />);

  const alert = screen.getByTestId("workspace-error");
  expect(alert).toHaveAttribute("data-slot", "alert");
  expect(alert).toHaveTextContent("send failed");
});
```

Keep the pending-widget test and the data-attribute test, but in the latter drop the `sticky-user-background` expectations and keep `data-chat-turn`/`transcript-turn-body` assertions. Remove the now-unused `fireEvent`/`vi` imports if nothing else uses them.

In `Workspace.test.tsx`, rewrite the test at ~line 184 ("renders user messages as sticky turn anchors that own following assistant rows"): keep its two-turn arrangement assertions (each user message renders before its turn's assistant rows) but replace the `document.querySelectorAll('[data-sticky-user-message="true"]')).toHaveLength(2)` line with `expect(screen.getAllByRole("group", { name: "You said" })).toHaveLength(2)` and drop any `sticky-user-background` queries.

- [ ] **Step 2: Run to verify failures**

Run: `npx vitest run src/views/workspace/TranscriptTurn.test.tsx src/views/workspace/Workspace.test.tsx`
Expected: FAIL (sticky chrome still renders; no `data-slot="message-group"`).

- [ ] **Step 3: Rewrite TranscriptTurn.tsx**

Full new file content:

```tsx
import type * as React from "react";
import MessageContent from "@/components/MessageContent";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { MessageGroup } from "@/components/ui/message";
import BashWidget from "@/views/chat/tool-widgets/BashWidget";
import TaskWidget from "@/views/chat/tool-widgets/TaskWidget";
import type { BashDetail, TaskDetail } from "@/lib/ipc";
import type { TranscriptTurn as TranscriptTurnModel } from "./transcriptTurns";

export type PendingTurnWidget =
  | { kind: "bash"; detail: BashDetail }
  | { kind: "task"; detail: TaskDetail };

export interface TranscriptTurnProps {
  turn: TranscriptTurnModel;
  isLastTurn?: boolean;
  pendingWidget?: PendingTurnWidget | null;
  error?: string | null;
}

export default function TranscriptTurn({
  turn,
  isLastTurn = false,
  pendingWidget = null,
  error = null,
}: TranscriptTurnProps): React.JSX.Element {
  return (
    <MessageGroup
      className="pb-2"
      data-chat-turn="true"
      data-testid="transcript-turn"
      data-last-turn={isLastTurn ? "true" : "false"}
    >
      {turn.user && <MessageContent message={turn.user} />}
      <div data-testid="transcript-turn-body" className="min-w-0">
        {turn.rows.map((message) => (
          <MessageContent key={message.id} message={message} />
        ))}
        {pendingWidget && (
          <div className="mb-6" data-testid="chat-message" role="group" aria-label="doce replied">
            {pendingWidget.kind === "bash" ? (
              <BashWidget detail={pendingWidget.detail} />
            ) : (
              <TaskWidget detail={pendingWidget.detail} />
            )}
          </div>
        )}
        {error && (
          <Alert variant="destructive" className="mb-6" data-testid="workspace-error">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}
      </div>
    </MessageGroup>
  );
}
```

(The `useRef`/`scrollToTurn` machinery existed only for the sticky re-anchor — gone with it.)

- [ ] **Step 4: Delete the sticky component and its test**

```bash
git rm src/views/workspace/StickyUserMessage.tsx src/views/workspace/StickyUserMessage.test.tsx
```

Then verify nothing else imports it: `grep -rn "StickyUserMessage" src/ tests/` — expected: no matches (if the WidgetGallery or another file matches, remove that usage too).

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run src/views/workspace/TranscriptTurn.test.tsx src/views/workspace/Workspace.test.tsx`
Expected: PASS.

- [ ] **Step 6: Typecheck, format, commit**

```bash
npx tsc -b && npm run format
git add -A && git commit -m "refactor(workspace): drop sticky user message, compose turns from MessageGroup

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Rename dispatcher to TranscriptRow; recompose rows; delete UserMessageBubble

**Files:**

- Move: `src/components/MessageContent.tsx` → `src/views/workspace/TranscriptRow.tsx`
- Move: `src/components/MessageContent.test.tsx` → `src/views/workspace/TranscriptRow.test.tsx`
- Delete: `src/components/UserMessageBubble.tsx`, `src/components/UserMessageBubble.test.tsx`
- Modify: `src/views/workspace/TranscriptTurn.tsx` (import path), any other importer found by grep

**Interfaces:**

- Consumes: `Bubble`/`BubbleContent` (`variant="user"` carries the doce-cream look in the ui layer), `Marker`/`MarkerContent`, `Message as ChatMessage`/`MessageContent as ChatMessageContent`/`MessageFooter` from `@/components/ui/message`, `Alert`/`AlertDescription`, `MarkdownPreview`, `UserMessageContent`, `Timer`, `formatTokenCount` — all existing.
- Produces: `export default function TranscriptRow({ message, showTimer }: TranscriptRowProps)` — same props as today's `MessageContent` (`message: Message; showTimer?: boolean`). Row DOM contracts: user rows `role="group" aria-label="You said"` with `data-testid="user-message-bubble"` on the BubbleContent and `data-testid="token-meter"` footer; assistant metadata in a `MessageFooter` with `data-testid="token-meter"`; errors as `Alert` `data-testid="error-message"`; context notices as plain `Marker` keeping `data-testid="context-notice"`, `data-notice-kind`, `role="status"`.

- [ ] **Step 1: Find every importer**

Run: `grep -rln '@/components/MessageContent\|@/components/UserMessageBubble' src/`
Expected importers: `src/views/workspace/TranscriptTurn.tsx`, the test file being moved, and possibly `src/views/design-system/WidgetGallery.tsx`. Every hit gets its import updated in Step 4.

- [ ] **Step 2: Move and update the test file**

```bash
git mv src/components/MessageContent.tsx src/views/workspace/TranscriptRow.tsx
git mv src/components/MessageContent.test.tsx src/views/workspace/TranscriptRow.test.tsx
git rm src/components/UserMessageBubble.tsx src/components/UserMessageBubble.test.tsx
```

In `TranscriptRow.test.tsx`: import `TranscriptRow from "./TranscriptRow"` and rename the `describe` and all `<MessageContent …>` usages to `TranscriptRow`. Then update the behavior assertions that pinned old chrome:

- "renders an error message distinctly" (~line 196): replace the marker assertions with

```tsx
expect(screen.getByTestId("error-message")).toHaveAttribute("data-slot", "alert");
expect(screen.getByTestId("error-message")).toHaveTextContent(/model exploded/);
```

(keep that test's existing fixture text — adjust the regex to it).

- Context-notice tests (~lines 271–342): both kinds now render an unstyled `Marker`. Keep `data-testid="context-notice"`, `data-notice-kind`, `role="status"`, and text assertions; DELETE any `toHaveClass` assertions on tier styling (`bg-muted`, `text-xs`, `text-muted-foreground/70`, borders).
- User-row tests: keep `user-message-bubble` and `token-meter` testids and text assertions; delete any `toHaveClass` on cream/width classes if present.
- UserMessageBubble.test.tsx is deleted; its two unique behaviors (rich_text rendering through `UserMessageContent`, no meter when `tokenCount` null) must exist in `TranscriptRow.test.tsx` — the "keeps the user token meter wired…" and "shows no token meter…" tests already cover the meter; add one rich_text test if the moved file lacks one:

```tsx
it("renders rich_text user content through UserMessageContent", () => {
  render(
    <TranscriptRow
      message={message({
        id: "u9",
        role: "user",
        contentType: "rich_text",
        content: JSON.stringify({ segments: [{ kind: "text", text: "rich hello" }] }),
      })}
    />,
  );
  expect(screen.getByTestId("user-message-bubble")).toHaveTextContent("rich hello");
});
```

(Match the `RichMessageContent` fixture shape already used in the deleted `UserMessageBubble.test.tsx` — copy its fixture rather than inventing one.)

- [ ] **Step 3: Run to verify failures**

Run: `npx vitest run src/views/workspace/TranscriptRow.test.tsx`
Expected: FAIL (component file still exports `MessageContent` with old chrome; imports broken).

- [ ] **Step 4: Rewrite TranscriptRow.tsx**

In the moved file: rename the component and props interface (`MessageContentProps` → `TranscriptRowProps`, `export default function TranscriptRow`), add `MessageFooter` to the ui/message import, add `Alert, AlertDescription`, add `UserMessageContent` and `formatTokenCount` imports, remove the `UserMessageBubble` import. The `ToolWidget` dispatcher function and the plan-row/tool_call/tool_result branches stay byte-identical. Replace these branches:

User branch (replaces lines 48–62):

```tsx
if (m.role === "user") {
  return (
    <ChatMessage
      align="end"
      className="mb-5"
      data-testid="chat-message"
      role="group"
      aria-label="You said"
    >
      <ChatMessageContent>
        <Bubble align="end" variant="user">
          <BubbleContent data-testid="user-message-bubble">
            {m.contentType === "rich_text" ? (
              <UserMessageContent content={m.content} />
            ) : (
              <MarkdownPreview>{m.content}</MarkdownPreview>
            )}
          </BubbleContent>
        </Bubble>
        {m.tokenCount != null && (
          <MessageFooter data-testid="token-meter">
            ↑ {formatTokenCount(m.tokenCount)} tokens
          </MessageFooter>
        )}
      </ChatMessageContent>
    </ChatMessage>
  );
}
```

(Rationale: `Bubble variant="user"` already carries the cream/border look in the ui layer; the old `bubbleClasses` override and the `prose` wrapper around rich text are dropped — BubbleContent's own `text-sm leading-relaxed` styles rich-text segments.)

Error branch (replaces the destructive-restyled Marker, lines 97–110):

```tsx
if (m.contentType === "error") {
  return (
    <ChatMessage className="mb-5" data-testid="chat-message" role="group" aria-label="doce replied">
      <ChatMessageContent>
        <Alert variant="destructive" data-testid="error-message">
          <AlertDescription>{m.content}</AlertDescription>
        </Alert>
      </ChatMessageContent>
    </ChatMessage>
  );
}
```

Context-notice branch (replaces lines 117–138) — one plain Marker for both tiers, kind preserved as data:

```tsx
if (m.contentType === "context_notice") {
  const detail = parseContextNoticeDetail(m.content);
  return (
    <ChatMessage className="mb-5" role="group" aria-label="doce replied">
      <ChatMessageContent>
        <Marker data-testid="context-notice" data-notice-kind={detail.kind} role="status">
          <MarkerContent>{detail.notice}</MarkerContent>
        </Marker>
      </ChatMessageContent>
    </ChatMessage>
  );
}
```

Assistant tail (replaces lines 144–170) — drop the `max-w-none` stacking (`variant="ghost"` already yields full width) and move metadata into `MessageFooter`:

```tsx
return (
  <ChatMessage className="mb-5" data-testid="chat-message" role="group" aria-label="doce replied">
    <ChatMessageContent>
      <Bubble variant="ghost">
        <BubbleContent>
          <MarkdownPreview>{m.content}</MarkdownPreview>
        </BubbleContent>
      </Bubble>
      {showAssistantMetadata && (
        <MessageFooter data-testid="token-meter">
          {showAssistantDuration && <Timer createdAt={m.createdAt} durationMs={m.durationMs} />}
          {showAssistantDuration && m.tokenCount != null && " · "}
          {m.tokenCount != null && `↓ ${formatTokenCount(m.tokenCount)} tokens`}
        </MessageFooter>
      )}
    </ChatMessageContent>
  </ChatMessage>
);
```

(Keep the existing explanatory comments — the FR-013 header comment, plan-row comment, tool_call pairing comment, FR-011 ToolWidget comment — updating the component name where they mention it.)

Update importers found in Step 1: in `TranscriptTurn.tsx`, `import TranscriptRow from "@/views/workspace/TranscriptRow"` and use `<TranscriptRow …>` for both the user row and body rows. Same for `WidgetGallery.tsx` if it imports the old path.

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run src/views/workspace/TranscriptRow.test.tsx src/views/workspace/TranscriptTurn.test.tsx src/views/workspace/Workspace.test.tsx`
Expected: PASS. Also run `grep -rn "components/MessageContent\|UserMessageBubble" src/` — expected: no matches.

- [ ] **Step 6: Typecheck, format, commit**

```bash
npx tsc -b && npm run format
git add -A && git commit -m "refactor(workspace): rename row dispatcher to TranscriptRow on stock primitives

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: StreamingStatus on Marker + Spinner

**Files:**

- Modify: `src/views/workspace/StreamingStatus.tsx`
- Modify: `src/views/workspace/StreamingStatus.test.tsx`

**Interfaces:**

- Consumes: `Marker`, `MarkerIcon`, `MarkerContent` from `@/components/ui/marker` (MarkerIcon is `aria-hidden` by itself); `Spinner` from `@/components/ui/spinner` (defaults `role="status"` — MUST be overridden here so the row's own `role="status"` stays the only live region).
- Produces: unchanged props (`startedAt: number | null`). DOM contract: `data-testid="agent-thinking"` wrapper, `role="status"` element labeled "Working", `data-testid="agent-thinking-spinner"` decorative icon, `data-testid="agent-thinking-timer"` with `aria-live="off"` and `tabular-nums` (exception 2).

- [ ] **Step 1: Update the tests**

Replace the dot assertions and drop fixed-width classes. New test bodies (keep the fake-timer scaffolding and the fallback-startedAt test unchanged apart from these assertions):

In "renders a quiet accessible working status with decorative animation":

```tsx
const status = screen.getByRole("status", { name: "Working" });
const timer = screen.getByTestId("agent-thinking-timer");

expect(status).toBeInTheDocument();
expect(status).toHaveTextContent("Working");
expect(status).not.toContainElement(timer);
expect(screen.getByTestId("agent-thinking")).toHaveTextContent("Working");
const spinner = screen.getByTestId("agent-thinking-spinner");
expect(spinner).toHaveAttribute("aria-hidden", "true");
expect(timer).toHaveTextContent("1.3s");
expect(timer).toHaveAttribute("aria-live", "off");
expect(timer).toHaveClass("tabular-nums");
```

Rename the third test to "ticks across the 0.9s -> 1.0s boundary" and replace its `w-[7ch]`/`shrink-0` class assertions with `toHaveClass("tabular-nums")` on both sides of the tick.

ALSO in `src/views/workspace/Workspace.test.tsx`: the send test at ~line 454 asserts `expect(status).toHaveClass("border-b")` on `agent-thinking` — delete that one line (the bar chrome is gone; the neighboring `composerShell` border assertions stay, the composer is out of scope).

- [ ] **Step 2: Run to verify failures**

Run: `npx vitest run src/views/workspace/StreamingStatus.test.tsx`
Expected: FAIL (`agent-thinking-spinner` not found).

- [ ] **Step 3: Rewrite the render**

Keep the timer logic (lines 1–19) untouched; replace the returned JSX:

```tsx
return (
  <div className="px-4" data-testid="agent-thinking">
    <div className="mx-auto max-w-3xl py-2">
      <Marker>
        <MarkerIcon data-testid="agent-thinking-spinner">
          <Spinner role="presentation" aria-label={undefined} />
        </MarkerIcon>
        <MarkerContent>
          <span role="status" aria-atomic="true" aria-label="Working">
            Working
          </span>
        </MarkerContent>
        <span
          aria-live="off"
          className="ml-auto shrink-0 tabular-nums"
          data-testid="agent-thinking-timer"
        >
          {formatElapsedMs(now - effectiveStartedAt)}
        </span>
      </Marker>
    </div>
  </div>
);
```

with imports:

```tsx
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import { Spinner } from "@/components/ui/spinner";
```

(The old `border-b`, fixed `h-8`, `text-xs text-muted-foreground`, hand-rolled dots, `w-[7ch]`, and `font-mono` are gone; Marker supplies muted text styling. `role="presentation"`/`aria-label={undefined}` neutralize Spinner's own `role="status"` default — its props spread wins over its defaults.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/views/workspace/StreamingStatus.test.tsx src/views/workspace/Workspace.test.tsx`
Expected: PASS (Workspace suite included — it asserts StreamingStatus placement/suppression).

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npm run format
git add -A && git commit -m "refactor(workspace): compose StreamingStatus from Marker and Spinner

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: PlanTracker on Card/Item/Badge/Progress; drop fade-out

**Files:**

- Modify: `src/views/workspace/PlanTracker.tsx`
- Modify: `src/views/workspace/PlanTracker.test.tsx`

**Interfaces:**

- Consumes: `Card`/`CardHeader`/`CardTitle`/`CardContent` from `@/components/ui/card`; `Item`/`ItemGroup`/`ItemMedia`/`ItemContent`/`ItemTitle` from `@/components/ui/item`; `Badge` from `@/components/ui/badge`; `Progress` from `@/components/ui/progress`; `Spinner`; `Button` from `@/components/ui/button`; `Check`, `Circle` from `lucide-react`.
- Produces: unchanged props (`conversationId: string`). DOM contract for tests: `plan-tracker`/`plan-card`/`plan-rail`/`plan-collapse`/`plan-step`/`plan-dot`/`plan-chip`/`plan-done-collapsed`/`plan-more` testids survive; step state exposed as `data-state="done" | "current" | "todo"` on `plan-step` items and `plan-dot` badges (replacing class-based assertions); `data-current` attribute kept.

- [ ] **Step 1: Update the tests**

- Step-state assertions (~lines 63–75): replace `toHaveClass("line-through")`, `toHaveClass("text-emerald-600")`, `toHaveClass("text-muted-foreground")` with `data-state` checks:

```tsx
expect(steps[0]).toHaveAttribute("data-state", "done");
expect(steps[1]).toHaveAttribute("data-state", "current");
expect(steps[2]).toHaveAttribute("data-state", "todo");
```

(keep the `data-current` assertion on the current step and the text-content assertions).

- Fade test (~line 102) becomes immediate: rename to "unmounts when the turn ends (plan: null)" and assert `plan-tracker` is removed right after the null event (no `opacity-0` intermediate, no timer advance):

```tsx
await waitFor(() => expect(screen.queryByTestId("plan-tracker")).not.toBeInTheDocument());
```

- Rail test (~line 192): replace any dot class assertions (`bg-emerald-600`, `border-amber-500`) with `data-state` checks on `plan-dot`, and keep the chip-past-12 behavior assertions.

- [ ] **Step 2: Run to verify failures**

Run: `npx vitest run src/views/workspace/PlanTracker.test.tsx`
Expected: FAIL.

- [ ] **Step 3: Rewrite PlanTracker.tsx**

Keep: the constants (drop `FADE_OUT_MS`), props, the whole subscription effect EXCEPT the fade branch, the caps/filter logic in `PlanCard`, and the `expanded` state machine. In `applyUpdate`, the null branch becomes:

```tsx
// Turn ended: unmount immediately.
setExpanded(false);
setPlan(null);
```

Delete `leaving` state, `leaveTimerRef`, and their cleanup lines. The overlay wrapper loses the fade classes:

```tsx
<div className="absolute top-3 right-3 z-10" data-testid="plan-tracker">
```

The collapse control becomes a ghost Button:

```tsx
{
  expanded && (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      className="mt-1 w-full @5xl:hidden"
      onClick={() => setExpanded(false)}
      aria-label="Hide plan"
      data-testid="plan-collapse"
    >
      collapse
    </Button>
  );
}
```

New `PlanCard` (helper `stepState` shared with the rail):

```tsx
function stepState(step: PlanSnapshot["steps"][number], index: number, currentStepIndex: number) {
  if (step.done) return "done";
  return index === currentStepIndex ? "current" : "todo";
}

function PlanCard({ plan, doneCount }: { plan: PlanSnapshot; doneCount: number }) {
  const collapseDone = plan.steps.length > CARD_COLLAPSE_THRESHOLD;
  const rows = plan.steps
    .map((step, index) => ({ step, index }))
    .filter(({ step, index }) => {
      if (!collapseDone) return true;
      return !step.done || index === plan.currentStepIndex;
    });
  const pendingVisible = collapseDone ? rows.slice(0, CARD_MAX_PENDING + 1) : rows;
  const hiddenCount = rows.length - pendingVisible.length;

  return (
    <Card className="w-64 gap-2 py-3">
      <CardHeader className="gap-1 px-3">
        <CardTitle className="truncate" title={plan.goal}>
          {plan.goal}
        </CardTitle>
        <Badge variant="secondary">
          {doneCount}/{plan.steps.length}
        </Badge>
        <Progress value={plan.steps.length > 0 ? (doneCount / plan.steps.length) * 100 : 0} />
      </CardHeader>
      <CardContent className="px-3">
        {collapseDone && doneCount > 0 && (
          <div data-testid="plan-done-collapsed">✓ {doneCount} done</div>
        )}
        <ItemGroup>
          {pendingVisible.map(({ step, index }) => (
            <Item
              key={index}
              size="xs"
              data-state={stepState(step, index, plan.currentStepIndex)}
              data-current={index === plan.currentStepIndex ? "true" : undefined}
              data-testid="plan-step"
            >
              <ItemMedia variant="icon">
                {step.done ? (
                  <Check />
                ) : index === plan.currentStepIndex ? (
                  <Spinner role="presentation" aria-label={undefined} />
                ) : (
                  <Circle />
                )}
              </ItemMedia>
              <ItemContent>
                <ItemTitle className="truncate" title={step.description}>
                  {step.description}
                </ItemTitle>
              </ItemContent>
            </Item>
          ))}
        </ItemGroup>
        {hiddenCount > 0 && <div data-testid="plan-more">+{hiddenCount} more</div>}
      </CardContent>
    </Card>
  );
}
```

New `PlanRail`:

```tsx
function PlanRail({ plan, doneCount }: { plan: PlanSnapshot; doneCount: number }) {
  if (plan.steps.length > RAIL_MAX_DOTS) {
    return (
      <Badge variant="secondary" data-testid="plan-chip">
        {doneCount}/{plan.steps.length}
      </Badge>
    );
  }
  return (
    <span className="flex flex-col items-center gap-1">
      {plan.steps.map((step, index) => (
        <Badge
          key={index}
          variant={
            step.done ? "default" : index === plan.currentStepIndex ? "secondary" : "outline"
          }
          className="size-5 justify-center p-0"
          data-state={stepState(step, index, plan.currentStepIndex)}
          data-current={index === plan.currentStepIndex ? "true" : undefined}
          data-testid="plan-dot"
        >
          {step.done ? <Check /> : index + 1}
        </Badge>
      ))}
    </span>
  );
}
```

Imports for the file:

```tsx
import { useEffect, useState } from "react";
import { Check, Circle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Item, ItemContent, ItemGroup, ItemMedia, ItemTitle } from "@/components/ui/item";
import { Progress } from "@/components/ui/progress";
import { Spinner } from "@/components/ui/spinner";
import { commands, events, type PlanSnapshot } from "@/lib/ipc";
import { cn } from "@/lib/cn";
```

(`cn` stays only for the card/rail visibility toggles `cn("hidden @5xl:block", expanded && "block")` etc. — pure layout. The old glassy pill recipe, `text-[10px]`, `h-4.5 w-4.5`, unicode-styled spans, emerald/amber classes are all gone. If `cn` ends up unused, remove the import.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/views/workspace/PlanTracker.test.tsx`
Expected: PASS.

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npm run format
git add -A && git commit -m "refactor(workspace): compose PlanTracker from Card, Item, Badge, and Progress

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: WorkspaceTopbar on Item; gauge → Progress + Tooltip; delete ContextUsageGauge

**Files:**

- Modify: `src/views/workspace/WorkspaceTopbar.tsx`
- Delete: `src/components/ContextUsageGauge.tsx`, `src/components/ContextUsageGauge.test.tsx`
- Modify: `src/views/workspace/WorkspaceTopbar.test.tsx`

**Interfaces:**

- Consumes: `Item`/`ItemContent`/`ItemTitle`/`ItemDescription` from `@/components/ui/item`; `Progress` from `@/components/ui/progress`; `Tooltip`/`TooltipTrigger`/`TooltipContent` from `@/components/ui/tooltip`; `useContextUsageStore` and `commands.getContextUsage` (moved in from the deleted gauge).
- Produces: unchanged props. DOM contract: `workspace-topbar`, `workspace-topbar-title`, `workspace-topbar-path` testids; `data-testid="context-usage-gauge"` with `role="status"` and the percentage `aria-label` (REQUIRED by `tests/e2e/specs/context-window-management.spec.ts`), inside the `data-topbar-no-drag` wrapper.

- [ ] **Step 1: Update WorkspaceTopbar.test.tsx**

The five existing tests keep working against the same testids/aria-labels — only one addition is needed: fold in the deleted gauge test's null-state behavior. Add:

```tsx
it("renders no usage indicator before context usage resolves", async () => {
  vi.mocked(commands.getContextUsage).mockRejectedValue(new Error("no model yet"));

  renderTopbar(conversationFixture());

  await screen.findByTestId("workspace-topbar");
  expect(screen.queryByTestId("context-usage-gauge")).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run to verify current state**

Run: `npx vitest run src/views/workspace/WorkspaceTopbar.test.tsx`
Expected: the new test PASSES already (old gauge also returned null) — that's fine; the deletion below is what this suite guards. The suite must still pass after Step 3.

- [ ] **Step 3: Rewrite WorkspaceTopbar.tsx and delete the gauge**

Replace the title/path divs and the gauge usage. Full new file:

```tsx
import { useEffect, useMemo, useState } from "react";
import { homeDir } from "@tauri-apps/api/path";
import { TopbarPortal } from "@/components/Topbar";
import { Item, ItemContent, ItemDescription, ItemTitle } from "@/components/ui/item";
import { Progress } from "@/components/ui/progress";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { commands, type Conversation, type Workspace } from "@/lib/ipc";
import { useContextUsageStore } from "@/state/contextUsageStore";
import { getConversationWorkspaceLabel } from "@/views/chat/sidebarConversationRow";

interface WorkspaceTopbarProps {
  conversation: Conversation;
}

/**
 * 010-context-window-management (UI refactor): a small usage indicator in
 * the topbar. Display-only — hovering shows the exact percentage in a
 * tooltip; compaction is triggered by typing `/compact` in the composer.
 */
function ContextUsageIndicator({ conversationId }: { conversationId: string }) {
  const usage = useContextUsageStore((s) => s.usage[conversationId]);
  const setUsage = useContextUsageStore((s) => s.setUsage);

  useEffect(() => {
    let cancelled = false;
    commands
      .getContextUsage(conversationId)
      .then((u) => {
        if (!cancelled) setUsage(u);
      })
      .catch(() => {
        // No model loaded yet, or nothing to report — leave the indicator
        // unrendered rather than surfacing an error for a background
        // enrichment call.
      });
    return () => {
      cancelled = true;
    };
  }, [conversationId, setUsage]);

  if (!usage) return null;

  const pct = usage.tokenBudget > 0 ? (usage.tokensUsed / usage.tokenBudget) * 100 : 0;
  const clampedPct = Math.min(100, Math.max(0, pct));
  const tooltipText =
    usage.state === "justCompacted"
      ? `${Math.round(pct)}% of context used · just compacted`
      : `${Math.round(pct)}% of context used`;

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <div
            className="flex h-8 w-16 items-center"
            data-testid="context-usage-gauge"
            role="status"
            aria-label={tooltipText}
          />
        }
      >
        <Progress value={clampedPct} />
      </TooltipTrigger>
      <TooltipContent data-testid="context-usage-tooltip">{tooltipText}</TooltipContent>
    </Tooltip>
  );
}

export default function WorkspaceTopbar({ conversation }: WorkspaceTopbarProps) {
  const [homePath, setHomePath] = useState<string | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);

  useEffect(() => {
    let cancelled = false;

    homeDir()
      .then((path) => {
        if (!cancelled) setHomePath(path);
      })
      .catch(() => {
        if (!cancelled) setHomePath("");
      });

    commands
      .listWorkspaces()
      .then((loadedWorkspaces) => {
        if (!cancelled) setWorkspaces(loadedWorkspaces);
      })
      .catch(console.error);

    return () => {
      cancelled = true;
    };
  }, []);

  const workspacesById = useMemo(
    () => new Map(workspaces.map((workspace) => [workspace.id, workspace])),
    [workspaces],
  );
  const workspaceLabel = getConversationWorkspaceLabel(
    conversation.workspaceId,
    workspacesById,
    homePath,
  );

  return (
    <TopbarPortal target="main">
      <div
        className="pointer-events-none flex min-w-0 flex-1 items-center justify-between gap-3"
        data-testid="workspace-topbar"
      >
        <Item size="xs" className="w-auto min-w-0 p-0">
          <ItemContent>
            <ItemTitle className="truncate" data-testid="workspace-topbar-title">
              {conversation.title}
            </ItemTitle>
            <ItemDescription className="truncate" data-testid="workspace-topbar-path">
              {workspaceLabel}
            </ItemDescription>
          </ItemContent>
        </Item>
        <div className="pointer-events-auto" data-topbar-no-drag>
          <ContextUsageIndicator conversationId={conversation.id} />
        </div>
      </div>
    </TopbarPortal>
  );
}
```

Then:

```bash
git rm src/components/ContextUsageGauge.tsx src/components/ContextUsageGauge.test.tsx
grep -rn "ContextUsageGauge" src/ tests/
```

Expected grep: no matches (fix any straggler, e.g. WidgetGallery).

Note: if Base UI's Tooltip requires a `TooltipProvider` ancestor at runtime and the trigger errors in tests, wrap the returned `<Tooltip>` in `<TooltipProvider>…</TooltipProvider>` from the same ui file — both are stock.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run src/views/workspace/WorkspaceTopbar.test.tsx`
Expected: PASS — including the pre-existing aria-label "25%" test and pointer-events tests.

- [ ] **Step 5: Typecheck, format, commit**

```bash
npx tsc -b && npm run format
git add -A && git commit -m "refactor(workspace): topbar Item stack and Progress-based context usage

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: Dependency removal, compliance sweep, full gates

**Files:**

- Modify: `package.json`, `package-lock.json` (via npm)
- Possibly modify: any file the sweep flags

**Interfaces:**

- Consumes: everything landed in Tasks 1–6.
- Produces: the verified end state — no `use-stick-to-bottom`, all gates green, compliance sweep clean.

- [ ] **Step 1: Remove the dead dependency**

```bash
grep -rn "use-stick-to-bottom" src/   # expected: no matches
npm uninstall use-stick-to-bottom
```

- [ ] **Step 2: Compliance sweep over in-scope files**

Run each; expected output is EMPTY except the named exceptions listed:

```bash
FILES="src/views/workspace/Workspace.tsx src/views/workspace/TranscriptTurn.tsx src/views/workspace/TranscriptRow.tsx src/views/workspace/StreamingStatus.tsx src/views/workspace/PlanTracker.tsx src/views/workspace/WorkspaceTopbar.tsx src/components/Timer.tsx src/components/MarkdownPreview.tsx"

# Arbitrary values/properties — allowed matches: ONLY [view-transition-name:chat-composer] in Workspace.tsx
grep -n "\[[a-z-]*:" $FILES

# Color/border/shadow/radius utilities — allowed matches: NONE
grep -nE '(^|[ "``])(bg|text|border|shadow|rounded|stroke|fill)-' $FILES | grep -v "data-testid\|aria-\|//"

# Typography utilities — allowed matches: prose line in MarkdownPreview.tsx; tabular-nums in Timer.tsx and StreamingStatus.tsx
grep -nE "(prose|font-|text-(xs|sm|base|lg|xl)|leading-|tracking-|tabular-nums|line-through|italic)" $FILES

# Palette colors — allowed matches: NONE
grep -nE "(emerald|amber|slate|zinc|gray|red|blue|green)-[0-9]" $FILES
```

Review every hit against the Global Constraints allowlist; fix violations (move the visual into the ui layer or drop it) rather than allowlisting new exceptions. Note: `text-left`/`text-right`/`text-center` (alignment) are layout, not typography — ignore those hits.

- [ ] **Step 3: No-Radix gate and full suites**

```bash
rg "@radix-ui|radix-ui" src package.json     # expected: no matches
npm run build                                 # expected: exit 0
npm test                                      # expected: all pass (ConversationList.test.tsx has a known full-suite GPU-load flake — rerun it in isolation if it's the only failure)
npm run lint                                  # expected: exit 0
npm run format:check                          # expected: exit 0
```

- [ ] **Step 4: Verify scroll behavior in the running app**

Launch the app (`npm run tauri dev`, or the project's run skill) and verify against the spec's behavior contract:

1. Send a message in a conversation → view snaps to bottom and follows streamed rows.
2. Scroll up during streaming → autoscroll stops (escape), scroll-to-end button appears.
3. Click the button → returns to bottom, button hides, following resumes.
4. Send another message while scrolled up → view re-arms to bottom.
5. Plan tracker card/rail appears over the transcript during a planned turn; streaming status shows Spinner + Working + ticking chron; topbar shows title/path + usage bar with hover tooltip.

If escape or re-arm semantics do NOT hold, STOP and report (spec: do not paper over with custom code).

- [ ] **Step 5: Optional e2e confirmation**

If the environment allows: `DOCE_E2E_SKIP_WIPE=1 npm run test:e2e` — at minimum the `workspace-chat` and `context-window-management` specs. NEVER run e2e without `DOCE_E2E_SKIP_WIPE=1` (the default run wipes real app data). If a run is interrupted, check for orphaned `doce` processes holding the single-instance lock.

- [ ] **Step 6: Final commit**

```bash
npm run format
git add -A && git commit -m "chore: drop use-stick-to-bottom after MessageScroller migration

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
