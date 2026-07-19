#![cfg(feature = "bench")]
//! (Whole binary requires the `bench` feature -- it consumes `doce_lib::bench`.
//! Without it, compiles empty so `cargo test --all-targets` (no feature, as CI
//! runs it) stays green; the benchmark protocol always passes `--features bench`.)

//! Real-model smoke test for `agent::observer::request_verdict` (task 4 of
//! observer-verified completion). `#[ignore]`d, `DOCE_BENCH_MODEL`-driven,
//! same pattern as `tests/real_model_smoke.rs`: spins a real `llama-server`
//! via `doce_lib::bench::TestServer` and calls the live observer path
//! `RealBackend`/`SubagentBackend`/`PlanExecBackend` all now go through.
//!
//! This is a SHORT check (two observer calls), not the tier gate -- it only
//! exists to catch an obviously broken prompt/wiring before the expensive
//! benchmark run. Run explicitly via:
//!   cargo test --features bench --test observer_smoke -- --ignored --nocapture

use doce_lib::agent::observer::request_verdict;
use doce_lib::agent::plan::{CompletionKind, MutationRecord, Plan, PlanStep};
use doce_lib::bench::TestServer;
use std::path::PathBuf;

fn installed_model_path() -> PathBuf {
    if let Ok(path) = std::env::var("DOCE_BENCH_MODEL") {
        return PathBuf::from(path);
    }
    let home = std::env::var("HOME").expect("HOME must be set");
    PathBuf::from(home)
        .join("Library/Application Support/app.doce.desktop/models/qwen3.5-4b-q4_k_m.gguf")
}

fn bug_04_plan() -> Plan {
    Plan {
        goal: "Fix the bug in bug_04.txt".to_string(),
        steps: vec![PlanStep {
            description: "Fix bug_04.txt".to_string(),
            done: false,
        }],
    }
}

#[tokio::test]
#[ignore]
async fn the_real_observer_rejects_a_todo_done_claim_with_no_successful_edit() {
    let model = installed_model_path();
    let Some(server) = TestServer::spawn(&model).await else {
        return; // sidecar binary or model GGUF absent -- skip (see TestServer)
    };

    let plan = bug_04_plan();
    // No successful Update to bug_04.txt: one failed attempt plus an
    // unrelated success -- the evidence must not support the claim.
    let log = vec![
        MutationRecord {
            tool: "Update".to_string(),
            target: Some("/repo/bug_04.txt".to_string()),
            ok: false,
        },
        MutationRecord {
            tool: "Update".to_string(),
            target: Some("/repo/bug_01.txt".to_string()),
            ok: true,
        },
    ];

    let verdict = request_verdict(
        &server.base_url,
        &CompletionKind::TodoItem(0),
        &plan,
        &log,
        None,
        Some(&plan.goal),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("observer call should succeed against a live server");

    println!("bug_04 (no successful edit) verdict: {verdict:?}");
    assert!(
        !verdict.complete,
        "observer approved a TodoDone claim for bug_04.txt with no successful edit to it \
         in the mutation log: {verdict:?}"
    );
}

#[tokio::test]
#[ignore]
async fn the_real_observer_approves_a_todo_done_claim_with_a_successful_edit() {
    let model = installed_model_path();
    let Some(server) = TestServer::spawn(&model).await else {
        return; // sidecar binary or model GGUF absent -- skip (see TestServer)
    };

    let plan = bug_04_plan();
    let log = vec![MutationRecord {
        tool: "Update".to_string(),
        target: Some("/repo/bug_04.txt".to_string()),
        ok: true,
    }];

    let verdict = request_verdict(
        &server.base_url,
        &CompletionKind::TodoItem(0),
        &plan,
        &log,
        None,
        Some(&plan.goal),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("observer call should succeed against a live server");

    println!("bug_04 (successful edit) verdict: {verdict:?}");
    assert!(
        verdict.complete,
        "observer rejected a TodoDone claim for bug_04.txt despite a successful edit to it \
         in the mutation log: {verdict:?}"
    );
}
