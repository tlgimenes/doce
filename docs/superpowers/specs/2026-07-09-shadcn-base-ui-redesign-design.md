# Shadcn Base UI Redesign Design

## Summary

Move the doce Tauri app UI to the latest shadcn stack with Base UI, install the
full shadcn component set up front, and redesign the app around the existing
two-pane shell. The visual direction is Brand Accent Workbench: compact neutral
workbench surfaces with logo-derived chocolate, caramel, peach, coral, cream,
and warm-white accents.

This is an app-only redesign. The static marketing site under `site/` is out of
scope.

## Context

doce is currently a Vite/Tauri React app using Tailwind v4, a small local UI
primitive set, Phosphor icons, Tiptap for rich input, `use-stick-to-bottom` for
chat scrolling, and one Radix dependency for `Slot`.

Relevant shadcn state as of July 9, 2026:

- shadcn's July 2026 changelog makes Base UI the default component library for
  new projects while keeping Radix supported.
- shadcn's CLI supports `shadcn add --all` / `-a` to add all available
  components.
- shadcn's June 2026 chat component release includes `MessageScroller`,
  `Message`, `Bubble`, `Attachment`, and `Marker`, plus `scroll-fade` and
  `shimmer` utilities.

Sources:

- https://ui.shadcn.com/docs/changelog
- https://ui.shadcn.com/docs/cli

## Goals

- Use shadcn's latest Base UI component path for the app.
- Install all available shadcn components up front, then use them deliberately
  in the redesign.
- Remove Radix from the app codebase and dependencies.
- Preserve the current shell and workflow: sidebar, main chat workspace,
  topbar, empty composer, settings, search, onboarding, shortcuts, and widget
  gallery.
- Redesign core app surfaces in one coordinated pass rather than a staged
  component-by-component migration.
- Adopt shadcn chat primitives where they improve transcript, scroll, message,
  attachment, and marker behavior without changing backend contracts.
- Add a universal `Cmd+K` command center.
- Keep dedicated conversation search and settings surfaces.

## Non-Goals

- Redesign the static `site/` assets or marketing page.
- Change backend commands, storage, model behavior, or Tauri IPC contracts.
- Rethink the product navigation beyond the current two-pane app shell.
- Keep Radix around after the migration.
- Replace Tiptap in `RichInput`.
- Replace existing domain-specific tool widget behavior with generic shadcn
  examples.

## Architecture

The app keeps the current high-level shell:

- fixed sidebar for conversation navigation and primary actions
- main pane for empty state, active workspace, settings, or widget gallery
- shared topbar with Tauri drag behavior and portal slots
- onboarding fallback when the app is not ready
- modal/sheet/dialog surfaces for search, command center, and shortcuts

`src/components/ui` becomes the shadcn-generated primitive layer. App-specific
components continue to live outside that directory and compose shadcn
components instead of being replaced by generic demos.

The Tailwind v4 CSS-first setup remains. `src/styles/theme.css` becomes the
bridge between shadcn's CSS variables/utilities and doce's Brand Accent
Workbench tokens.

## Theme

Brand Accent Workbench uses the logo as a palette source but keeps the app
quiet enough for repeated use:

- warm neutral background and card surfaces
- chocolate primary/action/focus color
- caramel progress/live status accent
- peach/coral soft attention accents
- cream and warm-white elevated surfaces
- compact spacing, 8px-or-less radii unless a generated component requires a
  smaller radius
- restrained borders and shadows

The theme should avoid turning every surface peach or brown. Logo colors are
primarily for selection, focus, status, progress, primary actions, and small
brand moments.

## Component Strategy

Initialize shadcn for the existing Vite app using Base UI and CSS variables.
Then add all available shadcn components with the CLI's `--all` option.

Generated components should be kept source-owned in the repository, as shadcn
intends. They may be adjusted to match doce's theme and TypeScript conventions,
but domain logic should not be placed directly into generic primitives.

Use shadcn components directly or with light wrappers for:

- `Button`, `Input`, `Textarea`, `Dialog`, `Command`, `Kbd`, `Tooltip`,
  `Popover`, `Sheet`, `Tabs`, `Separator`, `Badge`, `ScrollArea`, `Sidebar`,
  `Skeleton`, `Spinner`, `Progress`, `Switch`, `Field`, `Item`, and `Empty`
- chat primitives: `MessageScroller`, `Message`, `Bubble`, `Attachment`, and
  `Marker`
- utilities: `scroll-fade` and `shimmer`

Keep these as app-specific components:

- `RichInput`: keep Tiptap, attachments, skill mentions, paste collapse, native
  file behavior, drag/drop behavior, submit semantics, and tests; restyle its
  shell/actions with shadcn primitives.
- `TranscriptTurn`: keep transcript grouping and tool-call semantics; render
  user/assistant content with chat primitives.
- tool widgets: keep Bash, Read, Write, Search, Task, UserAsk, and unknown-tool
  parsing/behavior; redesign frames with shadcn components.
- `ConversationList`: keep polling and data ownership; render with shadcn
  sidebar/item patterns.
- `Topbar`: keep Tauri drag behavior and portal slots; restyle with tokens.
- `FolderPicker`: keep filesystem behavior; rebuild as a popover/command-style
  picker unless that breaks path suggestion behavior or existing tests. If that
  happens, keep the custom picker logic and restyle its controls with shadcn
  primitives.
- `Settings`: restructure into shadcn tabs/sections/forms while preserving
  current MCP and skills behavior.

Radix removal requirements:

- Replace `@radix-ui/react-slot` usage with the Base UI/shadcn equivalent.
- Do not generate or add Radix-based shadcn variants.
- Remove Radix packages after imports are gone.
- Verification must fail if `@radix-ui` or `radix-ui` remains in app source or
  package manifests.

## Surface Design

### Sidebar

Preserve the current sidebar workflow:

- New Agent keeps the current empty-composer behavior.
- Search opens the dedicated conversation search surface.
- Settings opens the settings surface.
- Conversation rows still show title, workspace, relative time, work state, and
  status.
- Archive remains a row action.
- Running, needs-input, failed, and done states stay visible.

Use shadcn `Sidebar` and item/list patterns for structure while keeping doce's
conversation-specific row behavior.

### Conversation Search

Search remains dedicated, opened by `Cmd+F` and the sidebar Search action. It
should become a polished dialog with:

- focused search input
- recent conversations when the query is empty
- highlighted FTS snippets for matches
- keyboard-friendly result navigation
- loading, empty, no-results, and backend-error states
- result selection that reuses the current conversation activation path

Search should not be merged into `Cmd+K`, although `Cmd+K` can include an
action that opens search.

### Command Center

`Cmd+K` becomes a universal command center for actions. It should derive actions
from existing app state and callbacks rather than duplicating business logic.

Initial actions:

- New Agent
- Search Conversations
- Open Settings
- Open Shortcuts
- Open Widget Gallery
- Focus Composer
- Archive Current Conversation
- Close Current Surface

Unavailable actions should be hidden or disabled with clear labels. For example,
Archive Current Conversation only appears or enables when a conversation is
active.

### Settings

Settings becomes a full app surface with tabs.

Initial sections:

- MCP Servers
- Skills

MCP server behavior stays the same: list servers, add a server, test a server,
and display tool names or errors. Skills remain read-only and continue to show
installed skill summaries.

Use shadcn field/form primitives, item rows, badges, loading states, empty
states, and inline feedback.

### Workspace And Chat

The workspace keeps its current data flow and generation behavior. The visual
structure should use shadcn chat primitives for the transcript entities listed
below:

- `MessageScroller` or `@shadcn/react/message-scroller` for scroll behavior if
  it preserves current stick-to-bottom behavior and test coverage.
- `Message` and `Bubble` for user/assistant turns.
- `Marker` for streaming state, tool activity, context notices, and separators.
- `Attachment` for user attachments in transcript. Composer attachment chips
  keep their Tiptap node behavior, but should share the same token styling and
  action affordances as transcript attachments.

If `MessageScroller` cannot preserve existing scroll semantics within the first
rewrite, keep the current scroll implementation and still adopt the other chat
primitives.

### Tool Widgets

Tool widgets should feel like first-class chat artifacts, not generic cards.
Use shadcn primitives for frames, disclosure, badges, separators, buttons, and
scroll areas while preserving current parsing and test behavior.

The redesign should keep existing progressive rendering and fallback behavior:
unknown or malformed tool payloads still render diagnostic widgets rather than
breaking the transcript.

### Onboarding, Shortcuts, Widget Gallery

Onboarding is restyled only. Its readiness/install logic is unchanged.

Shortcuts should move to shadcn dialog/command-like presentation while keeping
the current shortcut registry as the source of truth.

Widget Gallery should remain a developer/design-system surface and be updated to
preview the new primitives, chat widgets, command center, settings rows, and
theme tokens.

## Data And State

Backend contracts do not change.

`App.tsx` continues to own global surface state:

- app ready/onboarding flow
- active conversation
- pending initial turn
- settings/search/command/shortcuts/widget-gallery visibility
- shell routing

The implementation may clean up local state boundaries, but should not introduce
a new router unless the existing branching becomes a blocker.

Conversation list continues using:

- `listConversations`
- `listWorkspaces`
- `archiveConversation`

Search continues using:

- `searchConversations`

Settings continues using:

- `listMcpServers`
- `addMcpServer`
- `listMcpServerTools`
- `listSkills`

Workspace continues using the existing message load, generation, event refresh,
context usage, duplicate-send prevention, pending-question routing, and submit
paths.

Command center actions call the same handlers as sidebar buttons, workspace
controls, and existing shortcuts.

## Error Handling

Preserve current failure behavior and make failures more visible.

- Readiness/onboarding: keep retry/fallback behavior so the app does not render
  a blank screen.
- Search: show loading, empty query, no results, and backend failure states
  without closing the surface.
- Settings: render MCP add/test failures inline near the relevant form or row.
  One server test failure must not hide existing server rows.
- Chat: generation errors continue rendering in the workspace without erasing
  transcript state.
- Tool widgets: malformed payloads still fall back to diagnostic/unknown widgets.
- AskUserQuestion: preserve current disabled/submitting/error paths.
- Command center: unavailable actions are hidden or disabled based on app state.

## Testing

Update or add frontend tests for:

- shadcn primitive compatibility where doce relies on behavior, especially
  disabled and composition behavior
- `App` routing for settings/search/command center visibility
- `ConversationList` actions, row behavior, status display, and archive behavior
- `SearchPanel` query, recent results, no-results, errors, and selection
- `Settings` MCP add/test/skills rendering
- `Workspace` transcript rendering, generation-active/send-disabled behavior,
  and chat primitive rendering
- `RichInput` submit, attachments, skill mentions, paste-collapse, disabled
  behavior, and composer actions
- command center actions and shortcuts
- shortcuts dialog content from the shared shortcut registry

Verification commands:

- `npm run build`
- `npm test`
- `npm run lint`
- `rg "@radix-ui|radix-ui" src package.json package-lock.json`

Manual verification:

- app starts in Vite/Tauri development
- onboarding still appears when not ready
- new conversation flow works from sidebar and command center
- conversation search opens with `Cmd+F` and result selection works
- command center opens with `Cmd+K` and actions call existing workflows
- settings MCP add/test and skills list work
- transcript streams without scroll regressions
- tool widgets render success, error, pending, collapsed, and unknown states
- sidebar rows and chat content do not overflow awkwardly at narrow widths

## Acceptance Criteria

- The app uses shadcn generated components with Base UI, not Radix.
- All shadcn components are installed up front.
- Radix imports and package dependencies are removed.
- The static `site/` design is unchanged.
- The current two-pane app shell remains recognizable.
- The app uses the Brand Accent Workbench theme.
- Search is a dedicated surface.
- `Cmd+K` is a universal action command center.
- Settings is a redesigned app surface with MCP and skills sections.
- Chat transcript and composer use shadcn `Message`, `Bubble`, `Marker`, and
  `Attachment` primitives where they map to existing content. `MessageScroller`
  is adopted only if it preserves current scroll behavior; otherwise current
  scroll behavior is retained for this pass.
- Existing backend behavior and IPC contracts are unchanged.
- Build, tests, lint, and Radix-removal verification pass before completion.

## Risks

- `shadcn add --all` may add a large amount of code and dependencies. The
  implementation should keep generated primitives separate from domain
  components and avoid wiring unused primitives into app logic.
- `MessageScroller` may not exactly match the current stick-to-bottom behavior.
  Preserve user-visible scroll behavior even if that means deferring the
  scroller primitive.
- Removing Radix may require replacing `asChild`/slot behavior in local
  primitives and tests.
- A one-pass redesign has high snapshot/test churn. Keep behavior tests focused
  on user workflows rather than generated class names.
