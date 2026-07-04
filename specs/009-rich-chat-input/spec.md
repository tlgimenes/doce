# Feature Specification: Rich Chat Input

**Feature Branch**: `009-rich-chat-input`

**Created**: 2026-07-03

**Status**: Draft

**Input**: User description: "Replace the app's three separate plain-text chat inputs with one shared rich-text input, based on an existing reference implementation (~/code/mesh), adding: (1) pasting a large block of text collapses into a '<pasted N lines>' chip, (2) attaching an image replaces it with an '<imagename.png>' chip, (3) typing '/' opens a mention-style picker listing locally-installed skills, selecting one injects that skill's content into the conversation's context. Confirmed via interview: unify all three existing chat inputs into one shared component; images are UI-only for now (the installed model can't see them) — their data stays local and is never sent to the model; selecting a skill performs real context injection, not just a cosmetic chip (first-time wiring of skills into the agent loop); pasted-text collapses at roughly 10 lines or 500 characters, whichever comes first, and is expandable back to plain text; sent messages keep their rich structure so reopening a conversation re-renders the same chips; a native file-picker button is included alongside paste/drag-drop for attaching files."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - One consistent, capable input everywhere you type to Doce (Priority: P1)

Today, the three places a user types a message (starting a new conversation, chatting in a plain conversation, chatting in an agent/workspace conversation) are three separate, plain, single-line-feeling text boxes that behave slightly differently from each other. A user typing a message in any of them gets the same capable, multi-line-aware input: Enter sends, Shift+Enter adds a new line, and the input feels and behaves identically no matter which view they're in.

**Why this priority**: This is the foundation every other capability in this feature builds on — unifying the three inputs into one is what makes it possible to add paste-collapse, image attachment, and skill mentions everywhere at once instead of three times over, and it removes an existing inconsistency users already experience today.

**Independent Test**: Can be fully tested by typing a multi-line message (using Shift+Enter) in each of the three surfaces (new-conversation composer, plain chat, agent/workspace chat) and confirming identical behavior, appearance, and Enter-to-send/Shift+Enter-for-newline handling in all three.

**Acceptance Scenarios**:

1. **Given** a user is on any of the three chat-input surfaces, **When** they press Shift+Enter, **Then** a new line is inserted without sending the message.
2. **Given** a user has typed a message, **When** they press Enter (without Shift), **Then** the message sends immediately.
3. **Given** a user compares the input's appearance and behavior across all three surfaces, **When** they switch between them, **Then** the input looks and behaves identically in each.

---

### User Story 2 - Pasting a lot of text doesn't turn the input into a wall of text (Priority: P2)

A user pastes a large block of text (e.g., a stack trace, a long log excerpt, a file's contents) into the input. Instead of the input filling up with dozens of lines of raw text, the pasted content collapses into a compact "<pasted N lines>" chip. The user can still see at a glance that something was pasted, and can expand it back into editable text if they want to review or edit what they pasted before sending.

**Why this priority**: This is the single highest-value ergonomic improvement for the input itself — without it, exactly the kind of large paste a coding/system agent's users routinely need to share (errors, logs, file contents) makes the input unusable to visually scan or edit around.

**Independent Test**: Can be fully tested by pasting a large block of text and confirming it collapses into a compact chip showing the line count, then clicking the chip and confirming it expands back into the original, fully editable text.

**Acceptance Scenarios**:

1. **Given** the input is empty, **When** a user pastes a block of text longer than roughly 10 lines or 500 characters, **Then** the pasted content collapses into a "<pasted N lines>" chip instead of appearing as raw text.
2. **Given** a user pastes a short amount of text (under the collapse threshold), **When** the paste completes, **Then** the text appears normally, not as a chip.
3. **Given** a pasted-text chip is present in the input, **When** the user clicks it, **Then** it expands back into the original text, fully editable in place.
4. **Given** a user sends a message containing a pasted-text chip, **When** the agent receives it, **Then** the agent sees the full original pasted text, not a shortened or summarized version — the chip is a display convenience only.

---

### User Story 3 - Bring a skill into the conversation by typing "/" (Priority: P2)

A user wants the agent to use one of their locally-installed skills for the current task. Typing "/" opens a picker listing their installed skills by name and description. Selecting one inserts a marker into the message showing which skill was chosen, and that skill's full instructions become part of what the agent considers for that turn — not just a visual note, a real change in what the agent has to work with.

**Why this priority**: This is the one new capability in this feature that changes what the agent can actually do, not just how the input looks — skills exist in the app today but have no way to be brought into an actual conversation turn.

**Independent Test**: Can be fully tested by installing a skill, typing "/" in an agent-mode conversation, selecting that skill from the picker, sending a message, and confirming the agent's behavior reflects that skill's instructions (not just that a chip appeared).

**Acceptance Scenarios**:

1. **Given** a user is composing a message in an agent-mode conversation (a new conversation via the composer, or an existing workspace conversation) and has at least one skill installed, **When** they type "/", **Then** a picker appears listing their installed skills by name and description.
2. **Given** the skill picker is open, **When** the user selects a skill, **Then** a marker for that skill appears in the message, and the picker closes.
3. **Given** a message with a skill marker is sent, **When** the agent processes that turn, **Then** the selected skill's full instructions are included in what the agent has available, in addition to the user's typed text.
4. **Given** a user is composing a message in a plain (non-agent) conversation, **When** they type "/", **Then** no skill picker appears, since plain conversations have no access to skills or tools.
5. **Given** a user has no skills installed, **When** they type "/" in an agent-mode conversation, **Then** the picker shows a legible empty state rather than nothing or an error.

---

### User Story 4 - Attach an image for the record, even though the agent can't see it yet (Priority: P3)

A user wants to attach an image to their message — for their own reference, or in preparation for a future version of Doce that can actually see images. They can paste an image from the clipboard, drag one into the input, or pick one via a familiar file-selection dialog. However they attach it, it appears as a compact "<imagename.png>" chip rather than raw data cluttering the input, and hovering it shows a preview.

**Why this priority**: Lowest priority of the four because the current agent genuinely cannot see or use an image yet — this delivers a consistent, ready-for-later attachment experience and visual parity with the other chip types, but not a new agent capability today.

**Independent Test**: Can be fully tested by attaching an image via each of the three methods (paste, drag-and-drop, file-picker button) and confirming each produces the same compact chip with a hover preview, and that sending the message doesn't error or hang even though the agent can't process the image's contents.

**Acceptance Scenarios**:

1. **Given** the input has focus, **When** a user pastes an image from their clipboard, **Then** it appears as a compact "<imagename.png>"-style chip, not raw data.
2. **Given** the input is visible, **When** a user drags an image file onto it, **Then** the same chip appears.
3. **Given** a user clicks an "attach a file" control, **When** they pick an image via the resulting file-selection dialog, **Then** the same chip appears.
4. **Given** an image chip is present, **When** the user hovers over it, **Then** a preview of the image appears.
5. **Given** a message containing an image chip is sent, **When** the agent processes that turn, **Then** the agent's context reflects only that an image was attached (e.g., a filename mention), not the image's actual visual content, and the turn completes normally rather than erroring or stalling.

---

### Edge Cases

- What happens if a user pastes text that is mostly whitespace or blank lines past the collapse threshold? It still collapses per the same line/character rule — the threshold doesn't special-case content, only size.
- What happens if a user pastes text directly on top of an existing pasted-text chip (replacing a selection that includes it)? The old chip is removed as part of the replaced selection and a new chip (or plain text, if under threshold) is inserted in its place, same as replacing any other selected content.
- What happens if a user attaches a file that isn't an image (e.g., a PDF or text file) via drag-and-drop or the file picker? It still becomes an attachment chip using its filename, without an image-style hover preview.
- What happens if a user selects a skill from the picker, then deletes that skill marker before sending? That skill's content is not included in the turn — only markers actually present in the sent message count.
- What happens if the selected skill's content has changed or been removed from disk between selecting it and sending the message? Sending surfaces an error for that turn rather than silently sending an incomplete or stale context, consistent with how other failures in message sending already surface today.
- What happens to an in-progress paste-text expansion, image attachment, or open skill picker if the user switches to a different conversation mid-composition? Each surface's input is independent, matching how each already keeps its own separate draft today.
- What happens when a user reopens a conversation containing a message with a pasted-text chip, an image chip, and a skill marker? All three render again in their collapsed/chip form in the conversation history, matching how they appeared when originally sent.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST present one consistent rich-text input experience across all three places a user composes a message to Doce (starting a new conversation, a plain conversation, an agent/workspace conversation), replacing the three separate plain-text inputs that exist today.
- **FR-002**: The input MUST send the message on Enter and insert a new line on Shift+Enter, consistently across all three surfaces.
- **FR-003**: Pasting text that exceeds roughly 10 lines or 500 characters (whichever is reached first) MUST collapse into a compact chip showing how many lines were pasted, instead of inserting the raw text.
- **FR-004**: A pasted-text chip MUST be expandable back into its original, fully editable text by interacting with it.
- **FR-005**: The full original pasted text (not a summary or truncation) MUST be what the agent actually receives when a message containing a pasted-text chip is sent.
- **FR-006**: A user MUST be able to attach an image via clipboard paste, drag-and-drop, and a dedicated file-selection control, with all three methods producing the same visual result.
- **FR-007**: An attached image MUST render as a compact, named chip (not raw/inline image data cluttering the input) with a hover preview of the image.
- **FR-008**: An attached non-image file MUST render as a compact, named chip without an image-style preview.
- **FR-009**: An attached image's visual content MUST NOT be included in what is sent to the agent for processing — only a reference to it (e.g., its filename) MUST be included, since the currently-supported agent cannot process image content.
- **FR-010**: In an agent-mode composing surface (the new-conversation composer, or an existing agent/workspace conversation), typing "/" MUST open a picker listing the user's installed skills by name and description.
- **FR-011**: Typing "/" in a plain (non-agent) conversation MUST NOT open a skill picker, since plain conversations have no tool/skill access.
- **FR-012**: Selecting a skill from the picker MUST insert a visible marker for that skill into the message at the point of selection.
- **FR-013**: When a message containing a skill marker is sent, that skill's full content MUST be included in what the agent has available for that turn, not merely displayed as a visual note.
- **FR-014**: If a skill referenced by a marker in a message can no longer be read at send time, the system MUST surface an error for that send attempt rather than silently sending an incomplete turn.
- **FR-015**: The skill picker MUST show a legible empty state when the user has no skills installed, rather than appearing broken or not responding.
- **FR-016**: A sent message's rich content (pasted-text chips, image/file chips, skill markers) MUST be preserved such that reopening its conversation re-renders the same chips, not a plain-text fallback.
- **FR-017**: The input MUST NOT impose an artificial character or length limit of its own beyond what pasting/attachment behavior already implies.

### Key Entities

- **Message composition**: The in-progress, not-yet-sent content a user is authoring in any of the three input surfaces — plain text interspersed with zero or more pasted-text chips, image/file chips, and skill markers, all editable/removable before sending.
- **Pasted-text chip**: A collapsed representation of a large pasted text block, carrying the original full text and a displayed line count; expandable back to its original text.
- **Attachment chip**: A collapsed representation of an attached file, carrying the file's name and (for images) its visual content for local preview purposes; distinct visual treatment for images (with hover preview) versus other file types.
- **Skill marker**: A reference to one of the user's installed skills, inserted at a point in a message; resolves to that skill's full content at send time.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A user can compose and send a multi-line message using Shift+Enter in any of the three chat surfaces, with identical behavior in all three.
- **SC-002**: Pasting a large block of text (a stack trace, log excerpt, or similar) never leaves the input showing more than a handful of visible lines before sending — it collapses to a chip instead.
- **SC-003**: A user can recover and edit a large paste after collapsing it, without needing to delete and re-paste.
- **SC-004**: A user can bring one of their installed skills into a conversation turn in two actions or fewer (type "/", select the skill) and see the agent's response reflect that skill's guidance.
- **SC-005**: A user can attach an image using whichever method is most convenient to them (paste, drag, or file picker) and get the same result every time.
- **SC-006**: Reopening a conversation that contains pasted-text, image, or skill-marker chips shows the same chips a user saw when they originally sent the message — zero instances of history rendering as an unreadable plain-text dump of what was originally a rich message.
- **SC-007**: Sending a message with an attached image never causes the agent turn to error or hang because of the image's content.

## Assumptions

- All three existing chat-input surfaces (new-conversation composer, plain conversation, agent/workspace conversation) are unified into a single shared input experience — confirmed via interview, not an incremental per-surface rollout.
- The currently-supported local model cannot process image content; image attachment is intentionally scoped to a local, UI-only convenience for this pass — confirmed via interview. A future model capable of processing images is out of scope for this feature.
- Selecting a skill performs real context injection into the agent's turn, which requires connecting the app's existing skill-discovery capability to an actual conversation turn for the first time — confirmed via interview as the intended behavior, not merely a cosmetic marker.
- The paste-collapse threshold (roughly 10 lines or 500 characters, whichever comes first) is a reasonable default balancing "still readable inline" against "worth collapsing" — confirmed via interview rather than assumed.
- A native, OS-familiar file-selection dialog is included as an attachment method alongside paste and drag-and-drop — confirmed via interview.
- Skill markers and the skill picker are only relevant to agent-mode conversations (every new conversation is agent-mode per existing behavior; only pre-existing plain conversations from before that change lack tool/skill access) — consistent with the app's existing tool-access model, not a new restriction introduced by this feature.
- Non-image file attachments (e.g., text or PDF files) are included as a natural extension of the attachment capability, using the same chip treatment minus the image-specific preview — a reasonable default, not separately called out in the original request.
