# Shared Topbar Design

## Summary

Add a shared, fixed-height topbar system for the sidebar and main chat region.
The topbar provides a consistent transparent drag affordance across the app while
allowing each pane to render its own optional content.

The selected direction is a hybrid layout: sidebar and chat share the same
height and drag behavior, but remain visually segmented by the existing sidebar
divider. The chat topbar shows active conversation metadata; the empty state
keeps the drag affordance but renders no visible content.

## Goals

- Provide a draggable top area across both sidebar and chat regions.
- Keep the topbar visually transparent and lightweight.
- Move active conversation identity into the chat topbar:
  - conversation title
  - workspace path
  - context usage gauge
- Keep the empty state topbar visually blank.
- Avoid prop drilling by letting active views portal content into shell-owned
  topbar hosts.
- Keep sidebar actions below the shared topbar so they never overlap macOS
  traffic-light controls.

## Non-Goals

- Redesign the sidebar action buttons.
- Add settings or widget-gallery topbar titles in v1.
- Make the context usage gauge interactive.
- Replace native macOS traffic-light controls.
- Dynamically measure native titlebar or traffic-light height.

## Layout

The app shell owns the topbar hosts:

```tsx
<TopbarProvider>
  <div className="flex h-dvh">
    <div className="flex w-64 flex-col bg-sidebar">
      <TopbarHost target="sidebar" />
      <ConversationListBody />
    </div>

    <div className="flex min-w-0 flex-1 flex-col">
      <TopbarHost target="main" />
      <MainContent />
    </div>
  </div>
</TopbarProvider>
```

Both hosts use one shared height token, initially matching the current sidebar
affordance:

```css
--app-topbar-height: 40px;
```

Implementation can use Tailwind's `h-10` directly at first if that is more
consistent with the codebase, but the design intent is that sidebar and main
topbar heights are controlled from one place.

## Drag Behavior

`TopbarHost` owns the drag behavior for both panes:

- Adds `data-tauri-drag-region`.
- Handles primary-button `mousedown`.
- Calls `getCurrentWindow().startDragging()`.
- Prevents default selection behavior when drag starts.
- Logs drag failures with the same style as the current sidebar affordance.

Drag behavior is only attached to topbar hosts, not to the full chat scroll
surface. This avoids conflicts with text selection, scrolling, widgets, and
composer focus.

Interactive children inside a topbar should not trigger window dragging. For v1,
the main topbar content is non-interactive except the context gauge, which is
currently treated as display-only. If a topbar child becomes interactive later,
that child should stop propagation or be rendered outside the drag-start target.

## Portal Slot System

Create a small topbar slot system in `src/components/Topbar.tsx`. If the
component grows beyond the provider, host, and portal primitives during
implementation, split it into `src/components/topbar/` as a follow-up cleanup.

Public API:

```tsx
<TopbarProvider>
  ...
</TopbarProvider>

<TopbarHost target="sidebar" />
<TopbarHost target="main" />

<TopbarPortal target="main">
  ...
</TopbarPortal>
```

Responsibilities:

- `TopbarProvider` stores host DOM nodes for known targets.
- `TopbarHost` renders the fixed-height transparent drag container and registers
  its element with the provider.
- `TopbarPortal` uses `createPortal(children, host)` once the matching host is
  available.
- When the owner view unmounts, React naturally removes the portal content.
- Hosts remain mounted across app states, so the layout never jumps.

Known targets for v1:

- `sidebar`
- `main`

Only `main` needs dynamic content in v1. `sidebar` exists primarily to replace
the current anonymous spacer with the same shared topbar component.

## Chat Topbar Content

`Workspace` owns active chat topbar content and portals it into the main host.

Content layout:

```tsx
<TopbarPortal target="main">
  <div className="flex min-w-0 items-center justify-between gap-3">
    <div className="min-w-0">
      <div className="truncate text-sm font-medium">
        {conversation.title}
      </div>
      <div className="truncate text-xs text-muted-foreground">
        {workspacePathLabel}
      </div>
    </div>

    <ContextUsageGauge conversationId={conversation.id} />
  </div>
</TopbarPortal>
```

Rules:

- Empty state renders no `TopbarPortal` content, leaving the main topbar blank.
- Settings renders no main topbar content in v1.
- Widget gallery renders no main topbar content in v1.
- The workspace path uses unix-style home compaction, for example
  `/Users/gimenes/code/doce` becomes `~/code/doce`.
- If there is no workspace path available, render only the title and context
  gauge.

## Data Boundaries

`App` already owns the active `Conversation`, while `Workspace` currently only
receives `conversationId`. To render topbar metadata without duplicating active
conversation lookups, pass the full active conversation into `Workspace`:

```tsx
<Workspace conversation={activeConversation} ... />
```

`Workspace` then uses:

- `conversation.id` for messages and context gauge.
- `conversation.title` for the topbar title.
- `conversation.workspaceId` to derive the workspace path label.

Workspace path resolution should follow existing codebase patterns. If a shared
workspace hook already exists, use it. If not, prefer the smallest scoped helper
that avoids duplicating complex sidebar behavior. It is acceptable for v1 to
fetch the workspace list in the topbar subcomponent if that keeps ownership
clear and avoids changing broader state management.

## Composer Change

Move context usage display out of the composer by stopping this call:

```tsx
<RichInput contextGauge={<ContextUsageGauge conversationId={conversationId} />} />
```

Instead, render `ContextUsageGauge` in the main topbar. Keep the `contextGauge`
prop on `RichInput` for now unless removing it is trivial and low-risk. This
keeps the change focused on layout and ownership rather than broad input
cleanup.

## Empty State Behavior

The empty state should not render visible topbar content.

The main `TopbarHost` still renders its fixed-height transparent container so:

- the top drag affordance remains available;
- the chat surface starts below the shared topbar height;
- switching from empty state to active chat does not shift the shell layout.

## Testing

Add or update tests for:

- Sidebar and main topbar hosts render with the shared fixed height.
- Sidebar actions render below the topbar host.
- Empty state leaves the main topbar content empty.
- Active `Workspace` portals title, workspace path, and context gauge into the
  main topbar.
- Context usage no longer appears inside the composer when an active chat is
  open.
- Drag host markup exists on both sidebar and main topbars.

Existing chat/sidebar behavior should remain covered by current tests.

## Risks And Mitigations

- **Stale topbar content:** Portal ownership should naturally unmount content
  when `Workspace` unmounts. Tests should switch from active chat to empty state
  and assert the main topbar is blank.
- **Dragging conflicts with content:** Keep drag handling on the host and keep
  v1 topbar content display-only. Add event stopping only when interactive
  controls are introduced.
- **Layout drift between panes:** Use the same `TopbarHost` component for both
  sidebar and main panes.
- **Workspace data duplication:** Prefer reusing existing workspace helpers.
  If none exist, introduce the smallest focused helper for formatting and lookup.
