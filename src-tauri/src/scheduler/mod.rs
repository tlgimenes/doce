pub mod worker;

use crate::inference::ChatMessage;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

/// A single unit of queued or in-progress model work (research.md §24).
/// `priority_conversation_id` is the conversation whose focus state
/// determines this request's priority — for a subagent's requests this is
/// the *spawning* conversation, not the subagent's own id (research.md §25).
/// `messages` is the full role-tagged conversation so far (system prompt +
/// history), rendered through the model's own chat template by the worker
/// right before generating — see `worker::run_generation`.
pub struct GenerationRequest {
    pub request_id: String,
    pub conversation_id: String,
    pub priority_conversation_id: String,
    pub messages: Vec<ChatMessage>,
    pub assistant_message_id: String,
    pub assistant_created_at: i64,
    pub cancel: CancellationToken,
    pub result_tx: oneshot::Sender<Result<String, String>>,
}

#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct GenerationQueueUpdate {
    pub request_id: String,
    pub conversation_id: String,
    pub state: String,
    pub position: Option<u32>,
}

/// Single-flight scheduler: exactly one generation runs system-wide at any
/// moment. Priority is evaluated dynamically at pickup time (not fixed at
/// submission), so a request's effective priority tracks whatever
/// conversation is currently focused — no anti-starvation mechanism by
/// design (research.md §24, an accepted trade-off documented in spec.md).
pub struct Scheduler {
    queue: Mutex<VecDeque<GenerationRequest>>,
    focused_conversation_id: Mutex<Option<String>>,
    /// (request_id, cancel token) of whatever is currently executing, if
    /// anything — `cancel()` needs this to stop an in-flight generation,
    /// not just one still sitting in the queue.
    current: Mutex<Option<(String, CancellationToken)>>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            focused_conversation_id: Mutex::new(None),
            current: Mutex::new(None),
        }
    }

    pub fn set_focused_conversation(&self, conversation_id: Option<String>) {
        *self.focused_conversation_id.lock().unwrap() = conversation_id;
    }

    pub fn is_focused(&self, priority_conversation_id: &str) -> bool {
        self.focused_conversation_id.lock().unwrap().as_deref() == Some(priority_conversation_id)
    }

    pub fn submit(&self, request: GenerationRequest) {
        self.queue.lock().unwrap().push_back(request);
    }

    /// Picks the next request: any item whose `priority_conversation_id`
    /// matches the currently focused conversation goes first (FIFO within
    /// that tier); otherwise FIFO among the rest. Records it as the
    /// currently-executing request so `cancel()` can reach it.
    pub fn pop_next(&self) -> Option<GenerationRequest> {
        let mut queue = self.queue.lock().unwrap();
        let focused = self.focused_conversation_id.lock().unwrap().clone();

        let popped = if let Some(focused_id) = &focused {
            if let Some(pos) = queue
                .iter()
                .position(|r| &r.priority_conversation_id == focused_id)
            {
                queue.remove(pos)
            } else {
                queue.pop_front()
            }
        } else {
            queue.pop_front()
        };

        if let Some(req) = &popped {
            *self.current.lock().unwrap() = Some((req.request_id.clone(), req.cancel.clone()));
        }
        popped
    }

    pub fn clear_current(&self) {
        *self.current.lock().unwrap() = None;
    }

    /// Cancels a request whether it's still queued or already executing.
    pub fn cancel(&self, request_id: &str) -> bool {
        if let Some((id, token)) = self.current.lock().unwrap().as_ref() {
            if id == request_id {
                token.cancel();
                return true;
            }
        }
        let queue = self.queue.lock().unwrap();
        if let Some(req) = queue.iter().find(|r| r.request_id == request_id) {
            req.cancel.cancel();
            return true;
        }
        false
    }

    pub fn has_pending_for(&self, conversation_id: &str) -> bool {
        self.queue.lock().unwrap().iter().any(|r| {
            r.conversation_id == conversation_id || r.priority_conversation_id == conversation_id
        })
    }

    /// 0-indexed position each still-queued request would be popped in,
    /// given the currently focused conversation — a snapshot, not a
    /// mutation (FR-025/FR-026's "queued" visibility).
    pub fn queue_positions(&self) -> Vec<(String, u32)> {
        let queue = self.queue.lock().unwrap();
        let focused = self.focused_conversation_id.lock().unwrap().clone();

        let mut indices: Vec<usize> = (0..queue.len()).collect();
        if let Some(focused_id) = &focused {
            indices.sort_by_key(|&i| {
                if queue[i].priority_conversation_id == *focused_id {
                    0
                } else {
                    1
                }
            });
        }
        indices
            .into_iter()
            .enumerate()
            .map(|(position, i)| (queue[i].request_id.clone(), position as u32))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(
        id: &str,
        priority_conv: &str,
    ) -> (GenerationRequest, oneshot::Receiver<Result<String, String>>) {
        let (tx, rx) = oneshot::channel();
        (
            GenerationRequest {
                request_id: id.to_string(),
                conversation_id: priority_conv.to_string(),
                priority_conversation_id: priority_conv.to_string(),
                messages: vec![ChatMessage::user("hi")],
                assistant_message_id: format!("{id}-assistant"),
                assistant_created_at: 0,
                cancel: CancellationToken::new(),
                result_tx: tx,
            },
            rx,
        )
    }

    #[test]
    fn fifo_when_nothing_focused() {
        let s = Scheduler::new();
        let (r1, _rx1) = make_request("r1", "c1");
        let (r2, _rx2) = make_request("r2", "c2");
        s.submit(r1);
        s.submit(r2);

        assert_eq!(s.pop_next().unwrap().request_id, "r1");
        assert_eq!(s.pop_next().unwrap().request_id, "r2");
        assert!(s.pop_next().is_none());
    }

    #[test]
    fn focused_conversation_jumps_the_queue() {
        let s = Scheduler::new();
        let (r1, _rx1) = make_request("r1", "c1");
        let (r2, _rx2) = make_request("r2", "c2");
        s.submit(r1);
        s.submit(r2);
        s.set_focused_conversation(Some("c2".to_string()));

        // c2 was submitted second but is focused, so it pops first.
        assert_eq!(s.pop_next().unwrap().request_id, "r2");
        assert_eq!(s.pop_next().unwrap().request_id, "r1");
    }

    #[test]
    fn priority_is_evaluated_at_pickup_not_submission() {
        let s = Scheduler::new();
        let (r1, _rx1) = make_request("r1", "c1");
        s.submit(r1);
        // Focus set AFTER submission — still takes effect, since priority
        // is dynamic at pop time (research.md §24), not fixed at submit.
        s.set_focused_conversation(Some("c1".to_string()));
        assert_eq!(s.pop_next().unwrap().request_id, "r1");
    }

    #[test]
    fn cancel_queued_request_cancels_its_token() {
        let s = Scheduler::new();
        let (r1, _rx1) = make_request("r1", "c1");
        let token = r1.cancel.clone();
        s.submit(r1);

        assert!(s.cancel("r1"));
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_currently_executing_request_also_works() {
        let s = Scheduler::new();
        let (r1, _rx1) = make_request("r1", "c1");
        let token = r1.cancel.clone();
        s.submit(r1);
        let popped = s.pop_next().unwrap();
        assert_eq!(popped.request_id, "r1");

        // Not in the queue anymore, but still cancellable — this is exactly
        // what a bare queue-scan-only `cancel()` would miss.
        assert!(s.cancel("r1"));
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_unknown_request_returns_false() {
        let s = Scheduler::new();
        assert!(!s.cancel("nonexistent"));
    }

    #[test]
    fn queue_positions_reflect_focus() {
        let s = Scheduler::new();
        let (r1, _rx1) = make_request("r1", "c1");
        let (r2, _rx2) = make_request("r2", "c2");
        let (r3, _rx3) = make_request("r3", "c3");
        s.submit(r1);
        s.submit(r2);
        s.submit(r3);
        s.set_focused_conversation(Some("c3".to_string()));

        let positions: std::collections::HashMap<String, u32> =
            s.queue_positions().into_iter().collect();
        assert_eq!(positions["r3"], 0);
        assert_eq!(positions["r1"], 1);
        assert_eq!(positions["r2"], 2);
    }

    #[test]
    fn cancellation_isolated_to_the_targeted_request() {
        let s = Scheduler::new();
        let (r1, _rx1) = make_request("r1", "c1");
        let (r2, _rx2) = make_request("r2", "c2");
        let token1 = r1.cancel.clone();
        let token2 = r2.cancel.clone();
        s.submit(r1);
        s.submit(r2);

        assert!(s.cancel("r1"));
        assert!(token1.is_cancelled());
        assert!(!token2.is_cancelled());
    }
}
