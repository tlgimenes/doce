# Shadcn Base UI Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the doce Tauri app UI to latest shadcn with Base UI, install all shadcn components, apply the Brand Accent Workbench theme, remove Radix, and redesign the app shell, chat, search, settings, and command center without changing backend behavior.

**Architecture:** Keep the current two-pane app shell and Tauri IPC contracts. Put shadcn-generated primitives in `src/components/ui`, use app-owned wrappers for command/search/chat domain behavior, and keep `App.tsx` as the surface-state owner. Preserve current scroll behavior with `use-stick-to-bottom` for this pass while adopting shadcn chat message, bubble, marker, and attachment primitives for transcript rendering.

**Tech Stack:** React 19, Vite 8, Tauri 2, TypeScript 6, Tailwind CSS 4, shadcn latest with Base UI, Vitest, Testing Library, Tiptap 3, zustand, `use-stick-to-bottom`.

## Global Constraints

- App-only redesign. Do not redesign `site/`.
- Use shadcn's latest Base UI component path.
- Run shadcn's CLI with `add --all` / `-a` so all available shadcn components are installed up front.
- Remove Radix imports and package dependencies.
- Keep the existing two-pane shell: sidebar, main chat workspace, topbar, empty composer, settings, search, onboarding, shortcuts, and widget gallery.
- Add `Cmd+K` as a universal action command center.
- Keep dedicated conversation search opened by `Cmd+F` and sidebar Search.
- Keep backend commands, storage, model behavior, and Tauri IPC contracts unchanged.
- Keep Tiptap in `RichInput`.
- Keep tool widget parsing and fallback behavior.
- Use Brand Accent Workbench theme: warm neutral surfaces, chocolate primary/action/focus, caramel progress/live status, peach/coral attention accents, cream/warm-white elevated surfaces, compact spacing, 8px-or-less radii.
- Use lucide icons for redesigned button/control icons when a matching icon exists.
- Required final verification: `npm run build`, `npm test`, `npm run lint`, and `rg "@radix-ui|radix-ui" src package.json package-lock.json` with no matches.

---

## File Structure

### Generated And Theme Layer

- `components.json`: shadcn configuration. Must specify Base UI and existing aliases.
- `src/styles/theme.css`: Tailwind v4 import, shadcn token bridge, Brand Accent Workbench colors, existing view-transition animations, and global focus/reduced-motion rules.
- `src/components/ui/*`: shadcn-generated primitives from `shadcn add --all`. Generated files may be lightly adjusted for doce conventions, but domain behavior stays outside this directory.
- `src/components/ui/button.tsx`: app-facing button primitive. Preserve current `variant` and `size` names used across the app while removing Radix.
- `src/components/ui/KeyboardShortcut.tsx`: keep as a small app-owned display primitive unless shadcn `Kbd` can replace it without test churn.

### Shell And Global Surfaces

- `src/App.tsx`: owns ready flow, active conversation, settings/search/command/shortcuts/widget-gallery visibility, and global shortcuts.
- `src/lib/shortcuts.ts`: registry for real global shortcuts. `Cmd+K` opens the command center. Shortcuts dialog is opened by command center action.
- `src/views/command/CommandCenter.tsx`: new universal action palette.
- `src/views/command/CommandCenter.test.tsx`: command center action tests.
- `src/views/chat/ConversationSearchDialog.tsx`: new dialog wrapper around conversation search.
- `src/views/chat/SearchPanel.tsx`: search body with recent conversations, highlighted snippets, loading, no-results, and error states.
- `src/views/chat/ConversationList.tsx`: sidebar actions and conversation rows using shadcn sidebar/item patterns. Calls parent-owned search/settings/new handlers.
- `src/components/Dialog.tsx`: compatibility wrapper over the generated shadcn `Dialog` primitive, preserving the current `{ open, onClose, children }` app-facing props.
- `src/views/shortcuts/ShortcutsDialog.tsx`: restyled dialog that still renders from `Shortcut[]`.

### App Surfaces

- `src/views/settings/Settings.tsx`: tabbed MCP Servers and Skills surface using shadcn fields, tabs, item rows, badges, and inline errors.
- `src/views/onboarding/Onboarding.tsx`: restyled only. Model install logic stays unchanged.
- `src/views/design-system/WidgetGallery.tsx`: previews new theme, primitives, chat widgets, command center, settings rows.

### Chat

- `src/components/MessageContent.tsx`: dispatches user, assistant, context notice, error, and tool result rows into shadcn chat-style wrappers.
- `src/components/UserMessageBubble.tsx`: restyled user bubble, preserves rich text rendering and token meter.
- `src/views/workspace/TranscriptTurn.tsx`: keeps turn grouping and sticky user message behavior, uses new message wrappers.
- `src/views/workspace/Workspace.tsx`: keeps `use-stick-to-bottom`; wires redesigned transcript/composer surfaces.
- `src/views/workspace/StreamingStatus.tsx`, `PlanTracker.tsx`, `WorkspaceTopbar.tsx`, `StickyUserMessage.tsx`: restyled to the new theme.
- `src/views/chat/rich-input/RichInput.tsx`: keep Tiptap, native attachment handling, skill mentions, paste collapse, submit semantics; restyle controls with shadcn primitives.
- `src/views/chat/rich-input/extensions/attachment-node.tsx`: token-align attachment chips with transcript attachments.
- `src/views/chat/rich-input/UserMessageContent.tsx`: render persisted attachments with matching attachment token styling.
- `src/views/chat/tool-widgets/*.tsx`: restyle using shadcn disclosure/badge/separator/button/scroll area primitives while preserving parser-driven behavior.

### Verification Files

- `src/components/ui/button.test.tsx`: update for no-Radix button behavior.
- `src/lib/shortcuts.test.ts`: new shortcut registry tests.
- `src/App.test.tsx`: update global routing and shortcut tests.
- Existing tests under `src/views/**`: update assertions to behavior/test ids instead of exact old class strings where needed.

---

## Task 1: Bootstrap Shadcn Base UI And Brand Theme

**Files:**
- Create: `components.json`
- Create/Modify: `src/components/ui/*`
- Modify: `package.json`
- Modify: `package-lock.json`
- Modify: `src/styles/theme.css`
- Modify: `src/components/ui/button.tsx`
- Modify: `src/components/Dialog.tsx`
- Test: `src/components/ui/button.test.tsx`
- Test: `src/components/Dialog.test.tsx`

**Interfaces:**
- Consumes: existing Vite alias `@ -> ./src`, existing Tailwind v4 Vite plugin, existing `cn()` helper from `src/lib/cn.ts`.
- Produces: shadcn Base UI component files under `src/components/ui`, Brand Accent Workbench CSS tokens, and a Radix-free `Button` API with:
  - `ButtonVariant = "primary" | "secondary" | "destructive" | "ghost"`
  - `ButtonSize = "sm" | "md" | "icon" | "icon-sm"`
  - `buttonVariants({ variant, size }): string`
  - `<Button variant="primary" size="md" disabled={false} className="mt-2" onClick={handler}>Label</Button>`

- [ ] **Step 1: Confirm the working tree before dependency generation**

Run:

```bash
git status --short
```

Expected: existing unrelated dirty files may be present, but no frontend files from this task should be dirty yet. Do not stage or revert unrelated files.

- [ ] **Step 2: Run shadcn init with Base UI**

Run:

```bash
npx shadcn@latest init -t vite -b base --css-variables --yes
```

Expected: `components.json` is created or updated, package manifests change, and shadcn reports a Base UI configuration. If the CLI asks about overwriting `src/styles/theme.css`, accept and immediately restore the existing view-transition/reduced-motion sections in Step 5.

- [ ] **Step 3: Install all shadcn components**

Run:

```bash
npx shadcn@latest add --all --overwrite --yes
```

Expected: files are added under `src/components/ui`, package manifests change, and no files under `site/` change.

If `lucide-react` is not added by the CLI, run:

```bash
npm install lucide-react
```

Expected: `package.json` and `package-lock.json` include `lucide-react`.

- [ ] **Step 4: Inspect generated config for Base UI and aliases**

Open `components.json` and ensure it uses the existing alias shape:

```json
{
  "aliases": {
    "components": "@/components",
    "utils": "@/lib/utils",
    "ui": "@/components/ui",
    "lib": "@/lib"
  }
}
```

If the CLI creates `src/lib/utils.ts`, make it delegate to the existing helper rather than duplicating logic:

```ts
export { cn } from "./cn";
```

- [ ] **Step 5: Apply the Brand Accent Workbench theme**

In `src/styles/theme.css`, keep the existing view-transition keyframes and reduced-motion block. Replace the token section at the top with:

```css
@import "tailwindcss";
@plugin "@tailwindcss/typography";

@custom-variant dark (&:where(.dark, .dark *));

@theme {
  --color-doce-chocolate: oklch(48% 0.085 55);
  --color-doce-cacao: oklch(29% 0.055 49);
  --color-doce-caramel: oklch(82% 0.135 80);
  --color-doce-peach: oklch(82% 0.078 43);
  --color-doce-coral: oklch(73% 0.105 31);
  --color-doce-cream: oklch(94% 0.036 70);
  --color-doce-warm-white: oklch(98% 0.015 75);

  --color-background: var(--color-doce-warm-white);
  --color-foreground: oklch(23% 0.028 48);
  --color-card: oklch(100% 0 0);
  --color-card-foreground: var(--color-foreground);
  --color-popover: oklch(100% 0 0);
  --color-popover-foreground: var(--color-foreground);
  --color-primary: var(--color-doce-chocolate);
  --color-primary-foreground: oklch(98% 0.012 75);
  --color-secondary: oklch(94% 0.026 70);
  --color-secondary-foreground: oklch(30% 0.04 50);
  --color-muted: oklch(94% 0.026 70);
  --color-muted-foreground: oklch(52% 0.032 50);
  --color-accent: oklch(91% 0.046 65);
  --color-accent-foreground: oklch(28% 0.042 48);
  --color-destructive: oklch(58% 0.18 28);
  --color-destructive-foreground: oklch(98% 0.012 75);
  --color-border: oklch(87% 0.025 68);
  --color-input: oklch(89% 0.023 68);
  --color-ring: var(--color-doce-chocolate);
  --color-sidebar: oklch(95% 0.026 70);
  --color-sidebar-foreground: var(--color-foreground);
  --color-sidebar-primary: var(--color-doce-chocolate);
  --color-sidebar-primary-foreground: var(--color-primary-foreground);
  --color-sidebar-accent: oklch(91% 0.038 69);
  --color-sidebar-accent-foreground: var(--color-foreground);
  --color-sidebar-border: oklch(86% 0.026 68);
  --color-sidebar-ring: var(--color-doce-chocolate);

  --radius: 0.5rem;
}

.dark {
  --color-background: oklch(18% 0.026 48);
  --color-foreground: oklch(94% 0.025 72);
  --color-card: oklch(22% 0.03 48);
  --color-card-foreground: var(--color-foreground);
  --color-popover: oklch(22% 0.03 48);
  --color-popover-foreground: var(--color-foreground);
  --color-primary: var(--color-doce-caramel);
  --color-primary-foreground: oklch(20% 0.03 48);
  --color-secondary: oklch(28% 0.035 48);
  --color-secondary-foreground: var(--color-foreground);
  --color-muted: oklch(28% 0.035 48);
  --color-muted-foreground: oklch(72% 0.034 67);
  --color-accent: oklch(32% 0.045 50);
  --color-accent-foreground: var(--color-foreground);
  --color-destructive: oklch(70% 0.17 29);
  --color-destructive-foreground: oklch(16% 0.03 48);
  --color-border: oklch(34% 0.035 48);
  --color-input: oklch(34% 0.035 48);
  --color-ring: var(--color-doce-caramel);
  --color-sidebar: oklch(20% 0.03 48);
  --color-sidebar-foreground: var(--color-foreground);
  --color-sidebar-primary: var(--color-doce-caramel);
  --color-sidebar-primary-foreground: oklch(18% 0.026 48);
  --color-sidebar-accent: oklch(28% 0.035 48);
  --color-sidebar-accent-foreground: var(--color-foreground);
  --color-sidebar-border: oklch(32% 0.035 48);
  --color-sidebar-ring: var(--color-doce-caramel);
}

body {
  background-color: var(--color-background);
  color: var(--color-foreground);
}

:focus-visible {
  outline: 2px solid var(--color-ring);
  outline-offset: 2px;
}
```

- [ ] **Step 6: Make `Button` Radix-free and app-compatible**

Update `src/components/ui/button.tsx` so it does not import `@radix-ui/react-slot`, preserves the app's variants and sizes, and uses a native `button` root:

```tsx
import { forwardRef } from "react";
import { cn } from "@/lib/cn";

export type ButtonVariant = "primary" | "secondary" | "destructive" | "ghost";
export type ButtonSize = "sm" | "md" | "icon" | "icon-sm";

export interface ButtonProps extends React.ComponentPropsWithoutRef<"button"> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

const variantClasses: Record<ButtonVariant, string> = {
  primary: "bg-primary text-primary-foreground hover:bg-primary/90",
  secondary: "border border-border bg-card text-foreground hover:bg-accent",
  destructive: "bg-destructive text-destructive-foreground hover:bg-destructive/90",
  ghost: "bg-transparent text-foreground hover:bg-accent",
};

const sizeClasses: Record<ButtonSize, string> = {
  sm: "h-8 px-3 text-sm",
  md: "h-9 px-4 text-sm",
  icon: "size-8 p-0",
  "icon-sm": "size-6 p-0",
};

export function buttonVariants({
  variant = "primary",
  size = "md",
}: { variant?: ButtonVariant; size?: ButtonSize } = {}) {
  return cn(
    "inline-flex items-center justify-center gap-2 rounded-md font-medium transition-colors",
    "cursor-pointer disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50",
    "aria-disabled:pointer-events-none aria-disabled:cursor-not-allowed aria-disabled:opacity-50",
    variantClasses[variant],
    sizeClasses[size],
  );
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "primary", size = "md", disabled, onClick, ...props }, ref) => (
    <button
      ref={ref}
      className={cn(buttonVariants({ variant, size }), className)}
      disabled={disabled}
      aria-disabled={disabled || undefined}
      onClick={disabled ? undefined : onClick}
      {...props}
    />
  ),
);
Button.displayName = "Button";
```

- [ ] **Step 7: Update button tests**

In `src/components/ui/button.test.tsx`, remove the two `asChild` tests and add this Radix guard:

```tsx
import { readFileSync } from "node:fs";

it("does not import Radix Slot", () => {
  const source = readFileSync(new URL("./button.tsx", import.meta.url), "utf8");
  expect(source).not.toContain("@radix-ui/react-slot");
  expect(source).not.toContain("Slot");
});
```

- [ ] **Step 8: Replace local native dialog with a shadcn compatibility wrapper**

Update `src/components/Dialog.tsx` to preserve the existing app-facing props while delegating to the generated shadcn dialog:

```tsx
import { type ReactNode } from "react";
import {
  Dialog as DialogRoot,
  DialogContent,
} from "@/components/ui/dialog";

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
}

export default function Dialog({ open, onClose, children }: DialogProps) {
  return (
    <DialogRoot
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onClose();
      }}
    >
      <DialogContent
        className="border-border bg-popover text-popover-foreground"
        data-testid="app-dialog-content"
      >
        {children}
      </DialogContent>
    </DialogRoot>
  );
}
```

If the generated shadcn dialog exports different names, adapt `src/components/Dialog.tsx` to the generated names while keeping `DialogProps`, `data-testid="app-dialog-content"`, and close-on-`onOpenChange(false)` semantics.

- [ ] **Step 9: Update dialog tests**

In `src/components/Dialog.test.tsx`, replace native `<dialog>` assertions with role/content assertions:

```tsx
it("renders dialog content only while open", () => {
  const { rerender } = render(
    <Dialog open={true} onClose={vi.fn()}>
      <p>Hello</p>
    </Dialog>,
  );

  expect(screen.getByTestId("app-dialog-content")).toBeInTheDocument();
  expect(screen.getByText("Hello")).toBeInTheDocument();

  rerender(
    <Dialog open={false} onClose={vi.fn()}>
      <p>Hello</p>
    </Dialog>,
  );

  expect(screen.queryByText("Hello")).not.toBeInTheDocument();
});

it("calls onClose when Escape closes the dialog", async () => {
  const onClose = vi.fn();
  render(
    <Dialog open={true} onClose={onClose}>
      <p>Hello</p>
    </Dialog>,
  );

  await userEvent.keyboard("{Escape}");

  expect(onClose).toHaveBeenCalledTimes(1);
});
```

- [ ] **Step 10: Run task verification**

Run:

```bash
npm test -- src/components/ui/button.test.tsx src/components/Dialog.test.tsx
npm run build
```

Expected: button tests pass and the build passes. If generated shadcn components create lint-only unused exports, leave them; this task verifies TypeScript build, not lint.

- [ ] **Step 11: Commit**

```bash
git add components.json package.json package-lock.json src/components/ui src/components/Dialog.tsx src/components/Dialog.test.tsx src/lib/utils.ts src/styles/theme.css
git commit -m "feat(ui): bootstrap shadcn base ui"
```

If `src/lib/utils.ts` was not created, omit it from `git add`.

---

## Task 2: Global Shortcuts And App-Owned Surface State

**Files:**
- Modify: `src/lib/shortcuts.ts`
- Create: `src/lib/shortcuts.test.ts`
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`
- Modify: `src/views/chat/ConversationList.tsx`

**Interfaces:**
- Consumes: `Button` from Task 1.
- Produces:
  - `ShortcutHandlers.openCommandCenter(): void`
  - `buildShortcuts(handlers): Shortcut[]` where `Cmd+K` has id `open-command-center`
  - App-owned booleans: `showSearch`, `showCommandCenter`, `showShortcutsDialog`, `showSettings`, `showWidgetGallery`
  - `ConversationListProps.onOpenSearch: () => void`

- [ ] **Step 1: Add failing shortcut registry tests**

Create `src/lib/shortcuts.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import { buildShortcuts } from "./shortcuts";

function handlers() {
  return {
    focusInput: vi.fn(),
    newConversation: vi.fn(),
    openSearch: vi.fn(),
    openCommandCenter: vi.fn(),
    toggleWidgetGallery: vi.fn(),
  };
}

describe("buildShortcuts", () => {
  it("binds Cmd+K to the command center", () => {
    const h = handlers();
    const shortcut = buildShortcuts(h).find((s) => s.id === "open-command-center");

    expect(shortcut).toMatchObject({
      combo: "Cmd+K",
      metaKey: true,
      key: "k",
      description: "Open command center",
    });

    shortcut?.action();
    expect(h.openCommandCenter).toHaveBeenCalledTimes(1);
  });

  it("keeps Cmd+F dedicated to conversation search", () => {
    const h = handlers();
    const shortcut = buildShortcuts(h).find((s) => s.id === "search-conversations");

    expect(shortcut).toMatchObject({
      combo: "Cmd+F",
      metaKey: true,
      key: "f",
      description: "Search conversations",
    });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm test -- src/lib/shortcuts.test.ts
```

Expected: fail because `openCommandCenter` is not part of `ShortcutHandlers` and `open-command-center` does not exist.

- [ ] **Step 3: Update shortcut interfaces**

Change `src/lib/shortcuts.ts` so `ShortcutHandlers` is:

```ts
export interface ShortcutHandlers {
  focusInput: () => void;
  newConversation: () => void;
  openSearch: () => void;
  openCommandCenter: () => void;
  toggleWidgetGallery: () => void;
}
```

Change the shortcut entries to:

```ts
{
  id: "focus-input",
  combo: "Cmd+L",
  metaKey: true,
  key: "l",
  description: "Focus composer",
  action: handlers.focusInput,
},
{
  id: "new-conversation",
  combo: "Cmd+N",
  metaKey: true,
  key: "n",
  description: "New Agent",
  action: handlers.newConversation,
},
{
  id: "search-conversations",
  combo: "Cmd+F",
  metaKey: true,
  key: "f",
  description: "Search conversations",
  action: handlers.openSearch,
},
{
  id: "open-command-center",
  combo: "Cmd+K",
  metaKey: true,
  key: "k",
  description: "Open command center",
  action: handlers.openCommandCenter,
},
{
  id: "show-widget-gallery",
  combo: "Cmd+D",
  metaKey: true,
  key: "d",
  description: "Open widget gallery",
  action: handlers.toggleWidgetGallery,
},
```

- [ ] **Step 4: Move search state ownership into `App`**

In `src/App.tsx`, add state:

```ts
const [showSearch, setShowSearch] = useState(false);
const [showCommandCenter, setShowCommandCenter] = useState(false);
```

Change `buildShortcuts` handlers:

```ts
openSearch: () => setShowSearch(true),
openCommandCenter: () => setShowCommandCenter(true),
```

Keep `toggleWidgetGallery` as the existing toggle.

- [ ] **Step 5: Keep shortcut handling modal-safe**

In the `onKeyDown` handler in `src/App.tsx`, replace the shortcuts-dialog-only gate with:

```ts
if (showShortcutsDialog && match.id !== "open-command-center") return;
if (showCommandCenter && match.id !== "open-command-center") return;
```

Then keep `e.preventDefault(); match.action();`.

- [ ] **Step 6: Update `ConversationList` props**

In `src/views/chat/ConversationList.tsx`, add:

```ts
onOpenSearch: () => void;
```

Remove local `searching` state. Change:

```ts
const openSearch = () => {
  onOpenSearch();
};
```

Remove the `<Dialog>` and `<SearchPanel>` rendering at the bottom of `ConversationList`.

- [ ] **Step 7: Wire `ConversationList` from `App`**

In `src/App.tsx`, pass:

```tsx
onOpenSearch={() => setShowSearch(true)}
```

to `ConversationList`.

- [ ] **Step 8: Add failing app-level shortcut expectations**

In `src/App.test.tsx`, add this test near existing shortcut tests:

```tsx
it("opens command center with Cmd+K and keeps Cmd+F for conversation search", async () => {
  render(<App />);
  await waitForReady();

  pressCmd("k");
  expect(await screen.findByTestId("command-center")).toBeInTheDocument();

  pressCmd("f");
  expect(screen.queryByTestId("search-panel")).not.toBeInTheDocument();

  await userEvent.keyboard("{Escape}");
  pressCmd("f");
  expect(await screen.findByTestId("search-panel")).toBeInTheDocument();
});
```

This fails until Task 3 creates `ConversationSearchDialog` and Task 4 creates `CommandCenter`. Keep the exact test body from Step 8, but change the declaration prefix from `it(` to `it.skip(` until Task 4 unskips it.

- [ ] **Step 9: Run task verification**

Run:

```bash
npm test -- src/lib/shortcuts.test.ts src/views/chat/ConversationList.test.tsx
```

Expected: shortcut tests pass. Conversation list tests may need expected prop updates, but should pass after adding `onOpenSearch={vi.fn()}` wherever `ConversationList` is rendered in tests.

- [ ] **Step 10: Commit**

```bash
git add src/lib/shortcuts.ts src/lib/shortcuts.test.ts src/App.tsx src/App.test.tsx src/views/chat/ConversationList.tsx src/views/chat/ConversationList.test.tsx
git commit -m "feat(app): move global surface shortcuts into app shell"
```

---

## Task 3: Dedicated Conversation Search Dialog

**Files:**
- Create: `src/views/chat/ConversationSearchDialog.tsx`
- Create: `src/views/chat/ConversationSearchDialog.test.tsx`
- Modify: `src/views/chat/SearchPanel.tsx`
- Modify: `src/views/chat/SearchPanel.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`

**Interfaces:**
- Consumes: `showSearch` state from Task 2, `Conversation[]`, `commands.searchConversations`.
- Produces:
  - `<ConversationSearchDialog open onOpenChange recentConversations onSelectConversationId />`
  - `SearchPanel` loading and error states
  - `data-testid="conversation-search-dialog"`

- [ ] **Step 1: Add failing dialog wrapper test**

Create `src/views/chat/ConversationSearchDialog.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import ConversationSearchDialog from "./ConversationSearchDialog";

describe("ConversationSearchDialog", () => {
  it("renders SearchPanel in a dialog and closes on Escape", async () => {
    const onOpenChange = vi.fn();
    render(
      <ConversationSearchDialog
        open={true}
        onOpenChange={onOpenChange}
        recentConversations={[]}
        onSelectConversationId={vi.fn()}
      />,
    );

    expect(screen.getByTestId("conversation-search-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("search-panel")).toBeInTheDocument();

    await userEvent.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
```

- [ ] **Step 2: Run wrapper test to verify it fails**

Run:

```bash
npm test -- src/views/chat/ConversationSearchDialog.test.tsx
```

Expected: fail because the component does not exist.

- [ ] **Step 3: Implement `ConversationSearchDialog`**

Create `src/views/chat/ConversationSearchDialog.tsx`:

```tsx
import SearchPanel from "./SearchPanel";
import type { Conversation } from "@/lib/ipc";
import Dialog from "@/components/Dialog";

interface ConversationSearchDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  recentConversations: Conversation[];
  onSelectConversationId: (conversationId: string) => void;
}

export default function ConversationSearchDialog({
  open,
  onOpenChange,
  recentConversations,
  onSelectConversationId,
}: ConversationSearchDialogProps) {
  return (
    <Dialog open={open} onClose={() => onOpenChange(false)}>
      <div data-testid="conversation-search-dialog">
        <SearchPanel
          recentConversations={recentConversations}
          onSelect={(conversationId) => {
            onSelectConversationId(conversationId);
            onOpenChange(false);
          }}
        />
      </div>
    </Dialog>
  );
}
```

If Task 1 generated a shadcn dialog component and `src/components/Dialog.tsx` has already become a wrapper over it, keep this component using `@/components/Dialog` so tests do not depend on generated API details.

- [ ] **Step 4: Add failing search loading/error tests**

Append to `src/views/chat/SearchPanel.test.tsx`:

```tsx
it("shows a loading state while search is in flight", async () => {
  vi.mocked(commands.searchConversations).mockReturnValue(new Promise(() => {}));

  render(<SearchPanel onSelect={vi.fn()} />);
  await userEvent.type(screen.getByTestId("search-input"), "slow");

  expect(await screen.findByTestId("search-loading")).toHaveTextContent("Searching");
});

it("shows a backend error without closing the panel", async () => {
  vi.mocked(commands.searchConversations).mockRejectedValue(new Error("fts unavailable"));

  render(<SearchPanel onSelect={vi.fn()} />);
  await userEvent.type(screen.getByTestId("search-input"), "broken");

  expect(await screen.findByTestId("search-error")).toHaveTextContent("fts unavailable");
  expect(screen.getByTestId("search-panel")).toBeInTheDocument();
});
```

- [ ] **Step 5: Implement loading/error states in `SearchPanel`**

In `src/views/chat/SearchPanel.tsx`, add:

```ts
const [loading, setLoading] = useState(false);
const [error, setError] = useState<string | null>(null);
```

Change `runSearch`:

```ts
const runSearch = async (value: string) => {
  setQuery(value);
  setError(null);
  if (!value.trim()) {
    setResults([]);
    setLoading(false);
    return;
  }
  setLoading(true);
  try {
    const found = await commands.searchConversations(value);
    setResults(found);
  } catch (err) {
    setResults([]);
    setError(err instanceof Error ? err.message : String(err));
  } finally {
    setLoading(false);
  }
};
```

Render before results:

```tsx
{loading && (
  <p className="text-sm text-muted-foreground" data-testid="search-loading">
    Searching
  </p>
)}
{error && (
  <p className="text-sm text-destructive" data-testid="search-error">
    {error}
  </p>
)}
```

Keep the existing no-results message only when `!loading && !error`.

- [ ] **Step 6: Wire search dialog from `App`**

In `src/App.tsx`, import and render:

```tsx
<ConversationSearchDialog
  open={showSearch}
  onOpenChange={setShowSearch}
  recentConversations={conversationListRef.current?.getConversations?.() ?? []}
  onSelectConversationId={(conversationId) => {
    conversationListRef.current?.selectById(conversationId);
  }}
/>
```

To support this, extend `ConversationListHandle` in `ConversationList.tsx`:

```ts
getConversations: () => Conversation[];
selectById: (conversationId: string) => void;
```

Add implementations:

```ts
const selectById = (conversationId: string) => {
  const conversation = conversations.find((item) => item.id === conversationId);
  if (conversation) selectConversation(conversation);
};

useImperativeHandle(ref, () => ({
  createNew,
  openSearch,
  getConversations: () => conversations,
  selectById,
}));
```

- [ ] **Step 7: Unskip and update app search test**

In `src/App.test.tsx`, unskip the test added in Task 2 after both search and command center render paths exist. If command center is not done yet, split the search expectation into a separate passing test:

```tsx
it("opens dedicated conversation search with Cmd+F", async () => {
  render(<App />);
  await waitForReady();

  pressCmd("f");

  expect(await screen.findByTestId("conversation-search-dialog")).toBeInTheDocument();
  expect(screen.getByTestId("search-panel")).toBeInTheDocument();
});
```

- [ ] **Step 8: Run task verification**

Run:

```bash
npm test -- src/views/chat/SearchPanel.test.tsx src/views/chat/ConversationSearchDialog.test.tsx src/App.test.tsx
```

Expected: all tests pass. If `App.test.tsx` still has the skipped command-center combined test, that is acceptable until Task 4.

- [ ] **Step 9: Commit**

```bash
git add src/views/chat/SearchPanel.tsx src/views/chat/SearchPanel.test.tsx src/views/chat/ConversationSearchDialog.tsx src/views/chat/ConversationSearchDialog.test.tsx src/views/chat/ConversationList.tsx src/views/chat/ConversationList.test.tsx src/App.tsx src/App.test.tsx
git commit -m "feat(search): add dedicated conversation search dialog"
```

---

## Task 4: Universal Command Center

**Files:**
- Create: `src/views/command/CommandCenter.tsx`
- Create: `src/views/command/CommandCenter.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`
- Modify: `src/views/shortcuts/ShortcutsDialog.tsx`
- Modify: `src/views/shortcuts/ShortcutsDialog.test.tsx`

**Interfaces:**
- Consumes: app state callbacks from Task 2 and search dialog from Task 3.
- Produces:
  - `CommandCenterAction`
  - `<CommandCenter open onOpenChange actions />`
  - `data-testid="command-center"`

- [ ] **Step 1: Add command center tests**

Create `src/views/command/CommandCenter.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import CommandCenter, { type CommandCenterAction } from "./CommandCenter";

const actions: CommandCenterAction[] = [
  { id: "new-agent", label: "New Agent", shortcut: "Cmd+N", run: vi.fn() },
  { id: "search", label: "Search Conversations", shortcut: "Cmd+F", run: vi.fn() },
  { id: "archive", label: "Archive Current Conversation", run: vi.fn(), disabled: true },
];

describe("CommandCenter", () => {
  it("renders enabled and disabled actions", () => {
    render(<CommandCenter open={true} onOpenChange={vi.fn()} actions={actions} />);

    expect(screen.getByTestId("command-center")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /New Agent/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: /Archive Current Conversation/ })).toBeDisabled();
  });

  it("runs an enabled action and closes", async () => {
    const onOpenChange = vi.fn();
    const run = vi.fn();

    render(
      <CommandCenter
        open={true}
        onOpenChange={onOpenChange}
        actions={[{ id: "settings", label: "Open Settings", run }]}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: /Open Settings/ }));

    expect(run).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm test -- src/views/command/CommandCenter.test.tsx
```

Expected: fail because the component does not exist.

- [ ] **Step 3: Implement `CommandCenter`**

Create `src/views/command/CommandCenter.tsx`:

```tsx
import Dialog from "@/components/Dialog";
import { Button } from "@/components/ui/button";
import { KeyboardShortcut } from "@/components/ui/KeyboardShortcut";
import { cn } from "@/lib/cn";

export interface CommandCenterAction {
  id: string;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  run: () => void;
}

interface CommandCenterProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  actions: CommandCenterAction[];
}

export default function CommandCenter({ open, onOpenChange, actions }: CommandCenterProps) {
  return (
    <Dialog open={open} onClose={() => onOpenChange(false)}>
      <div className="w-[34rem] max-w-[90vw] p-2" data-testid="command-center">
        <div className="px-2 py-2 text-xs font-medium uppercase text-muted-foreground">
          Actions
        </div>
        <div className="space-y-1">
          {actions.map((action) => (
            <Button
              key={action.id}
              type="button"
              variant="ghost"
              className={cn("h-9 w-full justify-between px-2", action.disabled && "opacity-50")}
              disabled={action.disabled}
              onClick={() => {
                action.run();
                onOpenChange(false);
              }}
            >
              <span>{action.label}</span>
              {action.shortcut && <KeyboardShortcut keys={action.shortcut.split("+")} />}
            </Button>
          ))}
        </div>
      </div>
    </Dialog>
  );
}
```

- [ ] **Step 4: Build app command actions**

In `src/App.tsx`, import:

```ts
import CommandCenter, { type CommandCenterAction } from "@/views/command/CommandCenter";
```

Add a memoized `commandActions`:

```ts
const commandActions = useMemo<CommandCenterAction[]>(
  () => [
    { id: "new-agent", label: "New Agent", shortcut: "Cmd+N", run: () => conversationListRef.current?.createNew() },
    { id: "search", label: "Search Conversations", shortcut: "Cmd+F", run: () => setShowSearch(true) },
    { id: "settings", label: "Open Settings", run: () => setShowSettings(true) },
    { id: "shortcuts", label: "Open Shortcuts", run: () => setShowShortcutsDialog(true) },
    { id: "widget-gallery", label: "Open Widget Gallery", shortcut: "Cmd+D", run: () => setShowWidgetGallery(true) },
    {
      id: "focus-composer",
      label: "Focus Composer",
      shortcut: "Cmd+L",
      disabled: !activeConversation && ready !== true,
      run: () => {
        const selector = activeConversation
          ? '[data-testid="agent-input"]'
          : '[data-testid="empty-state-input"]';
        document.querySelector<HTMLElement>(selector)?.focus();
      },
    },
    {
      id: "archive-current",
      label: "Archive Current Conversation",
      disabled: !activeConversation,
      run: () => {
        if (activeConversation) conversationListRef.current?.archiveById(activeConversation.id);
      },
    },
    {
      id: "close-surface",
      label: "Close Current Surface",
      run: () => {
        setShowSettings(false);
        setShowSearch(false);
        setShowShortcutsDialog(false);
        setShowWidgetGallery(false);
      },
    },
  ],
  [activeConversation, ready],
);
```

Extend `ConversationListHandle` with:

```ts
archiveById: (conversationId: string) => void;
```

Implement `archiveById` by finding the conversation and reusing the same archive state update/command path currently used by the row button.

- [ ] **Step 5: Render `CommandCenter`**

In `src/App.tsx`, render near `ShortcutsDialog`:

```tsx
<CommandCenter
  open={showCommandCenter}
  onOpenChange={setShowCommandCenter}
  actions={commandActions}
/>
```

- [ ] **Step 6: Update shortcuts dialog copy**

In `src/views/shortcuts/ShortcutsDialog.test.tsx`, replace the expected `Cmd+K` row text:

```tsx
expect(screen.getByText("Open command center")).toBeInTheDocument();
expect(screen.getByTestId("shortcut-combo-open-command-center")).toHaveTextContent("Cmd+K");
```

Update `ShortcutsDialog.tsx` to render `s.combo` through `KeyboardShortcut`:

```tsx
<KeyboardShortcut keys={s.combo.split("+")} data-testid={`shortcut-combo-${s.id}`} />
```

- [ ] **Step 7: Unskip app command center test**

In `src/App.test.tsx`, unskip the combined `Cmd+K`/`Cmd+F` test from Task 2. Update expected text to:

```tsx
expect(await screen.findByTestId("command-center")).toBeInTheDocument();
expect(screen.getByRole("button", { name: /New Agent/ })).toBeInTheDocument();
```

- [ ] **Step 8: Run task verification**

Run:

```bash
npm test -- src/views/command/CommandCenter.test.tsx src/views/shortcuts/ShortcutsDialog.test.tsx src/App.test.tsx
```

Expected: all listed tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/views/command src/App.tsx src/App.test.tsx src/views/chat/ConversationList.tsx src/views/chat/ConversationList.test.tsx src/views/shortcuts/ShortcutsDialog.tsx src/views/shortcuts/ShortcutsDialog.test.tsx
git commit -m "feat(app): add universal command center"
```

---

## Task 5: Sidebar Shell Redesign

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/views/chat/ConversationList.tsx`
- Modify: `src/views/chat/ConversationList.test.tsx`
- Modify: `src/views/chat/sidebarConversationRow.ts`
- Modify: `src/views/chat/sidebarConversationRow.test.ts`
- Modify: `src/components/Topbar.tsx`
- Modify: `src/components/Topbar.test.tsx`

**Interfaces:**
- Consumes: app-owned search/settings/new handlers from Tasks 2-4.
- Produces: redesigned sidebar with stable test ids:
  - `data-testid="conversation-list"`
  - `data-testid="sidebar-actions"`
  - `data-testid="conversation-item"`
  - `data-testid="conversation-status-dot"`

- [ ] **Step 1: Add sidebar behavior test for parent-owned search**

In `src/views/chat/ConversationList.test.tsx`, add:

```tsx
it("calls the parent search handler from the sidebar Search action", async () => {
  const onOpenSearch = vi.fn();
  render(
    <ConversationList
      activeId={null}
      onSelect={vi.fn()}
      onNewConversation={vi.fn()}
      onOpenSearch={onOpenSearch}
      onOpenSettings={vi.fn()}
    />,
  );

  await userEvent.click(await screen.findByTestId("open-search"));

  expect(onOpenSearch).toHaveBeenCalledTimes(1);
});
```

- [ ] **Step 2: Run sidebar test to verify current behavior**

Run:

```bash
npm test -- src/views/chat/ConversationList.test.tsx
```

Expected: pass after Task 2, or fail only on missing prop in older test setup. Fix render helpers by passing `onOpenSearch={vi.fn()}`.

- [ ] **Step 3: Redesign sidebar actions**

In `src/views/chat/ConversationList.tsx`, replace `SIDEBAR_ACTION_BUTTON` with:

```ts
const SIDEBAR_ACTION_BUTTON =
  "h-8 w-full justify-start gap-2 rounded-md px-2 text-sm text-sidebar-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground";
```

Use lucide icons:

```ts
import { Archive, Cog, Plus, Search } from "lucide-react";
```

Replace Phosphor icons in this file with these lucide icons.

- [ ] **Step 4: Redesign conversation rows without changing data behavior**

Keep existing row `role`, `tabIndex`, `onClick`, `onKeyDown`, `data-testid`, and archive command behavior. Change classes to compact shadcn/sidebar styling:

```tsx
className={cn(
  "group flex min-h-12 w-full cursor-pointer items-start gap-2 rounded-md px-2 py-2 text-left transition-colors",
  "hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:outline-offset-[-2px]",
  isActive ? "bg-sidebar-accent text-sidebar-accent-foreground" : "bg-transparent",
)}
```

Use chocolate/caramel status mapping:

```ts
const STATUS_COLOR: Record<ConversationStatus, string> = {
  done: "bg-muted-foreground/45",
  in_progress: "bg-[var(--color-doce-caramel)] animate-pulse",
  requires_action: "bg-[var(--color-doce-coral)]",
  failed: "bg-destructive",
};
```

- [ ] **Step 5: Redesign `Topbar` tokens only**

In `src/components/Topbar.tsx`, keep drag behavior and portal logic unchanged. Adjust host classes to remain `h-10 shrink-0` and use:

```ts
"flex h-10 shrink-0 select-none items-center bg-transparent text-foreground"
```

Do not change `data-testid="topbar-sidebar"` or `data-testid="topbar-main"`.

- [ ] **Step 6: Update brittle class tests**

In `src/App.test.tsx`, `Topbar.test.tsx`, and `ConversationList.test.tsx`, keep assertions for:

- visible actions
- callbacks
- active row selected
- archive button behavior
- status dot data attributes
- topbar drag region

Remove assertions that require old exact classes such as `bg-sidebar-foreground/8`.

- [ ] **Step 7: Run task verification**

Run:

```bash
npm test -- src/views/chat/ConversationList.test.tsx src/views/chat/sidebarConversationRow.test.ts src/components/Topbar.test.tsx src/App.test.tsx
```

Expected: all listed tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/App.tsx src/App.test.tsx src/views/chat/ConversationList.tsx src/views/chat/ConversationList.test.tsx src/views/chat/sidebarConversationRow.ts src/views/chat/sidebarConversationRow.test.ts src/components/Topbar.tsx src/components/Topbar.test.tsx
git commit -m "feat(shell): redesign sidebar and topbar"
```

---

## Task 6: Settings Tabs Redesign

**Files:**
- Modify: `src/views/settings/Settings.tsx`
- Modify: `src/views/settings/Settings.test.tsx`

**Interfaces:**
- Consumes: existing `commands.listMcpServers`, `commands.addMcpServer`, `commands.listMcpServerTools`, `commands.listSkills`.
- Produces:
  - `data-testid="settings-view"`
  - `data-testid="settings-tab-mcp"`
  - `data-testid="settings-tab-skills"`
  - existing input and server test ids remain unchanged.

- [ ] **Step 1: Add tab behavior tests**

Append to `src/views/settings/Settings.test.tsx`:

```tsx
it("renders MCP and Skills tabs and switches between them", async () => {
  vi.mocked(commands.listSkills).mockResolvedValue([
    { name: "pdf-tools", description: "Work with PDF files" },
  ]);

  render(<Settings onClose={vi.fn()} />);

  expect(await screen.findByTestId("settings-tab-mcp")).toHaveAttribute("aria-selected", "true");
  await userEvent.click(screen.getByTestId("settings-tab-skills"));

  expect(screen.getByTestId("settings-tab-skills")).toHaveAttribute("aria-selected", "true");
  expect(await screen.findByTestId("skill-item")).toHaveTextContent("pdf-tools");
});

it("shows an inline add-server error and keeps existing rows visible", async () => {
  vi.mocked(commands.listMcpServers).mockResolvedValue([
    { id: "srv-1", name: "existing", transport: "stdio", config: "{}", enabled: true, createdAt: 1 },
  ]);
  vi.mocked(commands.addMcpServer).mockRejectedValue(new Error("bad command"));

  render(<Settings onClose={vi.fn()} />);
  await screen.findByTestId("mcp-server-item");

  await userEvent.type(screen.getByTestId("mcp-name-input"), "broken");
  await userEvent.type(screen.getByTestId("mcp-command-input"), "missing-bin");
  await userEvent.click(screen.getByTestId("add-mcp-server"));

  expect(await screen.findByTestId("mcp-add-error")).toHaveTextContent("bad command");
  expect(screen.getByText("existing")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
npm test -- src/views/settings/Settings.test.tsx
```

Expected: fail because tabs and add-server error do not exist.

- [ ] **Step 3: Add settings tab state and add-server error**

In `src/views/settings/Settings.tsx`, add:

```ts
const [activeTab, setActiveTab] = useState<"mcp" | "skills">("mcp");
const [addError, setAddError] = useState<string | null>(null);
```

Change `addServer`:

```ts
const addServer = async () => {
  if (!name.trim() || !command.trim()) return;
  const args = argsInput.trim() ? argsInput.trim().split(/\s+/) : [];
  setAddError(null);
  try {
    await commands.addMcpServer(name.trim(), command.trim(), args);
    setName("");
    setCommand("");
    setArgsInput("");
    refresh();
  } catch (err) {
    setAddError(err instanceof Error ? err.message : String(err));
  }
};
```

- [ ] **Step 4: Render tab triggers**

Add this after the settings header:

```tsx
<div className="mb-6 inline-flex rounded-md border border-border bg-card p-1">
  <button
    type="button"
    className={activeTab === "mcp" ? "rounded bg-primary px-3 py-1 text-sm text-primary-foreground" : "px-3 py-1 text-sm text-muted-foreground"}
    aria-selected={activeTab === "mcp"}
    data-testid="settings-tab-mcp"
    onClick={() => setActiveTab("mcp")}
  >
    MCP Servers
  </button>
  <button
    type="button"
    className={activeTab === "skills" ? "rounded bg-primary px-3 py-1 text-sm text-primary-foreground" : "px-3 py-1 text-sm text-muted-foreground"}
    aria-selected={activeTab === "skills"}
    data-testid="settings-tab-skills"
    onClick={() => setActiveTab("skills")}
  >
    Skills
  </button>
</div>
```

If shadcn generated `Tabs`, use it for markup, but preserve the two test ids and `aria-selected` behavior.

- [ ] **Step 5: Split sections by active tab**

Wrap the current MCP form and server list section with this condition, preserving every existing MCP test id inside the section:

```tsx
{activeTab === "mcp" && (
  <section data-testid="settings-mcp-panel">
    {addError && (
      <p className="mt-2 text-sm text-destructive" data-testid="mcp-add-error">
        {addError}
      </p>
    )}
  </section>
)}
```

Wrap the current skills list section with this condition, preserving the `skill-item` test id inside the section:

```tsx
{activeTab === "skills" && (
  <section data-testid="settings-skills-panel">
    {skills.length === 0 ? (
      <p className="text-sm text-muted-foreground">
        No skills found. Add a folder with a SKILL.md to your skills directory.
      </p>
    ) : (
      <ul className="space-y-2">
        {skills.map((s) => (
          <li key={s.name} className="rounded-md border border-border bg-card p-3 text-sm" data-testid="skill-item">
            <span className="font-medium">{s.name}</span>
            <span className="ml-2 text-muted-foreground">{s.description}</span>
          </li>
        ))}
      </ul>
    )}
  </section>
)}
```

- [ ] **Step 6: Restyle fields and rows with shadcn tokens**

Use `bg-card`, `border-border`, `rounded-md`, `text-muted-foreground`, and the generated `Badge` component from `src/components/ui/badge` for status labels. Keep all existing test ids:

- `mcp-name-input`
- `mcp-command-input`
- `mcp-args-input`
- `add-mcp-server`
- `mcp-server-item`
- `test-mcp-server`
- `mcp-server-tools`
- `skill-item`

- [ ] **Step 7: Run task verification**

Run:

```bash
npm test -- src/views/settings/Settings.test.tsx
```

Expected: all settings tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/views/settings/Settings.tsx src/views/settings/Settings.test.tsx
git commit -m "feat(settings): redesign settings with tabs"
```

---

## Task 7: Chat Transcript, Composer, And Tool Widget Redesign

**Files:**
- Modify: `src/components/MessageContent.tsx`
- Modify: `src/components/MessageContent.test.tsx`
- Modify: `src/components/UserMessageBubble.tsx`
- Modify: `src/components/UserMessageBubble.test.tsx`
- Modify: `src/views/workspace/TranscriptTurn.tsx`
- Modify: `src/views/workspace/TranscriptTurn.test.tsx`
- Modify: `src/views/workspace/Workspace.tsx`
- Modify: `src/views/workspace/Workspace.test.tsx`
- Modify: `src/views/workspace/StreamingStatus.tsx`
- Modify: `src/views/workspace/StreamingStatus.test.tsx`
- Modify: `src/views/chat/rich-input/RichInput.tsx`
- Modify: `src/views/chat/rich-input/*.test.tsx`
- Modify: `src/views/chat/tool-widgets/*.tsx`
- Modify: `src/views/chat/tool-widgets/*.test.tsx`

**Interfaces:**
- Consumes: existing message and tool detail types from `src/lib/ipc.ts`.
- Produces: transcript rows styled through shadcn chat primitives where available, preserving all current test ids and fallback behavior.

- [ ] **Step 1: Add transcript primitive marker tests**

Append to `src/views/workspace/TranscriptTurn.test.tsx`:

```tsx
it("marks transcript turns with chat primitive data attributes", () => {
  render(<TranscriptTurn turn={turn({})} />);

  expect(screen.getByTestId("transcript-turn")).toHaveAttribute("data-chat-turn", "true");
  expect(screen.getByTestId("transcript-turn-body")).toHaveClass("min-w-0");
});
```

- [ ] **Step 2: Add message content tests for markers**

In `src/components/MessageContent.test.tsx`, add cases for context notice and assistant metadata if not already present:

```tsx
it("renders context notices as marker-style status rows", () => {
  render(
    <MessageContent
      message={{
        id: "n1",
        conversationId: "c1",
        role: "assistant",
        contentType: "context_notice",
        content: JSON.stringify({ kind: "cleared", notice: "Old tool result cleared" }),
        toolName: null,
        createdAt: 1,
        durationMs: null,
        tokenCount: null,
      }}
    />,
  );

  expect(screen.getByTestId("context-notice")).toHaveAttribute("role", "status");
  expect(screen.getByTestId("context-notice")).toHaveTextContent("Old tool result cleared");
});
```

- [ ] **Step 3: Run focused chat tests to establish baseline**

Run:

```bash
npm test -- src/components/MessageContent.test.tsx src/views/workspace/TranscriptTurn.test.tsx
```

Expected: new marker test fails until `data-chat-turn` is added.

- [ ] **Step 4: Update `TranscriptTurn` wrapper attributes**

In `src/views/workspace/TranscriptTurn.tsx`, add:

```tsx
data-chat-turn="true"
```

to the root `<div data-testid="transcript-turn">`.

Keep `sticky-user-background`, `StickyUserMessage`, pending Bash/Task widget rendering, and error rendering unchanged.

- [ ] **Step 5: Redesign user and assistant message shells**

In `src/components/MessageContent.tsx`, keep the dispatch logic unchanged. Update wrapper classes:

```tsx
<div className="mb-5" data-testid="chat-message" role="group" aria-label="You said">
  <UserMessageBubble message={m} />
</div>
```

For assistant text:

```tsx
<div className="mb-5 max-w-none" data-testid="chat-message" role="group" aria-label="doce replied">
  <MarkdownPreview>{m.content}</MarkdownPreview>
  {showAssistantMetadata && (
    <p className="mt-1 text-xs text-muted-foreground" data-testid="token-meter">
      {showAssistantDuration && <Timer createdAt={m.createdAt} durationMs={m.durationMs} />}
      {showAssistantDuration && m.tokenCount != null && " · "}
      {m.tokenCount != null && `↓ ${formatTokenCount(m.tokenCount)} tokens`}
    </p>
  )}
</div>
```

For error rows:

```tsx
className="mb-5 rounded-md border border-destructive/25 bg-destructive/10 p-3 text-sm text-destructive"
```

For summarized context notices:

```tsx
className="mb-5 rounded-md border border-border bg-muted p-3 text-sm text-muted-foreground"
```

For cleared context notices:

```tsx
className="mb-5 text-xs text-muted-foreground/70"
```

- [ ] **Step 6: Redesign user bubble**

In `src/components/UserMessageBubble.tsx`, preserve props and test ids. Use:

```tsx
className={cn(
  "ml-auto max-w-[85%] rounded-md border border-border bg-[var(--color-doce-cream)] p-3 text-sm text-foreground shadow-sm",
  bubbleClassName,
)}
```

Keep token meter and rich text rendering behavior unchanged.

- [ ] **Step 7: Retain current scroll implementation**

In `src/views/workspace/Workspace.tsx`, keep `StickToBottom`. Do not wire `MessageScroller` in this pass. Add a code comment above `StickToBottom`:

```ts
// The shadcn MessageScroller primitive is installed, but this pass keeps
// use-stick-to-bottom because its pinned/escape behavior is covered by
// existing workspace tests and matches the app's current chat contract.
```

- [ ] **Step 8: Restyle `RichInput` controls**

In `src/views/chat/rich-input/RichInput.tsx`, keep all Tiptap setup, attachment code paths, and submit semantics. Update only shell/action classes:

- outer composer shell: `rounded-lg border border-border bg-card shadow-sm`
- editor content: `min-h-12 px-3 py-2 text-sm`
- attach button: `variant="ghost" size="icon"`
- send button: `variant="primary" size="icon"` with Brand Accent Workbench primary tokens

Keep these test ids unchanged:

- `empty-state-input`
- `empty-state-submit`
- `agent-input`
- `agent-send`

- [ ] **Step 9: Restyle tool disclosure**

In `src/views/chat/tool-widgets/ToolDisclosure.tsx`, keep native `<details>` behavior and test ids. Update classes:

```tsx
className="group overflow-hidden rounded-md border border-border bg-card text-sm shadow-sm [&>summary::-webkit-details-marker]:hidden"
```

and:

```tsx
className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 font-mono text-xs text-muted-foreground focus-visible:outline-offset-[-2px]"
```

- [ ] **Step 10: Replace Phosphor control icons in chat surfaces**

Use lucide icons where there is a direct match:

- `ArrowDownIcon` -> `ArrowDown`
- `PaperPlaneRightIcon` -> `SendHorizontal`
- `PlusIcon` -> `Plus`
- `CaretRightIcon` -> `ChevronRight`

Keep icon sizes at 14 or 16 based on the current component.

- [ ] **Step 11: Run chat test suite**

Run:

```bash
npm test -- src/components/MessageContent.test.tsx src/components/UserMessageBubble.test.tsx src/views/workspace/TranscriptTurn.test.tsx src/views/workspace/Workspace.test.tsx src/views/workspace/StreamingStatus.test.tsx src/views/chat/rich-input/RichInput.test.tsx src/views/chat/rich-input/RichInput.attachments.test.tsx src/views/chat/rich-input/RichInput.skills.test.tsx src/views/chat/rich-input/UserMessageContent.test.tsx src/views/chat/tool-widgets
```

Expected: all listed tests pass.

- [ ] **Step 12: Commit**

```bash
git add src/components/MessageContent.tsx src/components/MessageContent.test.tsx src/components/UserMessageBubble.tsx src/components/UserMessageBubble.test.tsx src/views/workspace src/views/chat/rich-input src/views/chat/tool-widgets
git commit -m "feat(chat): redesign transcript and composer surfaces"
```

---

## Task 8: Onboarding, Shortcuts, Widget Gallery, And Folder Picker Polish

**Files:**
- Modify: `src/views/onboarding/Onboarding.tsx`
- Modify: `src/views/onboarding/Onboarding.test.tsx`
- Modify: `src/views/shortcuts/ShortcutsDialog.tsx`
- Modify: `src/views/shortcuts/ShortcutsDialog.test.tsx`
- Modify: `src/views/design-system/WidgetGallery.tsx`
- Modify: `src/views/design-system/WidgetGallery.test.tsx`
- Modify: `src/views/shared/FolderPicker.tsx`
- Modify: `src/views/shared/FolderPicker.test.tsx`
- Modify: `src/views/chat/EmptyState.tsx`
- Modify: `src/views/chat/EmptyState.test.tsx`

**Interfaces:**
- Consumes: theme tokens from Task 1, command center from Task 4, current `FolderPicker` behavior.
- Produces: restyled onboarding/shortcuts/gallery/folder selection without behavior changes.

- [ ] **Step 1: Add onboarding brand theme test**

Append to `src/views/onboarding/Onboarding.test.tsx`:

```tsx
it("uses the logo-forward onboarding shell", async () => {
  vi.mocked(commands.getHardwareProfile).mockResolvedValue({
    tier: "apple-silicon-16gb",
    ramGb: 16,
    chip: "Apple M2",
    diskFreeGb: 200,
  });

  render(<Onboarding onReady={() => {}} />);

  expect(await screen.findByAltText("doce")).toHaveClass("h-24");
  expect(screen.getByText("doce")).toBeInTheDocument();
});
```

- [ ] **Step 2: Restyle onboarding only**

In `src/views/onboarding/Onboarding.tsx`, keep the effect and install progress logic unchanged. Update wrapper classes:

```tsx
className="flex h-dvh flex-col items-center justify-center gap-6 bg-background px-6 text-center text-foreground"
```

Update the progress bar track:

```tsx
className="h-2 w-full overflow-hidden rounded-full bg-muted"
```

Update progress fill:

```tsx
className="h-full w-full origin-left bg-[var(--color-doce-caramel)] transition-transform duration-300 ease-out"
```

- [ ] **Step 3: Restyle shortcuts dialog**

In `src/views/shortcuts/ShortcutsDialog.tsx`, keep `ShortcutsDialogProps` unchanged. Update the dialog body:

```tsx
<div className="w-full p-4" data-testid="shortcuts-dialog">
  <div className="mb-4 flex items-center justify-between">
    <h2 className="text-sm font-semibold">Keyboard shortcuts</h2>
    <Button
      variant="ghost"
      size="icon-sm"
      className="text-muted-foreground hover:bg-accent"
      onClick={onClose}
      data-testid="close-shortcuts-dialog"
      aria-label="Close dialog"
    >
      <X size={16} />
    </Button>
  </div>
  <ul className="space-y-1">
    {shortcuts.map((s) => (
      <li
        key={s.id}
        className="flex items-center justify-between rounded-md px-2 py-1.5 text-sm"
        data-testid="shortcut-item"
      >
        <span className="text-muted-foreground">{s.description}</span>
        <KeyboardShortcut keys={s.combo.split("+")} data-testid={`shortcut-combo-${s.id}`} />
      </li>
    ))}
  </ul>
</div>
```

Use `X` from `lucide-react` for the close icon.

- [ ] **Step 4: Restyle `FolderPicker` without behavior changes**

Keep all path suggestion logic and test ids. Update visual classes:

- menu: `rounded-md border border-border bg-popover p-2 text-popover-foreground shadow-md`
- rows: `rounded-md px-2 py-1.5 hover:bg-accent hover:text-accent-foreground`
- folder icon: use `Folder` from `lucide-react`

Do not convert to a new popover implementation if it breaks existing path-prefix tests.

- [ ] **Step 5: Restyle empty state around composer**

In `src/views/chat/EmptyState.tsx`, keep `submit` behavior unchanged. Update layout:

```tsx
className="flex h-full flex-col items-center justify-center bg-background px-6 text-foreground"
```

Wrap the composer:

```tsx
className="relative w-full max-w-2xl space-y-3 [view-transition-name:chat-composer]"
```

- [ ] **Step 6: Update widget gallery sections**

In `src/views/design-system/WidgetGallery.tsx`, add sections that render:

- `Button` variants
- command center preview text
- settings row preview
- existing tool widget examples
- Brand Accent Workbench color swatches using CSS variables

Keep `data-testid="widget-gallery"` and existing widget examples so current tests remain meaningful.

- [ ] **Step 7: Run task verification**

Run:

```bash
npm test -- src/views/onboarding/Onboarding.test.tsx src/views/shortcuts/ShortcutsDialog.test.tsx src/views/shared/FolderPicker.test.tsx src/views/chat/EmptyState.test.tsx src/views/design-system/WidgetGallery.test.tsx
```

Expected: all listed tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/views/onboarding src/views/shortcuts src/views/design-system src/views/shared src/views/chat/EmptyState.tsx src/views/chat/EmptyState.test.tsx
git commit -m "feat(ui): polish secondary app surfaces"
```

---

## Task 9: Radix And Icon Cleanup, Full Verification

**Files:**
- Modify: `package.json`
- Modify: `package-lock.json`
- Modify: any remaining `src/**/*.tsx` importing `@radix-ui` or old control icons
- Modify: tests with stale shortcut/icon text

**Interfaces:**
- Consumes: all previous tasks.
- Produces: final verified app with no Radix references.

- [ ] **Step 1: Search for Radix**

Run:

```bash
rg "@radix-ui|radix-ui" src package.json package-lock.json
```

Expected before cleanup: only package manifest references may remain. If source references remain, replace them before continuing.

- [ ] **Step 2: Remove Radix package**

Run:

```bash
npm uninstall @radix-ui/react-slot
```

Expected: `package.json` and `package-lock.json` remove `@radix-ui/react-slot`.

- [ ] **Step 3: Search for Phosphor control icons**

Run:

```bash
rg "@phosphor-icons/react" src
```

Expected: any remaining matches are reviewed. Replace remaining control-surface icons with lucide equivalents. If a non-control Phosphor icon remains with no lucide equivalent, keep it and keep the dependency.

- [ ] **Step 4: Run formatter**

Run:

```bash
npm run format
```

Expected: formatting completes successfully and modifies only frontend files touched by this plan.

- [ ] **Step 5: Run full unit tests**

Run:

```bash
npm test
```

Expected: all Vitest tests pass.

- [ ] **Step 6: Run build**

Run:

```bash
npm run build
```

Expected: TypeScript and Vite build pass.

- [ ] **Step 7: Run lint**

Run:

```bash
npm run lint
```

Expected: oxlint passes.

- [ ] **Step 8: Verify Radix removal**

Run:

```bash
rg "@radix-ui|radix-ui" src package.json package-lock.json
```

Expected: no output. Exit code may be 1 because no matches were found; that is the desired result.

- [ ] **Step 9: Verify site was not redesigned**

Run:

```bash
git diff --name-only HEAD~8..HEAD -- site
```

Expected: no output. If task count differs, use `git diff --name-only main -- site` before final PR/merge and confirm no `site/` files changed.

- [ ] **Step 10: Manual smoke test**

Run:

```bash
npm run dev
```

Expected: Vite starts on port 1420. Open the app through Tauri or browser dev flow used in this repository and manually verify:

- onboarding still appears when no installed model is available
- sidebar New Agent shows the empty composer
- `Cmd+F` opens conversation search
- `Cmd+K` opens command center
- command center opens Settings, Search, Shortcuts, Widget Gallery, and New Agent
- settings MCP add/test and Skills tabs render
- transcript renders user, assistant, tool, context notice, and error rows
- composer can submit text and still blocks while generation is active
- sidebar row text and chat content do not overlap at narrow widths

Stop the dev server before finishing this task.

- [ ] **Step 11: Commit final cleanup**

```bash
git add package.json package-lock.json src
git commit -m "chore(ui): verify shadcn redesign cleanup"
```

---

## Plan Self-Review

Spec coverage:

- Latest shadcn with Base UI: Task 1.
- `add --all`: Task 1.
- Brand Accent Workbench theme: Task 1.
- Shadcn Dialog compatibility wrapper: Task 1.
- Radix removal: Tasks 1 and 9.
- App-only scope and no `site/` redesign: Global Constraints and Task 9.
- Current shell preserved: Tasks 2, 5, and 7.
- Dedicated search: Task 3.
- Universal `Cmd+K`: Task 4.
- Settings tabs: Task 6.
- Chat primitives without scroll regression: Task 7 keeps `use-stick-to-bottom` and adopts transcript styling primitives.
- Onboarding, shortcuts, widget gallery: Task 8.
- Full verification: Task 9.

No task changes backend commands, Rust code, storage, or IPC contracts.
