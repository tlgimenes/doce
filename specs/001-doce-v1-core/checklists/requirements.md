# Specification Quality Checklist: Doce v1.0 — Zero-Config Local Personal Agent

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-07-02
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details)
- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded
- [x] Dependencies and assumptions identified

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria
- [x] User scenarios cover primary flows
- [x] Feature meets measurable outcomes defined in Success Criteria
- [x] No implementation details leak into specification

## Notes

- Source material: `docs/superpowers/specs/2026-07-02-doce-design.md` (design spec, retired after
  this feature spec was derived from it).
- Scope intentionally limited to v1.0 launch per the project constitution's scope-discipline
  principle; WhatsApp bridging and other channels are noted as deferred, not omitted.
- All items pass; no clarification questions required — the source design doc was detailed enough
  to fill every section with reasonable, documented defaults (see Assumptions in spec.md).
- 2026-07-02 update: added (then-numbered) User Story 6, FR-021–FR-025, SC-008/SC-009, and a
  Generation Request key entity, covering scheduling/responsiveness behavior across concurrent
  chats and agent tasks (surfaced during technical stack discussion, see `research.md` §24).
  Re-validated against this checklist: all items still pass — the new content is
  implementation-free, testable, and bounded.
- 2026-07-02 update: removed the permission/approval system entirely (formerly User Story 4,
  FR-012–014, SC-004/005, and the Permission Grant entity) per an explicit product decision to
  ship v1.0 fully unrestricted — see constitution v2.0.0 (Principle IV/V renumbering, Principle V
  now documents this trade-off and its required v1.1 revisit). Remaining user stories, FRs, and
  SCs renumbered for contiguity (final at that point: User Stories 1–5, FR-001–023, SC-001–007).
  Re-validated: all checklist items still pass.
- 2026-07-02 update: added subagent-spawning capability to the agent tool-use loop (FR-012/013,
  new SC-008, `Conversation`/`Generation Request` entity updates, `research.md` §25) — modeled on
  Claude Code's own subagent architecture (context-isolated, resumable-by-construction via the
  reused Conversation schema, one-level nesting cap, unbounded turns as a named accepted risk,
  priority inherited from the spawning conversation with no scheduler special-casing). Final
  numbering: FR-001–025, SC-001–008, User Stories 1–5 unchanged. Re-validated: all checklist
  items still pass — the new content is implementation-free (spec.md describes WHAT/WHY; the
  mechanism detail lives in research.md/plan.md/data-model.md as intended).
- 2026-07-02 update: added local conversation search (new User Story 6, FR-026/FR-027, SC-009)
  during a SQLite schema discussion — backed by SQLite FTS5 (`data-model.md`'s "Search" section,
  `research.md` §26), explicitly excluding subagent-run conversations from indexing to preserve
  their isolation guarantee. Also formalized several schema-level conventions during the same
  discussion (UUIDv7 primary keys, `INTEGER` epoch-millisecond timestamps, explicit `WAL`/
  `foreign_keys` pragmas, `PRAGMA user_version`-based migrations, a `content_type` discriminator
  on `Message`, a partial-unique-index invariant on `Model.is_active`) — amended into `research.md`
  §4 and `data-model.md`'s new "Schema conventions" section rather than added as spec-level
  requirements, since they're HOW, not WHAT/WHY. Final numbering (at that point): FR-001–027,
  SC-001–009, User Stories 1–6. Re-validated: all checklist items still pass.
- 2026-07-02 update: added a UI-driven set of capabilities from a design discussion (conversation
  list mockup) — an `AskUserQuestion` built-in tool (exact Claude Code parity, per the earlier
  tools research and a standing prior-session decision), a live-computed `Conversation.status`
  (`done`/`requires_action`/`failed`/`in_progress`, new FR-011, SC-010) with a precise
  `requires_action` rule (ends in `AskUserQuestion` or a trailing `?` outside a URL), and
  title-by-truncation (FR-012, no LLM call). Also took the opportunity to enumerate Doce's full
  v1.0 built-in tool set by exact name/signature (`Read`/`Write`/`Edit`/`Bash`/`Glob`/`Grep`/
  `AskUserQuestion`, `research.md` §27) instead of leaving FR-009 vague, per the standing
  tool-parity decision. New User Story 7. `research.md` §28 covers the status/title design;
  `data-model.md` gains a `Message.tool_name` column and a computed-`status` note;
  `contracts/tauri-ipc.md` gains `answer_user_question`/`ask-user-question`. Final numbering:
  FR-001–030, SC-001–010, User Stories 1–7. Re-validated: all checklist items still pass — swept
  cross-file for FR renumbering consistency (3 new FRs inserted after FR-009 shifted everything
  after it by +3).
- 2026-07-02 update: ran an adversarial 8-critic review (duplication, correctness, security,
  performance, testing, architecture, scope, documentation) via the review-plan skill before
  `/speckit-tasks`. Adopted fixes: 3 stale FR cross-references, a corrected `gbnf` crate write-up,
  a resolved KV-cache session-state question (research.md §24 — no longer a spike), a new `error`
  `content_type` on `Message`, a resolved active-model dual-source-of-truth (`Model.is_active` is
  authoritative), a fixed `requires_action` contains-vs-ends-in contradiction (now "ends in,"
  restricted to assistant-authored messages only), a 30-turn cap on subagents (FR-016), and a
  narrow hardcoded catastrophic-command denylist on `Bash` (FR-013, new SC-011, research.md §29).
  Full adopted/rejected/adapted/not-addressed accounting in `research.md`'s new "Critique
  Decisions" section. Final numbering: FR-001–030 (no shift — new content merged into existing
  FR-013/016 rather than inserted), SC-001–011, User Stories 1–7 unchanged. Re-validated: all
  checklist items still pass.
- 2026-07-02 update: tightened the testing strategy (research.md §9) with concrete scheduler test
  scenarios, a named fault-injection mechanism (`wiremock`) for download-resume tests, a direct
  FTS5-trigger test, and clarified that `quickstart.md` sections map 1:1 to WDIO e2e spec files
  rather than being a separate manual checklist. Added `research.md` §30 (Continuous Integration)
  and created `.github/workflows/ci.yml` — three jobs (rust/frontend/e2e) on `macos-26` runners
  (Apple Silicon-native; `macos-14` is being deprecated in 2026), gated on every push/PR. This is
  infrastructure config, not a spec-level FR — no spec.md changes, consistent with how the rest of
  the testing stack (Vitest, WebdriverIO) was handled at the research/plan level only.
