# Sidebar Conversation Row Design

Date: 2026-07-07
Status: Approved for implementation planning

## Goal

Improve the sidebar conversation rows so they are easier to scan and feel closer to the provided reference, while keeping the UI quiet and operational. Each row should communicate:

- what the conversation is about
- when it was last active
- which workspace it belongs to
- what state the work is in

The row should not expose raw token counts. Token counts are a technical implementation detail and are less useful than an explicit work-state label for deciding what to open next.

## Approved Row Structure

Each conversation row uses two text rows beside the existing status dot.

Row 1:

- Left: conversation title
- Right: relative updated time, such as `2m`, `14m`, `1h`

Row 2:

- Left: workspace path label
- Right: business-oriented work state label

Example:

```text
[dot] Fix fuzzy search ranking              2m
      ~/code/doce                      Working
```

## Visual Behavior

- The status dot stays as the left scan anchor.
- The title is the strongest text in the row.
- Time is right aligned and should not wrap.
- The second row uses muted text.
- The path truncates before the state label.
- Active row uses the existing selected background direction, but with a cleaner two-line layout.
- Hover stays subtle and should not add outlines or heavy borders.
- Rows remain compact enough for repeated scanning in the sidebar.

## Path Display Rules

- A conversation with no workspace, or with the current user's home directory, displays `Home`.
- A workspace inside the current user's home directory displays with `~`, for example `~/code/doce`.
- A workspace outside the home directory displays as its absolute path from root.
- If a workspace id cannot be resolved, fall back to `Home` rather than showing an internal id.

## Work State Labels

Use the existing `ConversationStatus` as the source of truth, but render product-facing labels:

| Existing status | Sidebar label |
| --- | --- |
| `in_progress` | `Working` |
| `requires_action` | `Review` |
| `failed` | `Blocked` |
| `done` | `Ready` |

The existing dot color can continue to represent the same status. The label makes the state readable without requiring the user to infer meaning from color alone.

## Data Flow

The current `Conversation` shape already includes `workspaceId`, `updatedAt`, `title`, and `status`. The sidebar can derive most of the row from this data.

Workspace paths should be resolved by loading workspaces through the existing workspace command and mapping `workspaceId` to the workspace path in the sidebar. This avoids adding new backend API surface for the first implementation.

If this creates visible loading churn, use `Home` as the initial fallback and replace it once workspace data is available.

## Components

The existing `ConversationList` remains the owner of the sidebar list.

Add focused helpers near the sidebar code or in a small local module if tests need them:

- format relative time from `updatedAt`
- format workspace path for display
- map `ConversationStatus` to work-state label

Avoid broad refactors or redesigning search, commands, or conversation persistence as part of this change.

## Testing

Add or update focused tests for:

- conversation rows render title and relative time
- status maps to the expected business label
- home workspace renders as `Home`
- user-home paths render with `~`
- long title/path content truncates through class names or stable structure
- selecting a row still calls the existing `onSelect` behavior

## Out of Scope

- Showing token count or context usage in the sidebar row
- Adding file counts, changed file counts, or model cost metrics
- Changing backend search behavior
- Redesigning sidebar action buttons
- Changing conversation status computation
