# Conversation Seen State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist when a conversation was last seen and render inactive sidebar conversation titles in bold when they have newer activity.

**Architecture:** Add a durable `conversations.last_seen_at` timestamp and expose it through the existing conversation IPC shape. A small `mark_conversation_seen` command updates the marker when the user opens or watches a conversation. The sidebar derives the bold state from `updatedAt > lastSeenAt` while keeping active conversations visually normal.

**Tech Stack:** Rust/Tauri commands, rusqlite migrations, Specta/hand-written IPC wrappers, React, Vitest, Tailwind classes.

---

## File Structure

- Create `src-tauri/src/storage/migrations/0008_conversation_last_seen_at.sql`
  - Adds `last_seen_at` and backfills existing rows from `updated_at`.
- Modify `src-tauri/src/storage/migrations.rs`
  - Registers migration 8 and adds migration coverage.
- Modify `src-tauri/src/commands/conversations.rs`
  - Adds `last_seen_at` to `Conversation`, inserts it for new rows, selects it in `list_conversations`, and adds `mark_conversation_seen`.
- Modify `src-tauri/src/agent/subagent.rs`
  - Supplies `last_seen_at` for subagent conversation inserts so schema defaults are not relied on.
- Modify `src-tauri/src/commands/mod.rs`
  - Exposes `mark_conversation_seen` through the Tauri/Specta command builder.
- Modify `src/lib/ipc.ts`
  - Adds `lastSeenAt` to the frontend `Conversation` type and hand-written command wrapper.
- Modify `src/lib/bindings.ts`
  - Regenerate or update the generated binding after Specta changes. Prefer running the repo’s existing generation path if available; otherwise apply the minimal generated diff.
- Modify `src/App.tsx`
  - Marks conversations seen when selected/opened and passes a watcher callback to `Workspace`.
- Modify `src/views/workspace/Workspace.tsx`
  - Calls a callback when active conversation messages refresh while open.
- Modify `src/views/chat/ConversationList.tsx`
  - Renders inactive unseen titles in bold and active titles normal.
- Modify tests:
  - `src-tauri/src/storage/migrations.rs`
  - `src-tauri/src/commands/conversations.rs`
  - `src/views/chat/ConversationList.test.tsx`
  - `src/App.test.tsx`
  - `src/views/workspace/Workspace.test.tsx`

---

### Task 1: Add `last_seen_at` Migration

**Files:**

- Create: `src-tauri/src/storage/migrations/0008_conversation_last_seen_at.sql`
- Modify: `src-tauri/src/storage/migrations.rs`

- [ ] **Step 1: Write migration tests**

Add these tests to `src-tauri/src/storage/migrations.rs` inside the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn conversation_last_seen_at_is_backfilled_from_updated_at() {
    let mut conn = Connection::open_in_memory().unwrap();
    apply_up_to(&mut conn, 7);
    conn.execute(
        "INSERT INTO conversations (id, workspace_id, spawned_by_conversation_id, title, created_at, updated_at) VALUES ('c1', NULL, NULL, 'Seen test', 10, 42)",
        [],
    )
    .unwrap();

    apply_pending(&mut conn).unwrap();

    let last_seen_at: i64 = conn
        .query_row(
            "SELECT last_seen_at FROM conversations WHERE id = 'c1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(last_seen_at, 42);
}

#[test]
fn conversation_last_seen_at_is_not_nullable_after_migration() {
    let mut conn = Connection::open_in_memory().unwrap();
    apply_pending(&mut conn).unwrap();

    conn.execute(
        "INSERT INTO conversations (id, title, created_at, updated_at) VALUES ('c1', 'x', 1, 2)",
        [],
    )
    .unwrap();

    let last_seen_at: i64 = conn
        .query_row(
            "SELECT last_seen_at FROM conversations WHERE id = 'c1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(last_seen_at, 0);
}
```

- [ ] **Step 2: Run migration tests and verify failure**

Run:

```bash
cd src-tauri
cargo test storage::migrations::tests::conversation_last_seen_at
```

Expected: tests fail because `last_seen_at` does not exist.

- [ ] **Step 3: Create migration file**

Create `src-tauri/src/storage/migrations/0008_conversation_last_seen_at.sql`:

```sql
ALTER TABLE conversations ADD COLUMN last_seen_at INTEGER NOT NULL DEFAULT 0;

UPDATE conversations
SET last_seen_at = updated_at
WHERE last_seen_at = 0;
```

- [ ] **Step 4: Register migration 8**

In `src-tauri/src/storage/migrations.rs`, extend `MIGRATIONS`:

```rust
    (
        8,
        include_str!("migrations/0008_conversation_last_seen_at.sql"),
    ),
```

Place it after migration 7.

- [ ] **Step 5: Run migration tests**

Run:

```bash
cd src-tauri
cargo test storage::migrations::tests::conversation_last_seen_at
```

Expected: both tests pass.

- [ ] **Step 6: Commit migration**

```bash
git add src-tauri/src/storage/migrations.rs src-tauri/src/storage/migrations/0008_conversation_last_seen_at.sql
git commit -m "feat: add conversation seen timestamp"
```

---

### Task 2: Expose `last_seen_at` and Add Mark-Seen Command

**Files:**

- Modify: `src-tauri/src/commands/conversations.rs`
- Modify: `src-tauri/src/agent/subagent.rs`
- Modify: `src-tauri/src/commands/mod.rs`

- [ ] **Step 1: Write backend helper tests**

In `src-tauri/src/commands/conversations.rs`, add tests inside the existing `#[cfg(test)] mod tests` using the existing `test_connection()` pattern:

```rust
#[test]
fn mark_conversation_seen_in_conn_sets_marker_to_at_least_updated_at() {
    let conn = test_connection();
    conn.execute(
        "INSERT INTO conversations (id, title, created_at, updated_at, last_seen_at) VALUES ('c1', 'x', 1, 100, 2)",
        [],
    )
    .unwrap();

    mark_conversation_seen_in_conn(&conn, "c1", 50).unwrap();

    let last_seen_at: i64 = conn
        .query_row(
            "SELECT last_seen_at FROM conversations WHERE id = 'c1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(last_seen_at, 100);
}

#[test]
fn mark_conversation_seen_in_conn_uses_now_when_it_is_newer_than_updated_at() {
    let conn = test_connection();
    conn.execute(
        "INSERT INTO conversations (id, title, created_at, updated_at, last_seen_at) VALUES ('c1', 'x', 1, 100, 2)",
        [],
    )
    .unwrap();

    mark_conversation_seen_in_conn(&conn, "c1", 150).unwrap();

    let last_seen_at: i64 = conn
        .query_row(
            "SELECT last_seen_at FROM conversations WHERE id = 'c1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(last_seen_at, 150);
}
```

- [ ] **Step 2: Run backend tests and verify failure**

Run:

```bash
cd src-tauri
cargo test commands::conversations::tests::mark_conversation_seen_in_conn
```

Expected: compile failure because `mark_conversation_seen_in_conn` does not exist yet.

- [ ] **Step 3: Extend the Rust `Conversation` type**

In `src-tauri/src/commands/conversations.rs`, update:

```rust
pub struct Conversation {
    pub id: String,
    pub workspace_id: Option<String>,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_seen_at: i64,
    /// Computed live, never cached (FR-011): `in_progress` | `requires_action`
    /// | `failed` | `done`.
    pub status: String,
}
```

- [ ] **Step 4: Set `last_seen_at` on conversation creation**

In `create_conversation`, replace the insert SQL with:

```rust
"INSERT INTO conversations (id, workspace_id, title, created_at, updated_at, last_seen_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
```

and pass `now` as parameter 6.

Update the returned `Conversation`:

```rust
Ok(Conversation {
    id,
    workspace_id,
    title,
    created_at: now,
    updated_at: now,
    last_seen_at: now,
    status: "done".to_string(),
})
```

- [ ] **Step 5: Include `last_seen_at` in list query**

In `list_conversations`, replace the select with:

```rust
"SELECT id, workspace_id, title, created_at, updated_at, last_seen_at FROM conversations
 WHERE spawned_by_conversation_id IS NULL
 AND (?1 IS NULL OR workspace_id = ?1)
 ORDER BY updated_at DESC"
```

Map the sixth column:

```rust
Ok((
    row.get::<_, String>(0)?,
    row.get::<_, Option<String>>(1)?,
    row.get::<_, String>(2)?,
    row.get::<_, i64>(3)?,
    row.get::<_, i64>(4)?,
    row.get::<_, i64>(5)?,
))
```

and construct:

```rust
Ok(Conversation {
    id,
    workspace_id,
    title,
    created_at,
    updated_at,
    last_seen_at,
    status,
})
```

- [ ] **Step 6: Add DB helper and `mark_conversation_seen` command**

Add to `src-tauri/src/commands/conversations.rs` near `list_conversations`:

```rust
fn mark_conversation_seen_in_conn(
    conn: &Connection,
    conversation_id: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE conversations
         SET last_seen_at = MAX(?1, updated_at)
         WHERE id = ?2",
        rusqlite::params![now, conversation_id],
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn mark_conversation_seen(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    conversation_id: String,
) -> Result<(), String> {
    let conn = db_cell.get(&app).await?;
    let now = now_ms();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        mark_conversation_seen_in_conn(conn, &conversation_id, now)
    })
    .await
    .map_err(|e| e.to_string())
}
```

- [ ] **Step 7: Update subagent conversation inserts**

In `src-tauri/src/agent/subagent.rs`, update inserts that create conversations so they include `last_seen_at`:

```rust
"INSERT INTO conversations (id, spawned_by_conversation_id, title, created_at, updated_at, last_seen_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
```

Pass `now` for the new parameter.

Update any test-only inserts in the same file similarly:

```sql
INSERT INTO conversations (id, title, created_at, updated_at, last_seen_at, spawned_by_conversation_id)
VALUES (?1, 'x', 0, 0, 0, ?2)
```

- [ ] **Step 8: Register command in Specta builder**

In `src-tauri/src/commands/mod.rs`, add `conversations::mark_conversation_seen` to `collect_commands!` immediately after `conversations::list_messages`.

- [ ] **Step 9: Run backend tests**

Run:

```bash
cd src-tauri
cargo test commands::conversations::tests::mark_conversation_seen_in_conn
cargo test storage::migrations::tests::conversation_last_seen_at
```

Expected: all selected tests pass.

- [ ] **Step 10: Commit backend API**

```bash
git add src-tauri/src/commands/conversations.rs src-tauri/src/agent/subagent.rs src-tauri/src/commands/mod.rs
git commit -m "feat: expose conversation seen state"
```

---

### Task 3: Update Frontend IPC Types

**Files:**

- Modify: `src/lib/ipc.ts`
- Modify: `src/lib/bindings.ts`

- [ ] **Step 1: Update hand-written IPC wrapper**

In `src/lib/ipc.ts`, update `Conversation`:

```ts
export interface Conversation {
  id: string;
  workspaceId: string | null;
  title: string;
  createdAt: number;
  updatedAt: number;
  lastSeenAt: number;
  status: ConversationStatus;
}
```

Add command wrapper:

```ts
markConversationSeen: (conversationId: string) =>
  invoke<void>("mark_conversation_seen", { conversationId }),
```

Place it near `listConversations` / `listMessages`.

- [ ] **Step 2: Update generated bindings**

Regenerate `src/lib/bindings.ts` using the project’s normal Specta generation path. If no direct script exists, make the minimal equivalent edit:

```ts
createConversation: (workspaceId: string | null) => typedError<Conversation, string>(__TAURI_INVOKE("create_conversation", { workspaceId })).then((v) => ((v.status === "ok" ? { ...v, data: ({...v.data,createdAt:BigInt(v.data.createdAt),updatedAt:BigInt(v.data.updatedAt),lastSeenAt:BigInt(v.data.lastSeenAt)}) } : v) as typeof v)),
listConversations: (workspaceId: string | null) => typedError<Conversation[], string>(__TAURI_INVOKE("list_conversations", { workspaceId })).then((v) => ((v.status === "ok" ? { ...v, data: v.data.map(i=>({...i,createdAt:BigInt(i.createdAt),updatedAt:BigInt(i.updatedAt),lastSeenAt:BigInt(i.lastSeenAt)})) } : v) as typeof v)),
markConversationSeen: (conversationId: string) => typedError<null, string>(__TAURI_INVOKE("mark_conversation_seen", { conversationId })),
```

Update the generated `Conversation` type with:

```ts
lastSeenAt: bigint,
```

- [ ] **Step 3: Typecheck frontend**

Run:

```bash
npm run build
```

Expected: TypeScript either passes or reports `lastSeenAt` missing from conversation fixtures. If fixtures fail, add `lastSeenAt` in the exact test files named by TypeScript before moving to Task 4.

- [ ] **Step 4: Commit IPC updates**

```bash
git add src/lib/ipc.ts src/lib/bindings.ts
git commit -m "feat: type conversation seen state"
```

---

### Task 4: Render Unseen Sidebar Titles

**Files:**

- Modify: `src/views/chat/ConversationList.tsx`
- Modify: `src/views/chat/ConversationList.test.tsx`

- [ ] **Step 1: Update test fixtures with `lastSeenAt`**

In every `Conversation` fixture in `src/views/chat/ConversationList.test.tsx`, add `lastSeenAt`. Use the same value as `updatedAt` unless the test is specifically about unseen state.

Example:

```ts
{
  id: "a",
  workspaceId: null,
  title: "First one",
  createdAt: 1,
  updatedAt: 3,
  lastSeenAt: 3,
  status: "done",
}
```

- [ ] **Step 2: Add sidebar unread style tests**

Add tests:

```tsx
it("renders an inactive conversation title bold when it has unseen updates", async () => {
  vi.mocked(commands.listConversations).mockResolvedValue([
    {
      id: "unseen",
      workspaceId: null,
      title: "New output arrived",
      createdAt: 1,
      updatedAt: 10,
      lastSeenAt: 5,
      status: "done",
    },
  ]);

  render(
    <ConversationList
      activeId={null}
      onSelect={vi.fn()}
      onNewConversation={vi.fn()}
      onOpenSettings={vi.fn()}
    />,
  );

  expect(await screen.findByText("New output arrived")).toHaveClass("font-semibold");
});

it("renders the active conversation title normal even when it has unseen updates", async () => {
  vi.mocked(commands.listConversations).mockResolvedValue([
    {
      id: "active",
      workspaceId: null,
      title: "Currently open",
      createdAt: 1,
      updatedAt: 10,
      lastSeenAt: 5,
      status: "done",
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

  expect(await screen.findByText("Currently open")).toHaveClass("font-medium");
});
```

- [ ] **Step 3: Run sidebar tests and verify failure**

Run:

```bash
npm test -- src/views/chat/ConversationList.test.tsx
```

Expected: unread title tests fail because the title class is not conditional yet.

- [ ] **Step 4: Implement conditional title class**

In `src/views/chat/ConversationList.tsx`, before the returned button body in the map callback, derive:

```tsx
const isActive = c.id === activeId;
const hasUnseenUpdates = !isActive && c.updatedAt > c.lastSeenAt;
```

Change the title span class to:

```tsx
className={cn(
  "min-w-0 flex-1 truncate text-[13px] leading-4",
  hasUnseenUpdates ? "font-semibold" : "font-medium",
)}
```

Also reuse `isActive` in the row class:

```tsx
isActive ? "bg-background" : "bg-transparent border-0 shadow-none hover:bg-background/70";
```

- [ ] **Step 5: Run sidebar tests**

Run:

```bash
npm test -- src/views/chat/ConversationList.test.tsx
```

Expected: all `ConversationList` tests pass.

- [ ] **Step 6: Commit sidebar rendering**

```bash
git add src/views/chat/ConversationList.tsx src/views/chat/ConversationList.test.tsx
git commit -m "feat: bold unseen conversation titles"
```

---

### Task 5: Mark Conversations Seen on Open and While Active

**Files:**

- Modify: `src/App.tsx`
- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/App.test.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx`

- [ ] **Step 1: Add command mock and fixtures in app tests**

In `src/App.test.tsx`, add `markConversationSeen: vi.fn()` to the `commands` mock and add `lastSeenAt` to all `Conversation` fixtures.

Use `lastSeenAt: updatedAt` unless testing unseen behavior.

- [ ] **Step 2: Add test for marking selected conversation seen**

Add to `src/App.test.tsx`:

```tsx
it("marks a conversation seen when the user selects it from the sidebar", async () => {
  const conversation = {
    id: "c1",
    workspaceId: null,
    title: "Unread thread",
    createdAt: 1,
    updatedAt: 10,
    lastSeenAt: 5,
    status: "done" as const,
  };
  vi.mocked(commands.listConversations).mockResolvedValue([conversation]);

  render(<App />);

  await userEvent.click(await screen.findByText("Unread thread"));

  expect(commands.markConversationSeen).toHaveBeenCalledWith("c1");
});
```

- [ ] **Step 3: Add workspace active-refresh callback test**

In `src/views/workspace/Workspace.test.tsx`, render `Workspace` with a spy prop:

```tsx
it("notifies when active messages refresh so the app can mark the conversation seen", async () => {
  const onConversationSeen = vi.fn();
  vi.mocked(commands.listMessages).mockResolvedValue([userMessage({ id: "m1", content: "hello" })]);

  render(<Workspace conversationId="c1" onConversationSeen={onConversationSeen} />);

  await waitFor(() => expect(onConversationSeen).toHaveBeenCalledWith("c1"));
});
```

Use the existing message fixture helpers in `Workspace.test.tsx` instead of introducing a second helper if one already exists.

- [ ] **Step 4: Run app/workspace tests and verify failure**

Run:

```bash
npm test -- src/App.test.tsx src/views/workspace/Workspace.test.tsx
```

Expected: tests fail because `markConversationSeen` and `onConversationSeen` wiring is missing.

- [ ] **Step 5: Extend `WorkspaceProps`**

In `src/views/workspace/Workspace.tsx`:

```ts
interface WorkspaceProps {
  conversationId: string;
  pendingInitialTurn?: PendingInitialTurn | null;
  onPendingInitialTurnConsumed?: (conversationId: string) => void;
  onConversationSeen?: (conversationId: string) => void;
}
```

Destructure:

```ts
onConversationSeen,
```

- [ ] **Step 6: Notify after message loads and refreshes**

In the initial `commands.listMessages(conversationId).then(...)` success path, after `setMessages(...)`, call:

```ts
onConversationSeen?.(conversationId);
```

In the `agent-message-persisted` refresh success path, after `setMessages(loadedMessages)`, call:

```ts
onConversationSeen?.(conversationId);
```

Include `onConversationSeen` in the relevant effect dependency arrays.

- [ ] **Step 7: Add mark-seen helper in App**

In `src/App.tsx`, add:

```ts
const markSeen = (conversationId: string) => {
  commands.markConversationSeen(conversationId).catch(console.error);
  setActiveConversation((current) =>
    current?.id === conversationId
      ? { ...current, lastSeenAt: Math.max(Date.now(), current.updatedAt) }
      : current,
  );
};
```

- [ ] **Step 8: Mark seen on conversation select**

In `onSelect`:

```tsx
onSelect={(conversation) => {
  setShowSettings(false);
  setPendingInitialTurn(null);
  setActiveConversation({
    ...conversation,
    lastSeenAt: Math.max(Date.now(), conversation.updatedAt),
  });
  commands.markConversationSeen(conversation.id).catch(console.error);
}}
```

If `markSeen` can be reused without stale state, prefer:

```tsx
setActiveConversation(conversation);
markSeen(conversation.id);
```

but ensure the optimistic active conversation state is normalized immediately.

- [ ] **Step 9: Pass active watcher to Workspace**

In the `Workspace` render:

```tsx
<Workspace
  key={activeConversation.id}
  conversationId={activeConversation.id}
  pendingInitialTurn={
    pendingInitialTurn?.conversationId === activeConversation.id ? pendingInitialTurn : null
  }
  onPendingInitialTurnConsumed={(conversationId) =>
    setPendingInitialTurn((prev) => (prev?.conversationId === conversationId ? null : prev))
  }
  onConversationSeen={markSeen}
/>
```

- [ ] **Step 10: Run app/workspace tests**

Run:

```bash
npm test -- src/App.test.tsx src/views/workspace/Workspace.test.tsx
```

Expected: selected tests pass.

- [ ] **Step 11: Commit open/active seen behavior**

```bash
git add src/App.tsx src/views/workspace/Workspace.tsx src/App.test.tsx src/views/workspace/Workspace.test.tsx
git commit -m "feat: mark active conversations seen"
```

---

### Task 6: Final Verification

**Files:**

- All files changed above.

- [ ] **Step 1: Run frontend tests**

```bash
npm test -- src/views/chat/ConversationList.test.tsx src/App.test.tsx src/views/workspace/Workspace.test.tsx
```

Expected: all selected frontend tests pass.

- [ ] **Step 2: Run Rust tests**

```bash
cd src-tauri
cargo test storage::migrations::tests::conversation_last_seen_at commands::conversations::tests::mark_conversation_seen_in_conn
```

Expected: all selected Rust tests pass.

- [ ] **Step 3: Run format/lint/build**

```bash
cd src-tauri
cargo fmt --check
cd ..
./node_modules/.bin/oxfmt --check src/App.tsx src/App.test.tsx src/components/MessageContent.tsx src/views/chat/ConversationList.tsx src/views/chat/ConversationList.test.tsx src/views/workspace/Workspace.tsx src/views/workspace/Workspace.test.tsx src/views/chat/rich-input/RichInput.tsx src/lib/ipc.ts
npm run lint
npm run build
```

Expected:

- `cargo fmt --check` exits 0, or reports only unrelated pre-existing formatting drift. If it reports touched files, format those touched files.
- `oxfmt --check` exits 0 for touched frontend files.
- `npm run lint` exits 0.
- `npm run build` exits 0.

- [ ] **Step 4: Inspect git status**

```bash
git status --short
```

Expected: only intended files are modified. Existing unrelated dirty files may remain; do not revert them.

- [ ] **Step 5: Final commit if needed**

If any verification-only formatting changes were made:

```bash
git add <formatted touched files>
git commit -m "style: format conversation seen state"
```

---

## Self-Review

Spec coverage:

- Persisted `last_seen_at`: Task 1 and Task 2.
- Existing rows backfilled from `updated_at`: Task 1.
- New conversations start seen: Task 2.
- Backend command and list exposure: Task 2.
- Frontend types: Task 3.
- Sidebar bold predicate: Task 4.
- Active conversation remains normal while streaming: Task 5.
- Best-effort mark failures: Task 5 uses `.catch(console.error)`.
- Tests: Tasks 1, 2, 4, 5, 6.

Placeholder scan:

- The plan uses concrete file paths, command names, test names, and code snippets throughout.

Type consistency:

- Rust field: `last_seen_at`.
- Frontend field: `lastSeenAt`.
- Command: `mark_conversation_seen` / `markConversationSeen`.
