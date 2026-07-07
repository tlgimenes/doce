# Feature Specification: Context Window Management for Chat and Agent Mode

**Feature Branch**: `010-context-window-management`

**Created**: 2026-07-04

**Status**: Draft

**Input**: User description: "Give doce real context-window management for its local model — live visibility into how full the conversation's context window is, automatic tiered compaction before it overflows, and offloading of huge tool outputs to files instead of stuffing them into the model's context, inspired by how Claude Code and Claude Desktop manage context."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See how full the conversation is, at a glance (Priority: P1)

While chatting, a user can see — without asking, without a failure occurring first — roughly how much of the model's context window the current conversation is using, right where they're already looking (near where they type). As the conversation grows, this indicator visibly shifts from a calm/normal state to a warning state as it approaches the limit.

**Why this priority**: This is the foundational trust-building piece. Today the user finds out a conversation is "too full" only when generation degrades or fails outright. Continuous visibility is valuable on its own even before any automatic mitigation exists, and every other story in this feature depends on the underlying token-accounting this story requires.

**Independent Test**: Start a new conversation, send messages until the conversation grows substantially, and observe the indicator move through its states purely from watching the chat UI — no developer tools, logs, or special action required.

**Acceptance Scenarios**:

1. **Given** a new, empty conversation, **When** the user views the chat screen, **Then** the context indicator shows a low/near-zero usage state.
2. **Given** a conversation whose accumulated history is a moderate fraction of the model's context window, **When** the user views the chat screen, **Then** the indicator reflects that fraction and reads in a "normal" visual state.
3. **Given** a conversation that has grown to near the model's context window limit, **When** the user views the chat screen, **Then** the indicator switches to a visually distinct "warning" state.
4. **Given** the user is actively watching the chat while a response streams in, **When** new messages are added to the conversation, **Then** the indicator updates without the user needing to refresh or switch views.

---

### User Story 2 - The conversation keeps working instead of hitting a wall (Priority: P2)

As a long-running conversation approaches the model's context limit, the system automatically makes room by first clearing out old, low-value content (stale tool outputs) and, if that isn't enough, summarizing older parts of the conversation — so the user can keep chatting productively instead of hitting an error or being forced to start a new conversation. When this happens, the user is told it happened.

**Why this priority**: This is the payoff that makes User Story 1's visibility meaningful — without it, the indicator would just be an accurate countdown to a dead end. It depends on User Story 1's token accounting to know when to act.

**Independent Test**: Drive a conversation (via a long back-and-forth or a scripted set of messages) past the point where its accumulated content would have exceeded the model's context window under today's behavior, and confirm the conversation continues to receive coherent responses instead of failing, with a visible notice that older content was condensed.

**Acceptance Scenarios**:

1. **Given** a conversation approaching its context limit that contains old tool-call results no longer central to the discussion, **When** the limit-approaching threshold is crossed, **Then** those old results are cleared first, without requiring a summarization pass, and the user is not interrupted.
2. **Given** a conversation that is still too large for the context window even after old tool results are cleared, **When** generation is about to be attempted, **Then** the system condenses older parts of the conversation into a summary, keeps the most recent turns intact, and the conversation continues.
3. **Given** a compaction (clearing or summarizing) has just occurred, **When** the user looks at the conversation transcript, **Then** a clear, unobtrusive notice appears marking that the conversation was condensed to save space.
4. **Given** the user wants to free up space proactively, **When** they choose to compact the conversation manually, **Then** the same condensing behavior runs immediately and the indicator reflects the freed-up space.

---

### User Story 3 - A single huge tool result doesn't blow the budget (Priority: P3)

In agent mode, when a tool (such as running a command or reading a file) produces a very large amount of output, the system keeps only a short preview in the conversation and stores the full output separately, where it can still be retrieved in full later if genuinely needed — instead of the entire output permanently occupying context space from that point forward.

**Why this priority**: Without this, a single oversized tool result can consume most or all of the context budget in one step, making the tiered compaction from User Story 2 far less effective and forcing premature summarization. It's scoped to agent mode specifically, since that's the only mode that currently produces open-ended tool output.

**Independent Test**: In agent mode, trigger a tool call known to produce a very large result (e.g., a command with voluminous output), and confirm the conversation's context usage rises only modestly (reflecting a short preview) rather than by the full size of the output, while the full output remains accessible on request.

**Acceptance Scenarios**:

1. **Given** a tool call is about to produce output larger than a defined size threshold, **When** the result is recorded into the conversation, **Then** only a short preview plus a reference to the full result is kept in the model-facing conversation content.
2. **Given** a tool result was stored with only a preview shown, **When** the assistant later needs the rest of that output to continue its work, **Then** it can retrieve additional portions of the full result on demand.
3. **Given** a user is reviewing the transcript, **When** they look at a tool call whose output was large, **Then** they can tell that the output was preview-only and access the full output themselves if they want to.

---

### Edge Cases

- What happens if a single message (e.g., one very large pasted block or one enormous tool result) by itself exceeds the entire context budget even with nothing else in the conversation? The system must still respond (e.g., by refusing that one action with a clear explanation) rather than silently truncating the user's own input without telling them, or crashing.
- What happens if compaction (clearing + summarization) still leaves the conversation over budget (e.g., because recent turns alone are already too large to fit)? The user must be told plainly that the conversation cannot continue growing and must be given a way forward (e.g., start fresh), rather than the system looping on repeated failed attempts.
- What happens if the user sends a new message while a compaction is in progress? The new message must not be lost or silently dropped; it should be queued until compaction completes, consistent with how the existing single-flight generation queue already behaves.
- What happens if the model being used changes (a different model with a different context window) mid-conversation? The usage indicator and thresholds must reflect the currently active model's actual limit, not a stale one.
- What happens when a conversation is reopened after being closed and reopened later? Context usage must be recomputed from the persisted conversation, not just reset to zero or left stale from a previous session.
- What happens to an offloaded tool result if its stored full copy is later unavailable (e.g., deleted)? Retrieval must fail gracefully with a clear message rather than crashing the conversation.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST continuously track how much of the active model's context window the current conversation is consuming, and make this figure available while the user is chatting (both in plain chat mode and agent mode).
- **FR-002**: System MUST display a live, always-visible indicator of context usage in the chat interface that updates as the conversation grows, without requiring a manual refresh.
- **FR-003**: The usage indicator MUST visually distinguish at least three states: normal usage, approaching-the-limit ("warning"), and a state reflecting that the conversation has just been condensed.
- **FR-004**: System MUST define a warning threshold (a portion of the context window) at which the indicator switches to its warning state, and this threshold MUST be reachable by the user well before generation would otherwise fail.
- **FR-005**: System MUST define a compaction threshold (a portion of the context window, at or after the warning threshold) at which automatic compaction is triggered before the next generation is attempted.
- **FR-006**: When the compaction threshold is crossed, system MUST first attempt a lightweight compaction pass that clears old, low-value tool-call content while preserving the most recent tool interactions, without invoking the model.
- **FR-007**: System MUST re-check context usage after the lightweight pass and, if still over the compaction threshold, MUST perform a summarization pass that condenses older conversation turns into a concise summary while preserving the most recent turns verbatim.
- **FR-008**: System MUST notify the user, visibly within the conversation, whenever compaction (lightweight clearing or summarization) has occurred.
- **FR-009**: Users MUST be able to manually trigger compaction at any time, independent of whether an automatic threshold has been crossed.
- **FR-010**: System MUST apply context tracking and the compaction pipeline to both plain chat mode and agent mode conversations.
- **FR-011**: In agent mode, system MUST detect when a tool call's result exceeds a defined size threshold and, in that case, retain only a short preview of that result plus a reference to the full result within the conversation the model sees.
- **FR-012**: System MUST allow the full content of an oversized tool result to be retrieved later (by the assistant continuing its work, and by the user inspecting the transcript), using the reference stored at the time of offloading.
- **FR-013**: System MUST allow this feature's thresholds (warning, compaction) to be adjusted by the user rather than being fixed at a single hardcoded value for all situations.
- **FR-014**: System MUST recompute context usage for a conversation from its persisted history when the conversation is reopened, rather than relying on transient in-memory state from a previous session.
- **FR-015**: System MUST handle the case where a single piece of content (a user message or an unoffloadable tool result) alone exceeds the available context budget by clearly informing the user rather than silently truncating or crashing.
- **FR-016**: System MUST handle the case where compaction does not sufficiently reduce usage by clearly informing the user that the conversation cannot continue to grow, rather than repeating the compaction attempt indefinitely.
- **FR-017**: System MUST NOT drop or silently discard a user's newly submitted message if it arrives while a compaction is in progress for that conversation.
- **FR-018**: System MUST reflect the correct context window size for whichever model is currently active, including when the active model changes between conversations.

### Key Entities

- **Context Usage State**: The live, per-conversation figure representing tokens consumed versus the active model's total context window, plus which visual state (normal / warning / just-compacted) currently applies. Tied to a conversation, recomputed as the conversation grows and when reopened.
- **Compaction Event**: A record of an automatic or manual compaction having occurred for a conversation — what kind (lightweight clearing vs. summarization), when it happened, and enough information to render the in-transcript notice to the user.
- **Offloaded Tool Result**: A tool result too large to keep inline — represented in the conversation by a short preview and a stable reference, with its full content retrievable later by that reference.
- **Context Thresholds**: The user-adjustable settings governing at what usage fraction the warning state and automatic compaction trigger.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can tell how full their current conversation's context is within one glance at the chat screen, with no more than a few seconds' delay after new content is added.
- **SC-002**: In a scripted long-running conversation that would previously have failed or degraded once its full history exceeded the model's context window, the conversation continues to produce coherent, on-topic responses through at least several times that prior effective limit.
- **SC-003**: At least 90% of conversations that reach the compaction threshold are resolved by the lightweight clearing pass alone, without requiring a model-driven summarization call, when the conversation includes prior tool activity.
- **SC-004**: A tool call producing a very large result increases the conversation's tracked context usage by no more than a small, bounded amount (reflecting a preview), rather than by the full size of that result.
- **SC-005**: Every time compaction occurs, a user reviewing the conversation can identify, without needing to ask, that and when it happened.
- **SC-006**: No user-submitted message is ever lost due to a compaction occurring around the same time it was sent.

## Assumptions

- The active model's total context window is a known, discoverable quantity at the time a conversation is being used (even though today it is hardcoded rather than surfaced) — this feature makes that quantity explicit and load-bearing rather than introducing a new way to determine it.
- "Warning" and "compaction" thresholds default to sensible, pre-set fractions of the context window out of the box (informed by the equivalent industry conventions this feature was inspired by), and the user-adjustability required by FR-013 is about tuning those defaults, not requiring configuration before the feature is usable.
- Summarization (the heavier compaction tier) is performed using the same local model already active in the conversation — no second/auxiliary model is introduced by this feature.
- The tool-output size threshold that triggers offloading (User Story 3) applies only to agent-mode tool results; plain chat mode has no tool results to offload.
- "Retrieval later" of an offloaded tool result's full content (FR-012) reuses the same underlying mechanism the assistant already has for reading file-like content on demand — this feature does not require inventing a wholly new retrieval concept, only extending offloading to a new kind of content.
- This feature governs in-conversation context budget management only; cross-session persistent memory/notes and isolating subagent work into separate context windows are related but explicitly out of scope here, to be considered as potential future, separate features.
- Manual compaction (FR-009) is available at the whole-conversation level; per-message or partial compaction is out of scope.
