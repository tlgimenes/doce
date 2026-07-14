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

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use tokio::sync::Mutex;

/// Filename of the crash-safety pidfile written under `app_data_dir` (Task
/// 3.2). `panic = "abort"` is set for release builds (`Cargo.toml`), which
/// means `Drop` does **not** run on a panic — and `llama-server` does not
/// exit on its own when doce's end of a pipe/stdin disappears — so a doce
/// crash can otherwise leave an orphaned `llama-server` holding the model's
/// GGUF mmap and KV cache resident. This file is the backstop: written once
/// a spawn is health-checked, read and reaped on the *next* startup.
const PIDFILE_NAME: &str = "llama-server.pid";

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
        SERVER_CTX_SIZE.to_string(),
        "-ngl".to_string(),
        "999".to_string(),
        "-m".to_string(),
        model_path.to_string_lossy().into_owned(),
    ]
}

/// The llama-server context size (`--ctx-size`) every spawn requests — the
/// single source of truth for the model's total token window. The app's
/// in-process INPUT budget (`inference::CONTEXT_WINDOW_TOKENS`) is derived
/// from this minus `OUTPUT_RESERVE_TOKENS`, so the two can never drift.
pub const SERVER_CTX_SIZE: u32 = 20480;

/// Headroom carved off `SERVER_CTX_SIZE` for the output tokens the server
/// decodes on top of the prompt, so the in-process input budget leaves the
/// server room. Sized so `SERVER_CTX_SIZE - OUTPUT_RESERVE_TOKENS == 16384`
/// (the historical input budget), keeping compaction thresholds unchanged.
pub const OUTPUT_RESERVE_TOKENS: u32 = 4096;

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
    /// The live sidecar process. Private — only this module spawns it, kills
    /// it on a failed health gate (`spawn`), and tears it down on model
    /// switch / graceful exit (`ServerState::shutdown` via
    /// `kill_and_cleanup`). Callers never reach into the raw `CommandChild`.
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

    // Crash-safety backstop (Task 3.2): now that the server is spawned and
    // health-checked, persist its pid+port so a *future* doce startup can
    // find and reap it if this process dies before a graceful shutdown
    // (panic="abort" skips Drop; llama-server itself doesn't exit on a
    // closed stdin). Best-effort — a write failure here shouldn't fail an
    // otherwise-healthy spawn.
    if let Ok(dir) = app.path().app_data_dir() {
        persist_pidfile(&dir, pid, port);
    }

    Ok(ServerHandle {
        base_url,
        child,
        port,
        pid,
    })
}

/// Writes `"<pid>:<port>"` to `<dir>/llama-server.pid`, creating `dir` if it
/// doesn't exist yet. Best-effort: a stale/orphaned sidecar is a much worse
/// outcome than a missing pidfile, but a spawn that already succeeded and
/// passed its health check must not be turned into a failure just because
/// this bookkeeping write hiccuped, so errors are logged and swallowed
/// rather than propagated.
pub fn persist_pidfile(dir: &Path, pid: u32, port: u16) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!(
            "[llama-server] failed to create {} for pidfile: {e}",
            dir.display()
        );
        return;
    }
    let pidfile = dir.join(PIDFILE_NAME);
    if let Err(e) = std::fs::write(&pidfile, format!("{pid}:{port}")) {
        eprintln!(
            "[llama-server] failed to write pidfile {}: {e}",
            pidfile.display()
        );
    }
}

/// Deletes `<dir>/llama-server.pid` if present. Best-effort, same rationale
/// as [`persist_pidfile`] — called on graceful shutdown paths (a later
/// task) so a clean exit doesn't leave a pidfile for the next startup's
/// [`reap_orphan`] to needlessly chase.
pub fn remove_pidfile(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(PIDFILE_NAME));
}

/// The testable core of orphan reaping: given a pidfile path (not
/// necessarily under a real `app_data_dir` — tests point this at a
/// `tempfile::tempdir()`), reap whatever `llama-server` it describes.
///
/// 1. Missing file → nothing to do.
/// 2. Unparseable contents (not `"<pid>:<port>"`) → remove it and stop; a
///    pidfile doce itself can't read back is as good as absent.
/// 3. Liveness probe via `kill(pid, 0)` (sends no signal, just checks the
///    pid exists and is reachable): `ESRCH` means the pid is dead — likely
///    a clean exit that, for whatever reason, didn't reach the
///    [`remove_pidfile`] call — so just remove the file.
/// 4. If alive, guard against **pid reuse**: the OS is free to recycle a
///    dead process's pid for something entirely unrelated by the time doce
///    restarts, so before sending SIGKILL, confirm via `ps -p <pid> -o
///    comm=` that the live process is actually `llama-server` and not some
///    other program that happens to have inherited the number.
/// 5. If it passes that check, `kill(pid, SIGKILL)` — llama-server has no
///    graceful-shutdown handshake worth waiting on here; this only runs at
///    startup, before doce's own server is spawned, so there's nothing this
///    orphan could still be legitimately serving.
/// 6. The pidfile is removed unconditionally at the end (dead pid, reused
///    pid we declined to kill, or one we just killed — in every case the
///    file no longer describes a process this startup should touch again).
pub fn reap_orphan_at(pidfile: &Path) {
    let Ok(contents) = std::fs::read_to_string(pidfile) else {
        return;
    };

    let pid: Option<i32> = contents.trim().split_once(':').and_then(|(pid_s, port_s)| {
        let pid = pid_s.parse::<i32>().ok()?;
        // Parsed only to validate the "pid:port" shape — reap_orphan_at
        // doesn't need the port itself (the ps-based comm= check below
        // is the pid-reuse guard, not a port re-bind check).
        let _port = port_s.parse::<u16>().ok()?;
        Some(pid)
    });

    let Some(pid) = pid else {
        let _ = std::fs::remove_file(pidfile);
        return;
    };

    // SAFETY: `libc::kill` is a plain syscall wrapper. `pid` was parsed from
    // a local file this process wrote in a previous run (or, in the
    // malformed/adversarial case, is still just an integer) — passing
    // signal `0` sends nothing and only probes whether the pid exists and
    // is reachable, per kill(2).
    let probe = unsafe { libc::kill(pid, 0) };
    let dead = probe == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);

    if !dead {
        // Alive (or we lack permission to signal it, which still means it
        // exists) — before touching it, make sure the OS didn't recycle
        // this pid for an unrelated process between the crash and this
        // startup.
        let is_llama_server = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("llama-server"));

        if is_llama_server {
            // SAFETY: same syscall-wrapper rationale as the liveness probe
            // above; this pid has just been confirmed (via `ps`) to be a
            // live `llama-server` process, not one recycled for something
            // else, so SIGKILL only ever targets our own orphan.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }

    let _ = std::fs::remove_file(pidfile);
}

/// Startup entry point (Task 3.2): resolves `<app_data_dir>/llama-server.pid`
/// and reaps whatever it describes via [`reap_orphan_at`]. Wired in
/// `lib.rs`'s `setup`, before any server is spawned for this run, so a
/// leftover orphan from a previous crash is killed before a fresh one could
/// end up sharing the machine with it (and, on memory-constrained hardware,
/// fatally competing for the same GPU memory).
pub fn reap_orphan(app: &AppHandle) {
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    reap_orphan_at(&dir.join(PIDFILE_NAME));
}

/// The one supervised `llama-server` and the model GGUF it was spawned for.
/// `model_path` is what [`plan_action`] compares against a newly-requested
/// model to decide Reuse vs. Restart; the `handle` owns the live child (its
/// private `CommandChild`) so [`ServerState::shutdown`] can kill it.
pub struct RunningServer {
    handle: ServerHandle,
    model_path: PathBuf,
}

/// App-managed (`tauri::manage`) holder for the at-most-one supervised
/// `llama-server` (Task 3.3). Separate from `InferenceState` on purpose: the
/// existing `InferenceEngine` still owns `count_tokens` and today's
/// generation, while this owns *only* the process lifecycle — one server runs
/// whenever a model is active, restarts on a model switch, and is killed on
/// graceful exit. A later task flips generation onto the server and shrinks
/// the engine to vocab-only; until then an idle server may coexist with the
/// engine, which is expected.
#[derive(Default)]
pub struct ServerState(pub Arc<Mutex<Option<RunningServer>>>);

/// What [`ServerState::ensure_running`] should do given the model the
/// supervised server (if any) is currently serving vs. the one just
/// requested. Pure and process-free so the decision is unit-testable without
/// spawning a real server (that's Task 8.1's integration test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerAction {
    /// A server is already up for exactly this model — keep it (no kill,
    /// no respawn: preserves the warm KV cache / loaded weights).
    Reuse,
    /// Nothing is running — launch a server for the requested model.
    Spawn,
    /// A server is up but for a *different* model — tear it down and respawn
    /// on the requested GGUF.
    Restart,
}

/// Decide Reuse/Spawn/Restart from the currently-served model path (if any)
/// and the requested one. `None` → nothing running → [`Spawn`]; same path →
/// [`Reuse`]; different path → [`Restart`]. See the enum variants for why.
///
/// [`Spawn`]: ServerAction::Spawn
/// [`Reuse`]: ServerAction::Reuse
/// [`Restart`]: ServerAction::Restart
pub fn plan_action(current_model: Option<&Path>, requested: &Path) -> ServerAction {
    match current_model {
        None => ServerAction::Spawn,
        Some(m) if m == requested => ServerAction::Reuse,
        Some(_) => ServerAction::Restart,
    }
}

/// Kills the child of a server we're done with and clears its crash-safety
/// pidfile, so the next startup's [`reap_orphan`] doesn't chase a pid we
/// already reaped. Best-effort throughout (`CommandChild::kill` consumes the
/// child; a lost kill or a missing pidfile is not worth failing a
/// switch/shutdown over).
fn kill_and_cleanup(running: RunningServer, app: &AppHandle) {
    let _ = running.handle.child.kill();
    if let Ok(dir) = app.path().app_data_dir() {
        remove_pidfile(&dir);
    }
}

/// Spawn a fresh server for `model_path`, store it in `guard`, and return its
/// base_url. Assumes any previous server has already been taken out of
/// `guard` (Restart) or that there was none (Spawn) — it overwrites whatever
/// is there.
async fn spawn_and_store(
    guard: &mut Option<RunningServer>,
    app: &AppHandle,
    model_path: &Path,
) -> Result<String, String> {
    let handle = spawn(app, model_path).await?;
    let base_url = handle.base_url.clone();
    *guard = Some(RunningServer {
        handle,
        model_path: model_path.to_path_buf(),
    });
    Ok(base_url)
}

impl ServerState {
    /// Ensures a healthy `llama-server` is running for `model_path` and
    /// returns its base_url. Reuses an existing server already serving this
    /// model, spawns one if none is running, or restarts (kill + respawn) if
    /// the running server is on a different model. Holds the state lock across
    /// the (health-gated) spawn so two concurrent callers can't race two
    /// servers into existence for the same model.
    pub async fn ensure_running(
        &self,
        app: &AppHandle,
        model_path: &Path,
    ) -> Result<String, String> {
        let mut guard = self.0.lock().await;
        match plan_action(guard.as_ref().map(|r| r.model_path.as_path()), model_path) {
            ServerAction::Reuse => Ok(guard
                .as_ref()
                .expect("Reuse is only returned when a server is running")
                .handle
                .base_url
                .clone()),
            ServerAction::Restart => {
                if let Some(running) = guard.take() {
                    kill_and_cleanup(running, app);
                }
                spawn_and_store(&mut guard, app, model_path).await
            }
            ServerAction::Spawn => spawn_and_store(&mut guard, app, model_path).await,
        }
    }

    /// Unconditionally tears down the current server (if any) and spawns a
    /// fresh one for `model_path`, returning its base_url. A model-switch
    /// entry point: the caller already knows the active model changed, so
    /// there's no Reuse case to consider here. Currently unreferenced —
    /// Task 6.2 removed the Settings manual model-switch surface that used
    /// to call this when the registry converged on a single model; kept as
    /// the restart primitive for if manual switching returns.
    pub async fn restart(&self, app: &AppHandle, model_path: &Path) -> Result<String, String> {
        let mut guard = self.0.lock().await;
        if let Some(running) = guard.take() {
            kill_and_cleanup(running, app);
        }
        spawn_and_store(&mut guard, app, model_path).await
    }

    /// The base_url of the currently-supervised server, or `None` if none is
    /// running. Unlike `ensure_running`, this never spawns — it only reports
    /// whatever is already up, for callers (the manual `compact_conversation`
    /// command) that need a live server to generate against but have no model
    /// path in hand to launch one.
    pub async fn current_base_url(&self) -> Option<String> {
        let guard = self.0.lock().await;
        guard
            .as_ref()
            .map(|running| running.handle.base_url.clone())
    }

    /// Kills the supervised server, if one is running, and clears its
    /// pidfile. Called on graceful exit (`RunEvent::ExitRequested`) so a clean
    /// shutdown doesn't leave an orphaned `llama-server` holding the model's
    /// GPU memory. Idempotent and best-effort.
    pub async fn shutdown(&self, app: &AppHandle) {
        let mut guard = self.0.lock().await;
        if let Some(running) = guard.take() {
            kill_and_cleanup(running, app);
        }
    }
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
        assert_eq!(args[ctx_i + 1], SERVER_CTX_SIZE.to_string());
        assert!(!args.iter().any(|a| a == "0.0.0.0"));
    }

    #[test]
    fn context_window_tokens_is_derived_from_server_ctx_size() {
        // The in-process INPUT budget must stay coupled to (and derived
        // from) the sidecar's --ctx-size, and the derived value must be
        // unchanged from the historical 16384 so no compaction threshold
        // shifts (20480 - 4096 == 16384).
        assert_eq!(
            SERVER_CTX_SIZE - OUTPUT_RESERVE_TOKENS,
            crate::inference::CONTEXT_WINDOW_TOKENS
        );
        assert_eq!(crate::inference::CONTEXT_WINDOW_TOKENS, 16384);
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

    #[test]
    fn reap_removes_stale_pidfile_and_ignores_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("llama-server.pid");
        std::fs::write(&pidfile, "999999:18080").unwrap(); // pid unlikely to exist
        reap_orphan_at(&pidfile); // no panic; file removed
        assert!(!pidfile.exists());
    }

    #[test]
    fn reap_removes_malformed_pidfile_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("llama-server.pid");
        std::fs::write(&pidfile, "garbage").unwrap();
        reap_orphan_at(&pidfile); // no panic; unparseable contents removed
        assert!(!pidfile.exists());
    }

    #[test]
    fn reap_orphan_at_on_missing_file_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("does-not-exist.pid");
        reap_orphan_at(&pidfile); // no panic, nothing to remove
        assert!(!pidfile.exists());
    }

    #[test]
    fn persist_pidfile_writes_pid_colon_port_and_remove_pidfile_deletes_it() {
        let dir = tempfile::tempdir().unwrap();
        persist_pidfile(dir.path(), 4242, 18080);
        let pidfile = dir.path().join("llama-server.pid");
        let contents = std::fs::read_to_string(&pidfile).unwrap();
        assert_eq!(contents, "4242:18080");

        remove_pidfile(dir.path());
        assert!(!pidfile.exists());
    }

    #[test]
    fn plan_action_none_spawns() {
        // No server running yet → the first active model must launch one.
        assert_eq!(
            plan_action(None, Path::new("/models/a.gguf")),
            ServerAction::Spawn
        );
    }

    #[test]
    fn plan_action_same_model_reuses() {
        // The already-running server is serving exactly this model → keep it;
        // a needless kill+respawn would drop the warmed KV cache for nothing.
        assert_eq!(
            plan_action(
                Some(Path::new("/models/a.gguf")),
                Path::new("/models/a.gguf")
            ),
            ServerAction::Reuse
        );
    }

    #[test]
    fn plan_action_different_model_restarts() {
        // A different active model → the running server is pointed at stale
        // weights and must be torn down and respawned on the new GGUF.
        assert_eq!(
            plan_action(
                Some(Path::new("/models/a.gguf")),
                Path::new("/models/b.gguf")
            ),
            ServerAction::Restart
        );
    }
}
