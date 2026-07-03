use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AskUserQuestionRequest {
    pub question_id: String,
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

/// `AskUserQuestion` (FR-010): the tool-use loop's pause/resume mechanic.
/// The agent loop registers a `oneshot` receiver here, emits an
/// `ask-user-question` event carrying the same `question_id`, and awaits
/// the receiver — which only resolves once `answer_user_question` (the
/// Tauri command the frontend's prompt UI calls) finds this same id in the
/// registry and sends into it. Deliberately Tauri-agnostic (no
/// `AppHandle`/event emission in here) so the pause/resume contract itself
/// is unit-testable without a running app.
#[derive(Default)]
pub struct PendingQuestions(Mutex<HashMap<String, oneshot::Sender<Vec<String>>>>);

impl PendingQuestions {
    pub fn register(&self, question_id: String) -> oneshot::Receiver<Vec<String>> {
        let (tx, rx) = oneshot::channel();
        self.0.lock().unwrap().insert(question_id, tx);
        rx
    }

    /// Returns `true` if a pending question with this id was found and
    /// answered; `false` if the id is unknown (already answered, or never
    /// registered) — the frontend command surfaces that as an error rather
    /// than silently no-op-ing.
    pub fn answer(&self, question_id: &str, selected: Vec<String>) -> bool {
        if let Some(tx) = self.0.lock().unwrap().remove(question_id) {
            tx.send(selected).is_ok()
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn answering_resolves_the_registered_receiver() {
        let pending = PendingQuestions::default();
        let rx = pending.register("q1".to_string());

        assert!(pending.answer("q1", vec!["Option A".to_string()]));
        let answer = rx.await.unwrap();
        assert_eq!(answer, vec!["Option A".to_string()]);
    }

    #[tokio::test]
    async fn answering_unknown_id_returns_false() {
        let pending = PendingQuestions::default();
        assert!(!pending.answer("nonexistent", vec![]));
    }

    #[tokio::test]
    async fn answering_twice_only_the_first_succeeds() {
        let pending = PendingQuestions::default();
        let _rx = pending.register("q1".to_string());

        assert!(pending.answer("q1", vec!["A".to_string()]));
        assert!(!pending.answer("q1", vec!["B".to_string()]));
    }

    #[tokio::test]
    async fn multiple_pending_questions_are_independent() {
        let pending = PendingQuestions::default();
        let rx1 = pending.register("q1".to_string());
        let rx2 = pending.register("q2".to_string());

        pending.answer("q2", vec!["for q2".to_string()]);
        pending.answer("q1", vec!["for q1".to_string()]);

        assert_eq!(rx1.await.unwrap(), vec!["for q1".to_string()]);
        assert_eq!(rx2.await.unwrap(), vec!["for q2".to_string()]);
    }
}
