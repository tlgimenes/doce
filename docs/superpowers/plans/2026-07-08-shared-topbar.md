# Shared Topbar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a shared transparent draggable topbar across the sidebar and chat region, with active conversation metadata and context usage rendered in the chat topbar.

**Architecture:** Add shell-owned topbar hosts plus a portal API so each app region keeps a stable drag affordance while active views inject optional content. App owns the sidebar/main layout; `ConversationList` becomes the sidebar body; `Workspace` portals active chat metadata into the main topbar when App passes the active conversation.

**Tech Stack:** React 19, TypeScript, Tauri v2 window drag API, Vitest, Testing Library, Tailwind CSS classes, existing `ContextUsageGauge`, existing workspace path formatting helpers.

---

## File Structure

- Create `src/components/Topbar.tsx`
  - Owns `TopbarProvider`, `TopbarHost`, and `TopbarPortal`.
  - Centralizes fixed height, drag markup, and `getCurrentWindow().startDragging()`.
- Create `src/components/Topbar.test.tsx`
  - Tests host markup, portal rendering, and primary-button drag behavior.
- Modify `src/App.tsx`
  - Wrap app shell in `TopbarProvider`.
  - Render sidebar and main `TopbarHost` components.
  - Move sidebar width/background/border to the shell.
  - Pass the full active conversation into `Workspace`.
- Modify `src/App.test.tsx`
  - Assert empty state keeps a blank main topbar.
  - Assert active chat renders metadata in the main topbar.
- Modify `src/views/chat/ConversationList.tsx`
  - Remove sidebar-local `startDragging()` and anonymous top spacer.
  - Keep only sidebar body content below the shell topbar.
- Modify `src/views/chat/ConversationList.test.tsx`
  - Replace the old sidebar affordance assertion with a sidebar-body assertion.
- Create `src/views/workspace/WorkspaceTopbar.tsx`
  - Renders title, workspace path label, and `ContextUsageGauge` into `TopbarPortal target="main"`.
  - Fetches home path and workspaces locally for the metadata label.
- Create `src/views/workspace/WorkspaceTopbar.test.tsx`
  - Tests title, home-compacted path, missing workspace fallback, and context gauge rendering in the portal.
- Modify `src/views/workspace/Workspace.tsx`
  - Accept an optional `conversation` prop for production App usage.
  - Keep `conversationId` supported for direct unit tests to avoid a noisy unrelated test-file rewrite.
  - Render `WorkspaceTopbar` when `conversation` is present.
  - Stop passing `contextGauge` to `RichInput`.

---

### Task 1: Topbar Portal Primitives

**Files:**
- Create: `src/components/Topbar.tsx`
- Create: `src/components/Topbar.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `src/components/Topbar.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { TopbarHost, TopbarPortal, TopbarProvider } from "./Topbar";

const startDragging = vi.fn();

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    startDragging,
  }),
}));

describe("Topbar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    startDragging.mockResolvedValue(undefined);
  });

  it("renders fixed-height draggable hosts for sidebar and main", () => {
    render(
      <TopbarProvider>
        <TopbarHost target="sidebar" />
        <TopbarHost target="main" />
      </TopbarProvider>,
    );

    const sidebar = screen.getByTestId("topbar-sidebar");
    const main = screen.getByTestId("topbar-main");

    expect(sidebar).toHaveClass("h-10", "shrink-0", "select-none");
    expect(main).toHaveClass("h-10", "shrink-0", "select-none");
    expect(sidebar).toHaveAttribute("data-tauri-drag-region");
    expect(main).toHaveAttribute("data-tauri-drag-region");
  });

  it("portals children into the matching host", async () => {
    render(
      <TopbarProvider>
        <TopbarHost target="main" />
        <TopbarPortal target="main">
          <div data-testid="main-topbar-content">Thread title</div>
        </TopbarPortal>
      </TopbarProvider>,
    );

    const host = screen.getByTestId("topbar-main");
    expect(await screen.findByTestId("main-topbar-content")).toBeInTheDocument();
    expect(host).toHaveTextContent("Thread title");
  });

  it("starts dragging only for the primary mouse button", () => {
    render(
      <TopbarProvider>
        <TopbarHost target="main" />
      </TopbarProvider>,
    );

    const host = screen.getByTestId("topbar-main");
    fireEvent.mouseDown(host, { button: 2 });
    expect(startDragging).not.toHaveBeenCalled();

    fireEvent.mouseDown(host, { button: 0 });
    expect(startDragging).toHaveBeenCalledTimes(1);
  });

  it("does not start dragging from children marked as non-drag controls", () => {
    render(
      <TopbarProvider>
        <TopbarHost target="main">
          <button type="button" data-topbar-no-drag>
            Control
          </button>
        </TopbarHost>
      </TopbarProvider>,
    );

    fireEvent.mouseDown(screen.getByRole("button", { name: "Control" }), { button: 0 });
    expect(startDragging).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
npm test -- src/components/Topbar.test.tsx
```

Expected: FAIL because `src/components/Topbar.tsx` does not exist.

- [ ] **Step 3: Implement the topbar primitives**

Create `src/components/Topbar.tsx`:

```tsx
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type MouseEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { cn } from "@/lib/cn";

export type TopbarTarget = "sidebar" | "main";

type TopbarHosts = Partial<Record<TopbarTarget, HTMLDivElement>>;

interface TopbarContextValue {
  hosts: TopbarHosts;
  registerHost: (target: TopbarTarget, element: HTMLDivElement | null) => void;
}

const TopbarContext = createContext<TopbarContextValue | null>(null);

function useTopbarContext() {
  const context = useContext(TopbarContext);
  if (!context) {
    throw new Error("Topbar components must be rendered inside TopbarProvider");
  }
  return context;
}

export function TopbarProvider({ children }: { children: ReactNode }) {
  const [hosts, setHosts] = useState<TopbarHosts>({});

  const registerHost = useCallback((target: TopbarTarget, element: HTMLDivElement | null) => {
    setHosts((current) => {
      if (current[target] === element) return current;
      const next = { ...current };
      if (element) {
        next[target] = element;
      } else {
        delete next[target];
      }
      return next;
    });
  }, []);

  const value = useMemo(() => ({ hosts, registerHost }), [hosts, registerHost]);

  return <TopbarContext.Provider value={value}>{children}</TopbarContext.Provider>;
}

interface TopbarHostProps {
  target: TopbarTarget;
  className?: string;
  children?: ReactNode;
}

export function TopbarHost({ target, className, children }: TopbarHostProps) {
  const { registerHost } = useTopbarContext();

  const ref = useCallback(
    (element: HTMLDivElement | null) => {
      registerHost(target, element);
    },
    [registerHost, target],
  );

  const startDrag = async (event: MouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const targetElement = event.target as HTMLElement | null;
    if (targetElement?.closest("[data-topbar-no-drag]")) return;

    event.preventDefault();
    await getCurrentWindow()
      .startDragging()
      .catch((error) => {
        console.error("Failed to start window dragging", error);
      });
  };

  return (
    <div
      ref={ref}
      className={cn(
        "flex h-10 shrink-0 select-none items-center bg-transparent",
        className,
      )}
      data-tauri-drag-region
      data-testid={`topbar-${target}`}
      onMouseDown={startDrag}
    >
      {children}
    </div>
  );
}

export function TopbarPortal({
  target,
  children,
}: {
  target: TopbarTarget;
  children: ReactNode;
}) {
  const { hosts } = useTopbarContext();
  const host = hosts[target];
  if (!host) return null;
  return createPortal(children, host);
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
npm test -- src/components/Topbar.test.tsx
```

Expected: PASS, 4 tests.

- [ ] **Step 5: Commit**

Run:

```bash
git add src/components/Topbar.tsx src/components/Topbar.test.tsx
git commit -m "feat: add shared topbar portal primitives"
```

---

### Task 2: App Shell Hosts And Sidebar Body Refactor

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`
- Modify: `src/views/chat/ConversationList.tsx`
- Modify: `src/views/chat/ConversationList.test.tsx`

- [ ] **Step 1: Write the failing App shell test**

In `src/App.test.tsx`, add this test inside the first `describe(...)` block after `marks the main content pane as the chat-surface view transition target`:

```tsx
  it("renders shared sidebar and main topbars while the empty state keeps the main topbar blank", async () => {
    render(<App />);
    await waitForReady();

    const sidebarTopbar = screen.getByTestId("topbar-sidebar");
    const mainTopbar = screen.getByTestId("topbar-main");

    expect(sidebarTopbar).toHaveClass("h-10", "shrink-0");
    expect(mainTopbar).toHaveClass("h-10", "shrink-0");
    expect(sidebarTopbar).toHaveAttribute("data-tauri-drag-region");
    expect(mainTopbar).toHaveAttribute("data-tauri-drag-region");
    expect(mainTopbar).toBeEmptyDOMElement();
    expect(screen.getByTestId("empty-state-input")).toBeInTheDocument();
  });
```

- [ ] **Step 2: Update the sidebar affordance test to describe the new body boundary**

In `src/views/chat/ConversationList.test.tsx`, replace the test named:

```tsx
it("keeps the window drag affordance and actions in place while search opens in a dialog", async () => {
```

with:

```tsx
it("renders sidebar actions at the top of the sidebar body while search opens in a dialog", async () => {
  vi.mocked(commands.listConversations).mockResolvedValue([]);

  render(
    <ConversationList
      activeId={null}
      onSelect={vi.fn()}
      onNewConversation={vi.fn()}
      onOpenSettings={vi.fn()}
    />,
  );

  const sidebar = await screen.findByTestId("conversation-list");
  const actions = screen.getByTestId("sidebar-actions");
  expect(sidebar.firstElementChild).toBe(actions);
  expect(actions).not.toHaveClass("mt-8");

  await userEvent.click(screen.getByTestId("open-search"));

  const searchPanel = screen.getByTestId("search-panel");
  expect(searchPanel.closest("dialog")).toBeInTheDocument();
  expect(sidebar.firstElementChild).toBe(actions);
  expect(actions).toBeInTheDocument();
});
```

- [ ] **Step 3: Run the targeted tests to verify they fail**

Run:

```bash
npm test -- src/App.test.tsx src/views/chat/ConversationList.test.tsx -t "shared sidebar|sidebar actions"
```

Expected:

- App test fails because `topbar-sidebar` and `topbar-main` do not exist.
- ConversationList test fails because the old spacer is still the first child.

- [ ] **Step 4: Refactor `ConversationList` into the sidebar body**

In `src/views/chat/ConversationList.tsx`:

1. Remove `getCurrentWindow` from imports:

```tsx
import { homeDir } from "@tauri-apps/api/path";
```

2. Delete the `startDrag` function.

3. Replace the root container classes and remove the anonymous drag spacer.

Replace:

```tsx
      <div
        className="relative flex h-dvh w-64 shrink-0 flex-col border-r border-sidebar-border bg-sidebar px-2 pb-3 pt-0 text-sidebar-foreground"
        data-testid="conversation-list"
      >
        <div
          className="-mx-2 h-10 shrink-0 select-none"
          data-tauri-drag-region
          data-testid="sidebar-window-affordance"
          onMouseDown={startDrag}
        />
        <div className="mb-3 flex flex-col gap-0.5" data-testid="sidebar-actions">
```

with:

```tsx
      <div
        className="relative flex min-h-0 flex-1 flex-col px-2 pb-3 text-sidebar-foreground"
        data-testid="conversation-list"
      >
        <div className="mb-3 flex flex-col gap-0.5" data-testid="sidebar-actions">
```

- [ ] **Step 5: Refactor `App` shell to render both hosts**

In `src/App.tsx`, add the import:

```tsx
import { TopbarHost, TopbarProvider } from "@/components/Topbar";
```

Then replace the entire `return (` block at the end of `App` with this complete
structure:

```tsx
  return (
    <TopbarProvider>
      <div className="flex h-dvh">
        <div className="flex w-64 shrink-0 flex-col border-r border-sidebar-border bg-sidebar">
          <TopbarHost target="sidebar" className="px-2" />
          <ConversationList
            ref={conversationListRef}
            activeId={activeConversation?.id ?? null}
            onSelect={(conversation) => {
              setShowSettings(false);
              setPendingInitialTurn(null);
              setActiveConversation(conversation);
              markSeen(conversation.id);
            }}
            onNewConversation={() => {
              setShowSettings(false);
              setPendingInitialTurn(null);
              setActiveConversation(null);
            }}
            onOpenSettings={() => setShowSettings(true)}
            onArchive={(conversationId) => {
              if (activeConversation?.id !== conversationId) return;
              setShowSettings(false);
              setPendingInitialTurn(null);
              setActiveConversation(null);
            }}
          />
        </div>
        <div className="flex min-w-0 flex-1 flex-col">
          <TopbarHost target="main" className="px-4" />
          <div
            className="min-h-0 flex-1 [view-transition-name:chat-surface]"
            data-testid="app-content-pane"
          >
            {showWidgetGallery ? (
              <WidgetGallery onClose={() => setShowWidgetGallery(false)} />
            ) : showSettings ? (
              <Settings onClose={() => setShowSettings(false)} />
            ) : activeConversation ? (
              <Workspace
                key={activeConversation.id}
                conversationId={activeConversation.id}
                pendingInitialTurn={
                  pendingInitialTurn?.conversationId === activeConversation.id
                    ? pendingInitialTurn
                    : null
                }
                onPendingInitialTurnConsumed={(conversationId) =>
                  setPendingInitialTurn((prev) =>
                    prev?.conversationId === conversationId ? null : prev,
                  )
                }
                onConversationSeen={markSeen}
              />
            ) : (
              <EmptyState onConversationCreated={activateConversation} />
            )}
          </div>
        </div>
        <ShortcutsDialog
          open={showShortcutsDialog}
          onClose={() => setShowShortcutsDialog(false)}
          shortcuts={shortcuts}
        />
      </div>
    </TopbarProvider>
  );
```

- [ ] **Step 6: Run the targeted tests to verify they pass**

Run:

```bash
npm test -- src/App.test.tsx src/views/chat/ConversationList.test.tsx -t "shared sidebar|sidebar actions"
```

Expected: PASS, 2 tests.

- [ ] **Step 7: Run the sidebar/App files**

Run:

```bash
npm test -- src/App.test.tsx src/views/chat/ConversationList.test.tsx
```

Expected: PASS.

- [ ] **Step 8: Commit**

Run:

```bash
git add src/App.tsx src/App.test.tsx src/views/chat/ConversationList.tsx src/views/chat/ConversationList.test.tsx
git commit -m "feat: add shared shell topbar hosts"
```

---

### Task 3: Workspace Topbar Content And Composer Gauge Move

**Files:**
- Create: `src/views/workspace/WorkspaceTopbar.tsx`
- Create: `src/views/workspace/WorkspaceTopbar.test.tsx`
- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`

- [ ] **Step 1: Write failing WorkspaceTopbar tests**

Create `src/views/workspace/WorkspaceTopbar.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { homeDir } from "@tauri-apps/api/path";
import { TopbarHost, TopbarProvider } from "@/components/Topbar";
import { commands } from "@/lib/ipc";
import { useContextUsageStore } from "@/state/contextUsageStore";
import WorkspaceTopbar from "./WorkspaceTopbar";

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn(() => Promise.resolve("/Users/tester")),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    startDragging: vi.fn(),
  }),
}));

vi.mock("@/lib/ipc", () => ({
  commands: {
    listWorkspaces: vi.fn(),
    getContextUsage: vi.fn(),
  },
}));

function renderTopbar() {
  return render(
    <TopbarProvider>
      <TopbarHost target="main" />
      <WorkspaceTopbar
        conversation={{
          id: "conv-1",
          workspaceId: "ws-code",
          title: "Design shared topbar",
        }}
      />
    </TopbarProvider>,
  );
}

describe("WorkspaceTopbar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useContextUsageStore.setState({ usage: {} });
    vi.mocked(homeDir).mockResolvedValue("/Users/tester");
    vi.mocked(commands.listWorkspaces).mockResolvedValue([
      {
        id: "ws-code",
        path: "/Users/tester/code/doce",
        displayName: "doce",
        createdAt: 1,
        lastOpenedAt: 2,
      },
    ]);
    vi.mocked(commands.getContextUsage).mockResolvedValue({
      conversationId: "conv-1",
      tokensUsed: 512,
      tokenBudget: 2048,
      state: "normal",
    });
  });

  it("portals conversation title, workspace path, and context usage into the main topbar", async () => {
    renderTopbar();

    expect(await screen.findByTestId("workspace-topbar")).toBeInTheDocument();
    expect(screen.getByTestId("topbar-main")).toHaveTextContent("Design shared topbar");
    expect(screen.getByTestId("workspace-topbar-title")).toHaveTextContent(
      "Design shared topbar",
    );
    await waitFor(() =>
      expect(screen.getByTestId("workspace-topbar-path")).toHaveTextContent("~/code/doce"),
    );
    expect(await screen.findByTestId("context-usage-gauge")).toHaveAttribute(
      "aria-label",
      expect.stringContaining("25%"),
    );
  });

  it("falls back to Home when the conversation has no workspace id", async () => {
    render(
      <TopbarProvider>
        <TopbarHost target="main" />
        <WorkspaceTopbar
          conversation={{
            id: "conv-home",
            workspaceId: null,
            title: "Home conversation",
          }}
        />
      </TopbarProvider>,
    );

    expect(await screen.findByTestId("workspace-topbar-title")).toHaveTextContent(
      "Home conversation",
    );
    expect(screen.getByTestId("workspace-topbar-path")).toHaveTextContent("Home");
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
npm test -- src/views/workspace/WorkspaceTopbar.test.tsx
```

Expected: FAIL because `WorkspaceTopbar.tsx` does not exist.

- [ ] **Step 3: Implement `WorkspaceTopbar`**

Create `src/views/workspace/WorkspaceTopbar.tsx`:

```tsx
import { useEffect, useMemo, useState } from "react";
import { homeDir } from "@tauri-apps/api/path";
import ContextUsageGauge from "@/components/ContextUsageGauge";
import { TopbarPortal } from "@/components/Topbar";
import { commands, type Conversation, type Workspace } from "@/lib/ipc";
import { getConversationWorkspaceLabel } from "@/views/chat/sidebarConversationRow";

export type WorkspaceTopbarConversation = Pick<Conversation, "id" | "title" | "workspaceId">;

interface WorkspaceTopbarProps {
  conversation: WorkspaceTopbarConversation;
}

export default function WorkspaceTopbar({ conversation }: WorkspaceTopbarProps) {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [homePath, setHomePath] = useState<string | null>(null);

  useEffect(() => {
    homeDir()
      .then(setHomePath)
      .catch(() => setHomePath(""));
  }, []);

  useEffect(() => {
    commands.listWorkspaces().then(setWorkspaces).catch(console.error);
  }, []);

  const workspacesById = useMemo(
    () => new Map(workspaces.map((workspace) => [workspace.id, workspace])),
    [workspaces],
  );

  const workspacePathLabel = getConversationWorkspaceLabel(
    conversation.workspaceId,
    workspacesById,
    homePath,
  );

  return (
    <TopbarPortal target="main">
      <div
        className="flex min-w-0 flex-1 items-center justify-between gap-3"
        data-testid="workspace-topbar"
      >
        <div className="min-w-0">
          <div
            className="truncate text-sm font-medium text-foreground"
            data-testid="workspace-topbar-title"
          >
            {conversation.title}
          </div>
          <div
            className="truncate text-xs text-muted-foreground"
            data-testid="workspace-topbar-path"
          >
            {workspacePathLabel}
          </div>
        </div>
        <div data-topbar-no-drag>
          <ContextUsageGauge conversationId={conversation.id} />
        </div>
      </div>
    </TopbarPortal>
  );
}
```

- [ ] **Step 4: Run `WorkspaceTopbar` tests to verify they pass**

Run:

```bash
npm test -- src/views/workspace/WorkspaceTopbar.test.tsx
```

Expected: PASS, 2 tests.

- [ ] **Step 5: Write failing App integration tests**

In `src/App.test.tsx`, add this test after the empty-topbar test from Task 2:

```tsx
  it("renders active conversation metadata in the main topbar and keeps the composer free of context chrome", async () => {
    const conversation = {
      id: "c-topbar",
      workspaceId: "ws-code",
      title: "Shared topbar plan",
      createdAt: 1,
      updatedAt: 1,
      lastSeenAt: 1,
      status: "done" as const,
    };
    vi.mocked(commands.listConversations).mockResolvedValue([conversation]);
    vi.mocked(commands.listWorkspaces).mockResolvedValue([
      {
        id: "ws-code",
        path: "/Users/tester/code/doce",
        displayName: "doce",
        createdAt: 1,
        lastOpenedAt: 1,
      },
    ]);
    vi.mocked(commands.getContextUsage).mockResolvedValue({
      conversationId: "c-topbar",
      tokensUsed: 256,
      tokenBudget: 1024,
      state: "normal",
    });

    render(<App />);

    await userEvent.click(await screen.findByText("Shared topbar plan"));

    const mainTopbar = screen.getByTestId("topbar-main");
    expect(await within(mainTopbar).findByTestId("workspace-topbar-title")).toHaveTextContent(
      "Shared topbar plan",
    );
    await waitFor(() =>
      expect(within(mainTopbar).getByTestId("workspace-topbar-path")).toHaveTextContent(
        "~/code/doce",
      ),
    );
    expect(await within(mainTopbar).findByTestId("context-usage-gauge")).toHaveAttribute(
      "aria-label",
      expect.stringContaining("25%"),
    );

    const composer = screen.getByTestId("workspace-composer-shell");
    expect(within(composer).queryByTestId("context-usage-gauge")).not.toBeInTheDocument();
  });
```

- [ ] **Step 6: Run the App integration test to verify it fails**

Run:

```bash
npm test -- src/App.test.tsx -t "active conversation metadata"
```

Expected: FAIL because App does not pass `conversation` to `Workspace`, Workspace does not render `WorkspaceTopbar`, and the composer still owns the gauge.

- [ ] **Step 7: Update `Workspace` props and render `WorkspaceTopbar`**

In `src/views/workspace/Workspace.tsx`:

1. Replace the `ContextUsageGauge` import with `WorkspaceTopbar`:

```tsx
import WorkspaceTopbar, { type WorkspaceTopbarConversation } from "./WorkspaceTopbar";
```

2. Remove this import:

```tsx
import ContextUsageGauge from "@/components/ContextUsageGauge";
```

3. Replace the props interface:

```tsx
interface WorkspaceProps {
  conversationId: string;
  pendingInitialTurn?: PendingInitialTurn | null;
  onPendingInitialTurnConsumed?: (conversationId: string) => void;
  onConversationSeen?: (conversationId: string) => void;
}
```

with:

```tsx
interface WorkspaceProps {
  conversation?: WorkspaceTopbarConversation;
  conversationId?: string;
  pendingInitialTurn?: PendingInitialTurn | null;
  onPendingInitialTurnConsumed?: (conversationId: string) => void;
  onConversationSeen?: (conversationId: string) => void;
}
```

4. Change the function signature:

```tsx
export default function Workspace({
  conversation,
  conversationId: explicitConversationId,
  pendingInitialTurn,
  onPendingInitialTurnConsumed,
  onConversationSeen,
}: WorkspaceProps) {
  const conversationId = conversation?.id ?? explicitConversationId;
  if (!conversationId) {
    throw new Error("Workspace requires a conversation or conversationId");
  }
```

5. Render `WorkspaceTopbar` at the top of the returned root:

```tsx
  return (
    <div className="flex h-full flex-col bg-background text-foreground">
      {conversation && <WorkspaceTopbar conversation={conversation} />}
```

This replaces the current:

```tsx
  return (
    <div className="flex h-dvh flex-col bg-background text-foreground">
```

6. Remove `contextGauge` from `RichInput`.

Replace:

```tsx
            contextGauge={<ContextUsageGauge conversationId={conversationId} />}
```

with no prop.

- [ ] **Step 8: Update App to pass the active conversation**

In `src/App.tsx`, replace the `Workspace` invocation's `conversationId` prop:

```tsx
              <Workspace
                key={activeConversation.id}
                conversationId={activeConversation.id}
```

with:

```tsx
              <Workspace
                key={activeConversation.id}
                conversation={activeConversation}
```

Keep every other prop unchanged.

- [ ] **Step 9: Run the App integration test to verify it passes**

Run:

```bash
npm test -- src/App.test.tsx -t "active conversation metadata"
```

Expected: PASS.

- [ ] **Step 10: Run Workspace and App tests**

Run:

```bash
npm test -- src/views/workspace/WorkspaceTopbar.test.tsx src/views/workspace/Workspace.test.tsx src/App.test.tsx
```

Expected: PASS.

- [ ] **Step 11: Commit**

Run:

```bash
git add src/views/workspace/WorkspaceTopbar.tsx src/views/workspace/WorkspaceTopbar.test.tsx src/views/workspace/Workspace.tsx src/App.tsx src/App.test.tsx
git commit -m "feat: show conversation metadata in shared topbar"
```

---

### Task 4: Final Regression Pass

**Files:**
- Verify only unless failures expose a required small fix.

- [ ] **Step 1: Run the focused frontend test set**

Run:

```bash
npm test -- src/components/Topbar.test.tsx src/views/workspace/WorkspaceTopbar.test.tsx src/views/chat/ConversationList.test.tsx src/App.test.tsx src/views/workspace/Workspace.test.tsx src/views/chat/SearchPanel.test.tsx
```

Expected: PASS. The total test count will be higher than before because `Topbar.test.tsx` and `WorkspaceTopbar.test.tsx` are new.

- [ ] **Step 2: Run lint**

Run:

```bash
npm run lint
```

Expected: PASS with no oxlint errors.

- [ ] **Step 3: Run production build**

Run:

```bash
npm run build
```

Expected: PASS. The existing Vite chunk-size warning may still appear.

- [ ] **Step 4: Run frontend formatting check**

Run:

```bash
./node_modules/.bin/oxfmt --check src/components/Topbar.tsx src/components/Topbar.test.tsx src/App.tsx src/App.test.tsx src/views/chat/ConversationList.tsx src/views/chat/ConversationList.test.tsx src/views/workspace/Workspace.tsx src/views/workspace/WorkspaceTopbar.tsx src/views/workspace/WorkspaceTopbar.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Inspect final diff**

Run:

```bash
git status --short --branch
git diff --stat HEAD~3..HEAD
```

Expected:

- Working tree clean.
- Three feature commits on top of the plan commit:
  - `feat: add shared topbar portal primitives`
  - `feat: add shared shell topbar hosts`
  - `feat: show conversation metadata in shared topbar`

- [ ] **Step 6: Report completion**

Summarize:

- topbar hosts exist for sidebar and main;
- empty state leaves main topbar blank;
- active chat portals title, workspace path, and context gauge;
- context gauge is no longer inside the composer;
- verification commands and outcomes.
