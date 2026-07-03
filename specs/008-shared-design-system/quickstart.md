# Quickstart: Shared Design System — Button

Validation guide for the Button component and its first migration
slice. Assumes `Button` is implemented at `src/components/ui/button.tsx`
per [contracts/button.md](./contracts/button.md) and [data-model.md](./data-model.md).

## Prerequisites

```sh
npm install   # pulls in @radix-ui/react-slot (new dependency, see research.md)
```

## 1. Component-level validation (Vitest + Testing Library)

```sh
npm test -- src/components/ui/button.test.tsx
```

Expected coverage, mapping directly to spec requirements:

- **FR-002/FR-003**: renders with `cursor-pointer` classes when enabled;
  renders with `disabled` and no pointer/hover classes when disabled;
  `onClick` does not fire when disabled.
- **FR-004/FR-006**: focusing via `userEvent.tab()` shows a focus-visible
  style; pressing Enter/Space while focused triggers `onClick`.
- **FR-005**: default role is the native `button` role (no extra ARIA
  needed); `asChild` case exposes the same activation behavior on the
  substituted element.
- Variant/size classes render as expected for each `variant`/`size`
  combination (data-model.md table).

## 2. Visual/manual validation

```sh
npm run dev
```

- Open the app, hover over a `Button` instance in each migrated view
  (see audit table below) — cursor should be a pointer, a hover style
  should appear.
- Tab to each button with the keyboard — a visible focus ring should
  appear.
- Toggle a disabled button (e.g. Settings' "Add" MCP server button
  before filling required fields) — no pointer cursor, no hover style.
- Toggle the app's dark theme — verify all states remain visually
  distinguishable in both themes.

## 3. Migration audit (User Story 3, Button slice)

Hand-rolled `<button>` sites found via `grep -rn "<button" src --include="*.tsx"`
(excluding test files), to be migrated to `<Button>` in this pass:

| File | Line | Current purpose | Target variant |
|---|---|---|---|
| `src/App.tsx` | 60 | (inspect at implementation time) | TBD |
| `src/views/chat/ConversationList.tsx` | 69, 76, 84, 95 | new conversation / settings / conversation row actions | TBD |
| `src/views/settings/Settings.tsx` | 55, 84, 100 | close settings, add MCP server, test connection | TBD (55/100 look like `ghost`/text-style, 84 likely `primary`) |
| `src/views/chat/SearchPanel.tsx` | 56, 62 | close search, result row | TBD |
| `src/views/chat/Chat.tsx` | 209, 235 | (inspect at implementation time) | TBD |
| `src/views/workspace/Workspace.tsx` | 73, 140 | (inspect at implementation time) | TBD |

Exact `variant`/`size` mapping is decided per-site during
implementation (tasks.md), matching each button's current visual
appearance so migration stays behavior/appearance-preserving (FR-010).

## 4. Regression check (SC-004)

```sh
npm test
npm run test:e2e
```

Expected: all previously passing unit and e2e tests continue to pass;
no test required a `data-testid` to move or be renamed.
