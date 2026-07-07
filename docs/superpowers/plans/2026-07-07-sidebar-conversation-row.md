# Sidebar Conversation Row Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render sidebar conversation rows as a compact two-line summary with title/time on the first row and workspace path/work state on the second row.

**Architecture:** Keep `ConversationList` as the sidebar owner. Add a small pure helper module for relative time, workspace path labels, and product-facing work-state labels, then wire those helpers into the list after loading workspaces and the current home directory. No backend API changes are needed for the first implementation.

**Tech Stack:** React 19, TypeScript, Vitest, Testing Library, Tauri `homeDir()`, existing `commands.listConversations()` and `commands.listWorkspaces()`.

---

## File Structure

- Create `src/views/chat/sidebarConversationRow.ts`
  - Pure formatting helpers for the sidebar row.
  - No React dependencies.
- Create `src/views/chat/sidebarConversationRow.test.ts`
  - Unit tests for relative time, path labels, workspace lookup fallback, and work-state labels.
- Modify `src/views/chat/ConversationList.tsx`
  - Load workspaces and home path.
  - Build a workspace lookup map.
  - Render the two-line row layout.
- Modify `src/views/chat/ConversationList.test.tsx`
  - Add `listWorkspaces` and `homeDir` mocks.
  - Cover path, relative time, and work-state rendering in the row.

---

### Task 1: Add Sidebar Row Formatting Helpers

**Files:**

- Create: `src/views/chat/sidebarConversationRow.ts`
- Create: `src/views/chat/sidebarConversationRow.test.ts`

- [ ] **Step 1: Write the failing helper tests**

Create `src/views/chat/sidebarConversationRow.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import {
  formatConversationRelativeTime,
  formatWorkspacePathLabel,
  getConversationWorkspaceLabel,
  getConversationWorkStateLabel,
} from "./sidebarConversationRow";

describe("sidebarConversationRow", () => {
  it("formats compact relative update times", () => {
    const now = 1_800_000_000_000;

    expect(formatConversationRelativeTime(now - 30_000, now)).toBe("now");
    expect(formatConversationRelativeTime(now - 2 * 60_000, now)).toBe("2m");
    expect(formatConversationRelativeTime(now - 3 * 60 * 60_000, now)).toBe("3h");
    expect(formatConversationRelativeTime(now - 4 * 24 * 60 * 60_000, now)).toBe("4d");
    expect(formatConversationRelativeTime(now - 40 * 24 * 60 * 60_000, now)).toBe("1mo");
    expect(formatConversationRelativeTime(now - 2 * 365 * 24 * 60 * 60_000, now)).toBe("2y");
  });

  it("formats workspace paths with Home and tilde labels", () => {
    expect(formatWorkspacePathLabel(null, "/Users/tester")).toBe("Home");
    expect(formatWorkspacePathLabel("/Users/tester", "/Users/tester")).toBe("Home");
    expect(formatWorkspacePathLabel("/Users/tester/", "/Users/tester")).toBe("Home");
    expect(formatWorkspacePathLabel("/Users/tester/code/doce", "/Users/tester")).toBe(
      "~/code/doce",
    );
    expect(formatWorkspacePathLabel("/Volumes/projects/doce", "/Users/tester")).toBe(
      "/Volumes/projects/doce",
    );
  });

  it("uses Home while a workspace cannot be resolved by id", () => {
    const workspaces = new Map([
      [
        "ws-code",
        {
          path: "/Users/tester/code/doce",
        },
      ],
    ]);

    expect(getConversationWorkspaceLabel(null, workspaces, "/Users/tester")).toBe("Home");
    expect(getConversationWorkspaceLabel("missing", workspaces, "/Users/tester")).toBe("Home");
    expect(getConversationWorkspaceLabel("ws-code", workspaces, null)).toBe("Home");
    expect(getConversationWorkspaceLabel("ws-code", workspaces, "/Users/tester")).toBe(
      "~/code/doce",
    );
  });

  it("maps technical statuses to product-facing work states", () => {
    expect(getConversationWorkStateLabel("in_progress")).toBe("Working");
    expect(getConversationWorkStateLabel("requires_action")).toBe("Review");
    expect(getConversationWorkStateLabel("failed")).toBe("Blocked");
    expect(getConversationWorkStateLabel("done")).toBe("Ready");
  });
});
```

- [ ] **Step 2: Run the helper test to verify it fails**

Run:

```bash
npm test -- src/views/chat/sidebarConversationRow.test.ts
```

Expected: FAIL because `./sidebarConversationRow` does not exist.

- [ ] **Step 3: Implement the helper module**

Create `src/views/chat/sidebarConversationRow.ts`:

```ts
import type { ConversationStatus, Workspace } from "@/lib/ipc";

type WorkspaceLookup = Map<string, Pick<Workspace, "path">>;

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const MONTH_MS = 30 * DAY_MS;
const YEAR_MS = 365 * DAY_MS;

const WORK_STATE_LABEL: Record<ConversationStatus, string> = {
  in_progress: "Working",
  requires_action: "Review",
  failed: "Blocked",
  done: "Ready",
};

const normalizePath = (path: string) =>
  path.length > 1 && path.endsWith("/") ? path.slice(0, -1) : path;

export function formatConversationRelativeTime(updatedAt: number, now = Date.now()) {
  const elapsed = Math.max(0, now - updatedAt);

  if (elapsed < MINUTE_MS) return "now";
  if (elapsed < HOUR_MS) return `${Math.floor(elapsed / MINUTE_MS)}m`;
  if (elapsed < DAY_MS) return `${Math.floor(elapsed / HOUR_MS)}h`;
  if (elapsed < MONTH_MS) return `${Math.floor(elapsed / DAY_MS)}d`;
  if (elapsed < YEAR_MS) return `${Math.floor(elapsed / MONTH_MS)}mo`;
  return `${Math.floor(elapsed / YEAR_MS)}y`;
}

export function formatWorkspacePathLabel(path: string | null | undefined, homePath: string | null) {
  if (!path) return "Home";

  const normalizedPath = normalizePath(path);
  if (!homePath) return normalizedPath;

  const normalizedHome = normalizePath(homePath);
  if (normalizedPath === normalizedHome) return "Home";
  if (normalizedPath.startsWith(`${normalizedHome}/`)) {
    return `~${normalizedPath.slice(normalizedHome.length)}`;
  }

  return normalizedPath;
}

export function getConversationWorkspaceLabel(
  workspaceId: string | null,
  workspacesById: WorkspaceLookup,
  homePath: string | null,
) {
  if (!workspaceId || !homePath) return "Home";

  const workspace = workspacesById.get(workspaceId);
  if (!workspace) return "Home";

  return formatWorkspacePathLabel(workspace.path, homePath);
}

export function getConversationWorkStateLabel(status: ConversationStatus) {
  return WORK_STATE_LABEL[status];
}
```

- [ ] **Step 4: Run the helper test to verify it passes**

Run:

```bash
npm test -- src/views/chat/sidebarConversationRow.test.ts
```

Expected: PASS for all `sidebarConversationRow` tests.

- [ ] **Step 5: Commit helper module and tests**

Run:

```bash
git add src/views/chat/sidebarConversationRow.ts src/views/chat/sidebarConversationRow.test.ts
git commit -m "test: cover sidebar conversation row formatting"
```

Expected: commit includes only the new helper and helper test files.

---

### Task 2: Render Two-Line Conversation Rows

**Files:**

- Modify: `src/views/chat/ConversationList.test.tsx`
- Modify: `src/views/chat/ConversationList.tsx`

- [ ] **Step 1: Write the failing sidebar rendering test**

Modify the imports and mocks at the top of `src/views/chat/ConversationList.test.tsx`:

```ts
import { createRef } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { homeDir } from "@tauri-apps/api/path";
import ConversationList, { type ConversationListHandle } from "./ConversationList";
import { commands } from "@/lib/ipc";

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn(() => Promise.resolve("/Users/tester")),
}));

vi.mock("@/lib/ipc", () => ({
  commands: {
    listConversations: vi.fn(),
    listWorkspaces: vi.fn(),
  },
}));
```

Update `beforeEach` in the same file:

```ts
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(homeDir).mockResolvedValue("/Users/tester");
  vi.mocked(commands.listWorkspaces).mockResolvedValue([]);
});
```

Add this test inside the `describe("ConversationList", () => { ... })` block:

```ts
it("renders each conversation as title/time plus path/work-state rows", async () => {
  const updatedAt = Date.now() - 2 * 60_000;
  const conversation = {
    id: "active",
    workspaceId: "ws-code",
    title: "Fix fuzzy search ranking",
    createdAt: updatedAt - 60_000,
    updatedAt,
    status: "in_progress" as const,
  };

  vi.mocked(commands.listConversations).mockResolvedValue([conversation]);
  vi.mocked(commands.listWorkspaces).mockResolvedValue([
    {
      id: "ws-code",
      path: "/Users/tester/code/doce",
      displayName: "doce",
      createdAt: 1,
      lastOpenedAt: 2,
    },
  ]);

  render(
    <ConversationList
      activeId="active"
      onSelect={vi.fn()}
      onNewConversation={vi.fn()}
      onOpenSettings={vi.fn()}
    />,
  );

  const row = await screen.findByTestId("conversation-item");

  await waitFor(() => {
    expect(row).toHaveTextContent("Fix fuzzy search ranking");
    expect(row).toHaveTextContent("2m");
    expect(row).toHaveTextContent("~/code/doce");
    expect(row).toHaveTextContent("Working");
  });
});
```

- [ ] **Step 2: Run the sidebar test to verify it fails**

Run:

```bash
npm test -- src/views/chat/ConversationList.test.tsx
```

Expected: FAIL because `ConversationList` has not loaded `listWorkspaces`, has not loaded `homeDir`, and does not render path/work-state text.

- [ ] **Step 3: Implement workspace/home loading and row layout**

Modify the imports at the top of `src/views/chat/ConversationList.tsx`:

```tsx
import {
  forwardRef,
  type MouseEvent,
  useEffect,
  useImperativeHandle,
  useMemo,
  useState,
} from "react";
import { MagnifyingGlassIcon, GearIcon, PlusIcon } from "@phosphor-icons/react";
import { homeDir } from "@tauri-apps/api/path";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Dialog from "@/components/Dialog";
import { Button } from "@/components/ui/button";
import { KeyboardShortcut } from "@/components/ui/KeyboardShortcut";
import { cn } from "@/lib/cn";
import { commands, type Conversation, type ConversationStatus, type Workspace } from "@/lib/ipc";
import SearchPanel from "./SearchPanel";
import {
  formatConversationRelativeTime,
  getConversationWorkspaceLabel,
  getConversationWorkStateLabel,
} from "./sidebarConversationRow";
```

Add state inside `ConversationList` beside the existing conversations/search state:

```tsx
const [conversations, setConversations] = useState<Conversation[]>([]);
const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
const [homePath, setHomePath] = useState<string | null>(null);
const [searching, setSearching] = useState(false);
```

Replace the existing `refresh` function with:

```tsx
const refresh = () => {
  commands.listConversations().then(setConversations);
  commands.listWorkspaces().then(setWorkspaces);
};
```

Add this effect below the existing refresh interval effect:

```tsx
useEffect(() => {
  homeDir()
    .then(setHomePath)
    .catch(() => setHomePath(""));
}, []);
```

Add the workspace lookup below the effects:

```tsx
const workspacesById = useMemo(
  () => new Map(workspaces.map((workspace) => [workspace.id, workspace])),
  [workspaces],
);
```

Replace the current conversation row button inside `conversations.map((c) => (` with:

```tsx
<Button
  key={c.id}
  variant="ghost"
  size="sm"
  onClick={() => onSelect(c)}
  data-testid="conversation-item"
  data-conversation-id={c.id}
  className={cn(
    "h-auto min-h-12 w-full items-start justify-start gap-2 rounded-lg px-2 py-2 text-left",
    c.id === activeId ? "bg-background" : "bg-background/40 hover:bg-background/70",
  )}
>
  <span
    className={cn("mt-1.5 size-2 shrink-0 rounded-full", STATUS_COLOR[c.status])}
    title={STATUS_LABEL[c.status]}
    data-testid="conversation-status-dot"
    data-status={c.status}
  />
  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
    <span className="flex min-w-0 items-baseline gap-2">
      <span className="min-w-0 flex-1 truncate text-[13px] font-semibold leading-4">{c.title}</span>
      <span className="shrink-0 text-[11px] leading-4 text-sidebar-foreground/55 tabular-nums">
        {formatConversationRelativeTime(c.updatedAt)}
      </span>
    </span>
    <span className="flex min-w-0 items-center gap-2">
      <span className="min-w-0 flex-1 truncate text-[11px] leading-4 text-sidebar-foreground/60">
        {getConversationWorkspaceLabel(c.workspaceId, workspacesById, homePath)}
      </span>
      <span className="shrink-0 text-[11px] leading-4 text-sidebar-foreground/60">
        {getConversationWorkStateLabel(c.status)}
      </span>
    </span>
  </span>
</Button>
```

- [ ] **Step 4: Run the sidebar test to verify it passes**

Run:

```bash
npm test -- src/views/chat/ConversationList.test.tsx
```

Expected: PASS for all `ConversationList` tests.

- [ ] **Step 5: Commit sidebar rendering**

Run:

```bash
git add src/views/chat/ConversationList.tsx src/views/chat/ConversationList.test.tsx
git commit -m "feat: render sidebar conversation summaries"
```

Expected: commit includes only the sidebar component and sidebar component tests.

---

### Task 3: Verify Focused Scope

**Files:**

- Test: `src/views/chat/sidebarConversationRow.test.ts`
- Test: `src/views/chat/ConversationList.test.tsx`
- Test: `src/App.test.tsx`

- [ ] **Step 1: Run focused chat/sidebar tests**

Run:

```bash
npm test -- src/views/chat/sidebarConversationRow.test.ts src/views/chat/ConversationList.test.tsx src/App.test.tsx
```

Expected: PASS.

- [ ] **Step 2: Run formatting check**

Run:

```bash
npm run format:check
```

Expected: PASS. If this fails because the new TypeScript formatting differs from `oxfmt`, run `npm run format -- src/views/chat/sidebarConversationRow.ts src/views/chat/sidebarConversationRow.test.ts src/views/chat/ConversationList.tsx src/views/chat/ConversationList.test.tsx`, then rerun `npm run format:check`.

- [ ] **Step 3: Run lint**

Run:

```bash
npm run lint
```

Expected: PASS.

- [ ] **Step 4: Run build**

Run:

```bash
npm run build
```

Expected: PASS.

- [ ] **Step 5: Check staged/untracked files**

Run:

```bash
git status --short
```

Expected: only intentional files from this feature are modified or already committed. Existing unrelated work may still appear and must not be reverted.

---

## Self-Review

Spec coverage:

- Title/time row: covered by Task 2 rendering.
- Path/work-state row: covered by Task 1 helpers and Task 2 rendering.
- `Home`, `~/...`, and absolute path rules: covered by Task 1 helper tests.
- Product-facing work states: covered by Task 1 helper tests and Task 2 rendering.
- No token count: covered by the chosen helper API and rendering code.
- No backend API change: covered by the architecture and Task 2 using `listWorkspaces` plus `homeDir()`.

Placeholder scan:

- No incomplete requirements are present.
- Every code step includes the exact code or command needed for execution.

Type consistency:

- `ConversationStatus`, `Workspace`, `Conversation`, and `commands.listWorkspaces()` names match `src/lib/ipc.ts`.
- The helper API used by `ConversationList.tsx` is defined in Task 1 before Task 2 consumes it.
