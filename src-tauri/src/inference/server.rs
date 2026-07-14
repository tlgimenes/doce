//! Process supervisor for the bundled `llama-server` sidecar (llama-server
//! cutover, Task 3.1): assembles the Global-Constraint launch flags, spawns
//! the binary registered in `tauri.conf.json > bundle > externalBin` (Task
//! 1.2), and health-gates the caller on `/health` so nothing POSTs to
//! `http.rs`'s `LlamaServerClient` before the server can actually answer.
//!
//! Lifecycle management beyond one spawn — orphan PID/port-file reaping,
//! wiring this into `InferenceState`, and killing/restarting on model
//! switch — is later tasks (3.2, 3.3). This module only owns the primitive:
//! given a model path, produce a running, healthy server or a clear error.

use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

/// How long [`spawn`] will wait for `/health` to answer 200 before giving up
/// and treating the launch as failed (Task 3.1's health gate). llama-server
/// mmaps the GGUF and warms Metal on startup, which for the cutover's
/// ~2.7GB Q4_K_M model is well under this on real hardware — generous
/// headroom is cheap here since the only cost of waiting is the loading
/// spinner staying up a bit longer, while too tight a budget would flake on
/// a cold-cache first run or a slower machine.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(60);

/// Delay between `/health` polls — frequent enough that a fast-starting
/// server (warm mmap cache) isn't held up waiting on the poll interval, not
/// so frequent that a slow-starting one is spammed with connection-refused
/// requests for a minute straight.
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// The exact `llama-server` argv for one spawn, per the plan's Global
/// Constraints ("Server launch flags (every spawn)"): `--jinja` so the
/// model's own embedded Jinja chat template renders tool calls (not a
/// built-in template guess); `--reasoning-format deepseek` so `<think>`
/// content streams as its own SSE field instead of being spliced into
/// `content`; `--host 127.0.0.1` — LOOPBACK ONLY, never `0.0.0.0`, doce has
/// no auth story for this port; `-np 1` — one parallel slot, matching the
/// harness's one-turn-at-a-time invariant; `--ctx-size 20480` — explicit,
/// never 0 (0 would mean "whatever the GGUF's trained default is," which
/// varies per model and silently changes the context budget the rest of
/// the app assumes); `-ngl 999` — offload every layer to Metal (more than
/// any real model has; llama.cpp clamps to the actual layer count).
///
/// Pure and llama.cpp-free — the exact argv assembly is unit-tested here
/// without spawning a real process (that's Task 8.1's integration test).
pub fn launch_args(port: u16, model_path: &Path) -> Vec<String> {
    vec![
        "--jinja".to_string(),
        "--reasoning-format".to_string(),
        "deepseek".to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        port.to_string(),
        "-np".to_string(),
        "1".to_string(),
        "--ctx-size".to_string(),
        "20480".to_string(),
        "-ngl".to_string(),
        "999".to_string(),
        "-m".to_string(),
        model_path.to_string_lossy().into_owned(),
    ]
}

/// Picks an ephemeral, currently-free TCP port by asking the OS for one:
/// bind a listener to `127.0.0.1:0` (port `0` is the standard "assign me
/// any free port" request), read back whatever port the kernel chose, then
/// drop the listener so the port is free again by the time this returns.
/// There is an inherent (and, in practice, vanishingly small) TOCTOU race
/// between that drop and `spawn` actually binding llama-server to the same
/// port — no different from any other "ask the OS for a free port, hand it
/// to a child process" pattern, and not worth a retry loop for a
/// single-user local app.
pub fn free_port() -> u16 {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral loopback port");
    let port = listener
        .local_addr()
        .expect("read back the OS-assigned port")
        .port();
    drop(listener);
    port
}

/// Broadcast of `spawn`'s progress, emitted on the `server-status` event
/// (frontend consumption is Task 3.3 — for now this is emitted so the event
/// exists on the wire and can be listened for while the rest of the
/// lifecycle wiring lands). `port` is set once a port has been chosen
/// (`starting` onward), `None` only if it were ever emitted before that
/// point (never true today, kept `Option` for a future error variant that
/// doesn't have one).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub state: String,
    pub port: Option<u16>,
}

/// A running, health-checked `llama-server` sidecar. `child` is private —
/// only this module spawns and (on a failed health gate) kills it; a later
/// task (3.2, orphan reaping) is expected to add the shutdown path other
/// modules call through, rather than every caller reaching into the raw
/// `CommandChild` themselves.
pub struct ServerHandle {
    pub base_url: String,
    #[allow(
        dead_code,
        reason = "held for the supervisor's own lifecycle management (graceful shutdown, orphan reaping) — wired up in Task 3.2/3.3, not read within this task's scope"
    )]
    child: CommandChild,
    pub port: u16,
    pub pid: u32,
}

/// Spawns the bundled `llama-server` sidecar on a fresh ephemeral port and
/// blocks until it answers `GET /health` with 200 (or [`HEALTH_TIMEOUT`]
/// elapses), so callers never race a `LlamaServerClient::chat` call against
/// a server that's still mmapping its GGUF. Emits `server-status` at
/// `starting` (port chosen, process launching) and then either `ready`
/// (health check passed) or `error` (spawn failed outright, or the health
/// check timed out — in which case the half-started child is killed here
/// rather than left running as an immediate orphan).
pub async fn spawn(app: &AppHandle, model_path: &Path) -> Result<ServerHandle, String> {
    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");

    // Shared across every `server-status` emit below (`starting`, `ready`,
    // and both `error` paths — the spawn-failure arms just below and the
    // health-timeout branch further down) so the event shape can't drift
    // between call sites and a new failure path can't forget to emit.
    let emit_status = |state: &str| {
        let _ = app.emit(
            "server-status",
            ServerStatus {
                state: state.to_string(),
                port: Some(port),
            },
        );
    };

    emit_status("starting");

    let sidecar_cmd = match app.shell().sidecar("llama-server") {
        Ok(cmd) => cmd,
        Err(e) => {
            // Sidecar binary missing/misregistered — nothing was spawned,
            // but a frontend listener that's been told "starting" needs to
            // hear "error" or it's stuck forever.
            emit_status("error");
            return Err(e.to_string());
        }
    };

    let (mut rx, child) = match sidecar_cmd.args(launch_args(port, model_path)).spawn() {
        Ok(pair) => pair,
        Err(e) => {
            // Same as above: the OS refused to spawn the process at all, so
            // there's no child to kill, but the "starting" state still
            // needs to resolve to "error" for anything listening.
            emit_status("error");
            return Err(e.to_string());
        }
    };
    let pid = child.pid();

    // Drain stdout/stderr on a background task: the sidecar's pipes are
    // bounded, so a supervisor that never reads them risks stalling
    // llama-server's own writes once a pipe fills. Lines are logged for
    // local debugging (llama-server's own startup/model-load diagnostics
    // land on stderr); nothing here feeds back into the health gate below,
    // which polls the HTTP endpoint directly instead of scraping log text.
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) | CommandEvent::Stderr(line) => {
                    eprintln!("[llama-server] {}", String::from_utf8_lossy(&line));
                }
                CommandEvent::Error(err) => {
                    eprintln!("[llama-server] pipe error: {err}");
                }
                CommandEvent::Terminated(payload) => {
                    eprintln!("[llama-server] terminated: {payload:?}");
                    break;
                }
                // `CommandEvent` is `#[non_exhaustive]` — a future
                // tauri-plugin-shell release could add a variant; treat
                // anything unrecognized as a no-op rather than fail to
                // build against it.
                _ => {}
            }
        }
    });

    let http = reqwest::Client::new();
    let health_url = format!("{base_url}/health");
    let deadline = tokio::time::Instant::now() + HEALTH_TIMEOUT;
    loop {
        let healthy = http
            .get(&health_url)
            // Per-request timeout: a hung TCP handshake or stalled response
            // must not itself eat the ~60s health deadline in one bad poll
            // iteration. Well under HEALTH_POLL_INTERVAL's cadence budget
            // and generous for a loopback request to a process on the same
            // machine.
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .is_ok_and(|resp| resp.status().is_success());
        if healthy {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            emit_status("error");
            // Half-started and never going to answer — kill it now rather
            // than leaving an orphaned llama-server holding the port (and
            // GPU memory) with nothing supervising it.
            let _ = child.kill();
            return Err(format!(
                "llama-server (pid {pid}) did not become healthy on {base_url} within {:?}",
                HEALTH_TIMEOUT
            ));
        }
        tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
    }

    emit_status("ready");

    Ok(ServerHandle {
        base_url,
        child,
        port,
        pid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_launch_args_with_loopback_and_explicit_ctx() {
        let args = launch_args(18080, std::path::Path::new("/m.gguf"));
        assert!(args.contains(&"--host".to_string()));
        let host_i = args.iter().position(|a| a == "--host").unwrap();
        assert_eq!(args[host_i + 1], "127.0.0.1");
        assert!(args.iter().any(|a| a == "--jinja"));
        assert!(args.iter().any(|a| a == "--reasoning-format"));
        assert!(args.iter().any(|a| a == "-np"));
        let ctx_i = args.iter().position(|a| a == "--ctx-size").unwrap();
        assert_ne!(args[ctx_i + 1], "0");
        assert!(!args.iter().any(|a| a == "0.0.0.0"));
    }

    #[test]
    fn free_port_returns_nonzero_and_bindable() {
        let port = free_port();
        assert_ne!(port, 0);
        // The listener that discovered `port` was dropped before returning
        // it — confirm it's actually free again, not just a nonzero number.
        let rebound = std::net::TcpListener::bind(("127.0.0.1", port));
        assert!(
            rebound.is_ok(),
            "free_port returned port {port}, which isn't bindable immediately after"
        );
    }
}
