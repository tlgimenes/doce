use crate::scheduler::Scheduler;
use tauri::State;

/// FR-026: updates the scheduler's live focus state. The frontend calls
/// this on every view change (switching conversations) — the scheduler
/// re-evaluates priority dynamically at pickup time, so a focus change
/// takes effect for whatever's still queued, not just future submissions.
#[tauri::command]
#[specta::specta]
pub fn set_focused_conversation(scheduler: State<'_, Scheduler>, conversation_id: Option<String>) {
    scheduler.set_focused_conversation(conversation_id);
}

/// FR-028: cancels a request whether it's still queued or already
/// executing, preserving whatever partial output was generated so far (the
/// worker persists it as a normal completed message either way).
#[tauri::command]
#[specta::specta]
pub fn cancel_generation(scheduler: State<'_, Scheduler>, request_id: String) -> bool {
    scheduler.cancel(&request_id)
}
