//! Shared helpers for the real-model integration suites (`agent_tasks.rs`,
//! `real_model_smoke.rs`). Each `tests/*.rs` file is its own crate, so
//! `mod common;` compiles this module into whichever suite includes it --
//! `#![allow(dead_code)]` because not every suite uses every helper.
//!
//! The one thing both suites need is a REAL `llama-server` to POST at (the
//! llama-server cutover moved generation off the in-process engine onto the
//! HTTP client). Production's `inference::server::spawn` needs an
//! `AppHandle` to run the Tauri sidecar wrapper, which a plain integration
//! test doesn't have -- so `TestServer` runs the SAME built sidecar binary
//! directly, reusing that module's `free_port`/`launch_args`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The built sidecar binary the Tauri bundle ships (`tauri.conf.json`'s
/// `externalBin`), resolved next to this crate's manifest -- the exact
/// binary `inference::server::spawn` launches in production, minus the
/// AppHandle-bound sidecar wrapper a test can't use.
pub fn sidecar_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/llama-server-aarch64-apple-darwin")
}

/// A real `llama-server` spawned for one test, killed on `Drop` so the
/// process dies with the test rather than leaking. Health-gated on spawn so
/// no `LlamaServerClient::chat` ever races a server still mmapping its GGUF,
/// exactly like production's [`inference::server::spawn`].
pub struct TestServer {
    child: std::process::Child,
    pub base_url: String,
}

impl TestServer {
    /// Spawns the sidecar on `model_path` and blocks until `GET /health`
    /// answers 200 (up to 60s, matching production's health gate). Returns
    /// `None` -- printing why -- when the sidecar binary or the model GGUF is
    /// absent, so an `#[ignore]` test invoked without them SKIPS cleanly
    /// rather than panicking (the Qwen3.5 GGUF is normally onboarding-
    /// downloaded and may not be present on a given machine).
    pub async fn spawn(model_path: &Path) -> Option<TestServer> {
        let binary = sidecar_binary();
        if !binary.exists() {
            eprintln!(
                "[test-server] skipping: sidecar binary absent at {}",
                binary.display()
            );
            return None;
        }
        if !model_path.exists() {
            eprintln!(
                "[test-server] skipping: model GGUF absent at {}",
                model_path.display()
            );
            return None;
        }

        let port = doce_lib::inference::server::free_port();
        let args = doce_lib::inference::server::launch_args(port, model_path);
        let child = std::process::Command::new(&binary)
            .args(&args)
            // Silence the sidecar's own stdout/stderr; a test that fails is
            // debugged from its assertions, not a firehose of server logs.
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn llama-server sidecar");
        let mut server = TestServer {
            child,
            base_url: format!("http://127.0.0.1:{port}"),
        };

        let http = reqwest::Client::new();
        let health_url = format!("{}/health", server.base_url);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            let healthy = http
                .get(&health_url)
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .is_ok_and(|resp| resp.status().is_success());
            if healthy {
                return Some(server);
            }
            if tokio::time::Instant::now() >= deadline {
                // Half-started and never going to answer -- kill it now
                // rather than leak an orphan holding the port and GPU memory.
                let _ = server.child.kill();
                panic!(
                    "llama-server did not become healthy on {} within 60s",
                    server.base_url
                );
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Maps a `LlamaServerClient::chat` result into the loop's `TurnOutcome`,
/// mirroring production's `commands::agent::chat_result_to_turn_outcome`: a
/// HARD transport/server failure surfaces as `error` (which run_loop checks
/// FIRST and TERMINATES on) rather than an empty no-tool-call outcome a
/// Require-mode loop would retry forever against a dead server. The
/// benchmark backends never cancel, so -- unlike production -- there is no
/// `Cancelled` arm to special-case: any `Err` is a terminal error.
pub fn chat_outcome_to_turn_outcome(
    result: Result<doce_lib::inference::http::ChatOutcome, doce_lib::inference::InferenceError>,
) -> doce_lib::agent::TurnOutcome {
    match result {
        Ok(o) => doce_lib::agent::TurnOutcome {
            tool_call: o.tool_call,
            text: o.text,
            reasoning: o.reasoning,
            finish_reason: o.finish_reason,
            usage: o.usage,
            error: None,
            cancelled: false,
        },
        Err(e) => {
            let msg = format!("Error: inference failed: {e}");
            doce_lib::agent::TurnOutcome {
                tool_call: None,
                text: msg.clone(),
                reasoning: String::new(),
                finish_reason: String::new(),
                usage: None,
                error: Some(msg),
                cancelled: false,
            }
        }
    }
}
