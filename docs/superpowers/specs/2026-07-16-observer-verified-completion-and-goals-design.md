# Observer-Verified Completion + User Goals — Design

**Date:** 2026-07-16
**Status:** design, pending user review
**Motivator:** tier4 seed 11 dropped to 19/20 because the model marked `bug_04`
done (`TodoDone {index:4}`) after only *reading* it — no edit ever landed — and
FinishTask then hallucinated it as fixed. No current guard catches a *falsely-done*
item (the FinishTask bounce only names *undone* items). The model grades its own
homework. This design removes the self-report.

## Goal (of this design)

1. **Observer-verified completion.** Every completion API call (`TodoDone`,
   `FinishTask`) is adjudicated by a separate observer LLM call that judges the
   claim against accumulated *evidence*, not the model's say-so. The completion
   API's reply tells the model whether the item/task was actually closed.
2. **User goals.** An optional, user-set goal string, injected into a context
   slot as persistent steering, and checked by the observer at FinishTask. When
   the observer confirms a set goal is met, the task auto-finishes.

Non-goals: proactive mid-work halting (observer runs only on completion claims);
decomposing the goal into todos (goal and todos are independent tracks); changing
`run_loop` (sacred — the gate lives in the backend dispatch).

## Decisions (from the design interview)

| # | Decision |
|---|----------|
| Authority | Observer is **authoritative** at both `TodoDone` and `FinishTask`: state changes only if the verdict approves. |
| Cadence | Observer runs on **every completion claim** (`TodoDone` OR `FinishTask`), not per turn. |
| Goal↔todos | **Independent tracks.** Goal is a persistent north-star + a finish criterion; todos stay the model's own working list. |
| Goal lifecycle | **Optional.** No goal → behaves as today. Set goal + observer confirms met → **auto-finish** (honored without a human gate). |
| Reject escape | **Per-item retry cap** (mirrors the one-shot `finish_bounced`): the observer may reject a given completion at most N times; after that the model's claim stands and the disagreement is surfaced. Prevents a wrong 4B verdict from deadlocking the run. |

## Architecture

### 1. Propose → observe → commit (the completion path)

Today `handle_todo_tool` mutates state synchronously and returns
`PlanToolReply::{Reply,Finish}`. An authoritative observer is an **async LLM
call** the sync handler cannot make, and the mutation must not happen until the
verdict approves. So the completion path splits:

- `handle_todo_tool` (sync, `agent/plan.rs`) stops *committing* completions.
  For `TodoDone`/`FinishTask` it returns a **proposal** describing the candidate
  change — a new variant, e.g.
  `PlanToolReply::ProposeComplete { kind: CompletionKind, answer: Option<String> }`
  where `CompletionKind` is `TodoItem(index)` or `FinishTask`. Non-completion
  arms (`Todo` append/grow) still commit synchronously and return `Reply` as now.
- The **backend dispatch** (the three sites: `commands/agent.rs:829`, `:1016`,
  `bench/mod.rs:782`) receives the proposal, calls the observer, then:
  - **approved** → commit (flip the item's `done`, or honor Finish), reply
    confirming closure.
  - **rejected** (under the retry cap) → do **not** commit; reply telling the
    model it is *not* closed and why (the observer's `missing` text).
  - **rejected at/over the cap** → commit anyway (model wins), reply noting the
    unresolved disagreement.
- `run_loop` is untouched: it still sees a tool result string and either a
  continue or a terminal finish. Only the backend's interpretation of the plan
  reply changes.

Both backends call **one shared library helper** (`bench` must stay
production-faithful — no second observer implementation). Home:
`doce_lib::agent::observer` (new module), consumed by `commands/agent.rs` and
`bench/mod.rs` alike.

### 2. The evidence source: an append-only mutation log

The observer judges from **evidence**, and that evidence must survive
compaction and be identical in production and the benchmark. Reading the
conversation fails both (compaction trims the middle; production is DB-backed,
bench is in-memory). Instead, the **backend accumulates an append-only mutation
log** as it *dispatches* tools — it already sees every call and result:

```
struct MutationRecord { tool: String, target: Option<String>, ok: bool }
// e.g. { "Update", Some("…/bug_04.txt"), false }  (edit that didn't match)
//      { "Update", Some("…/bug_08.txt"), true }
```

- Appended in the backend's `execute_tool` path for mutating tools
  (`Update`/Edit, `Write`, `Bash` that writes — start with the file-mutating
  set; read-only tools need not be logged). Never trimmed.
- The log lives beside `plan_state` in each backend (or on `PlanState` itself so
  both backends share the field via the plan machine — TBD in planning, but the
  *append site* is the backend since that is where tools execute).
- Because it is derived from dispatch, not from the message window, it is immune
  to `fit_turn_to_budget` and identical across production/bench.

### 3. The observer call

Input (small, focused — keeps the observer's own window tiny and latency low):

- **For `TodoDone {index:N}`:** the todo text at N + the mutation records whose
  `target` plausibly matches it + the model's most recent action. Verdict:
  "is this item actually done?"
- **For `FinishTask {answer}`:** the goal (if set) + the full todo list + the
  mutation log summary + the answer. Verdict: "is the goal met / is the task
  actually complete?"

Output: a structured verdict `{ complete: bool, missing: String }`. Reuse the
existing forced-tool + grammar mechanism (`tool_choice:"required"` over a
single `Verdict` tool) so the observer must emit a parseable verdict, the same
way the agent is constrained. Observer prompt is a new constant
(`OBSERVER_PROMPT`) — benchmark-gated bytes.

Determinism: the observer call is a normal server request on the single slot,
seeded by `DOCE_GEN_SEED` like every other generation, with `StableToolCallIds`
in the bench path. N extra calls per run = N extra deterministic rolls; the gate
stays byte-reproducible.

### 4. Goals

- **Storage:** a `goal TEXT` column on the `conversations` table (migration).
  Set/cleared via a new Tauri command (`set_conversation_goal`); loaded into
  `PlanState.goal: Option<String>` when a task starts.
- **Injection:** the goal rides in the **tail slot** next to the todo
  recitation (`todo_tail` → generalize to `state_tail`). It is short, stable,
  and in the tail it (a) survives compaction and (b) sits at highest recency
  every turn. The tail must render the goal **even when the todo list is empty**
  (today `todo_tail` returns `""` on an empty plan).
- **Completion:** checked only by the observer at FinishTask (independent of
  todos). Observer confirms goal met → auto-finish (honor without human gate),
  mark goal satisfied, emit a UI event.
- **UI:** a control to set/edit/clear the goal on a conversation. (Frontend
  detail; the parallel session owns four `src/views/**` files — none of them are
  touched by this; a new goal control lands in its own component/command.)

## What this deliberately does NOT change

- `run_loop` (`agent/mod.rs`) — byte-untouched.
- The agent system prompt / `SUMMARIZATION_PROMPT` / `MEMORY_EXTRACTION_PROMPT`
  bytes, except the **intended, benchmark-gated** surfaces this feature adds:
  the new `OBSERVER_PROMPT`, the generalized `state_tail` (todo + goal), and any
  wording change to the `TodoDone`/`FinishTask` replies. Every model-facing byte
  change is gated.
- The append-only `Todo` + granular `TodoDone` contract (541fc7b) stays — the
  observer sits *on top* of it.

## Validation (the built-in test)

seed 11's `bug_04` is the canonical case: the model marks it done with no
successful edit in the mutation log → the observer rejects the `TodoDone` →
the model must actually edit it. Gate questions, refereed by the existing
benchmark:

1. **tier4 recovers seed 11 to 20/20** (observer catches the false completion)
   and holds seeds 22/33 at 20/20.
2. **tier6 holds 14/14** (the drift fix is preserved; observer doesn't regress
   it) — and ideally the goal/observer path helps, not hurts.
3. Determinism: same seed → identical score AND turns, with the observer calls
   in the loop.
4. Cost is reported honestly: observer-call count and added wall-clock per run
   (tier6 already ~45 min; expect materially longer with ~14+ observer calls).

## Open questions for planning

- **Retry cap N.** Start N=2 (one reject + one honored retry, like
  `finish_bounced`), tune empirically. Per-item counter on `PlanState`.
- **Mutation-log home.** On `PlanState` (shared via the plan machine) vs. per
  backend. Prefer `PlanState` so both backends and the observer read one field.
- **Digest matching for `TodoDone`.** How todo text maps to mutation targets
  when a todo doesn't name a file. First cut: pass the whole (small) mutation
  log and let the observer LLM do the matching; narrow later if needed.
- **Bash-as-mutation.** Whether/how to log shell writes. First cut: log Bash
  invocations as `{tool:"Bash", target:None, ok:<exit0>}`; refine if the
  observer needs finer evidence.
- **Cost ceiling.** If per-`TodoDone` observation proves too slow, a fallback is
  FinishTask-only observation (still authoritative, ~1 call/run) — but the
  decision is per-completion, so we build that and measure first.
```
