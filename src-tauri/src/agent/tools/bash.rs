use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct BashResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Independent cap for each of stdout/stderr as they enter the
/// model-facing `model_text` that `agent::dispatch`'s Bash arm builds.
/// Tail-biased: for failures and long build/test logs the signal is at
/// the END; the head is kept for context (the same reasoning as Grep's
/// caps — an unbounded String from a chatty command is both a memory and
/// a context-budget hazard).
///
/// Deliberately NOT applied inside `run()` / `BashResult` itself:
/// `dispatch::execute`'s Bash arm needs the FULL, untruncated
/// stdout/stderr to fold into `detail.outcome.stdout`/`stderr` (what the
/// transcript UI renders and what a human — or a later `Read` — can
/// still recover in full), and only the copy that goes into
/// `model_text` needs to shrink. Truncating here, before `BashResult`
/// exists, would erase the full text before it ever reached `detail` —
/// this way there is exactly one truncation site, at the one place that
/// already builds both `model_text` and `detail` from the same
/// `BashResult`.
pub const BASH_OUTPUT_MAX_BYTES: usize = 65536;
const HEAD_KEEP_LINES: usize = 20;
const TAIL_KEEP_LINES: usize = 200;

/// Tail-biased truncation used by `dispatch::execute`'s Bash arm for the
/// model-facing `model_text` only (see `BASH_OUTPUT_MAX_BYTES`'s doc
/// comment for why this isn't applied inside `run()`).
pub fn truncate_tail_biased(text: &str) -> String {
    if text.len() <= BASH_OUTPUT_MAX_BYTES {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let head: Vec<&str> = lines.iter().take(HEAD_KEEP_LINES).copied().collect();
    // `.min(lines.len())` guards against a pathological input with fewer
    // than `HEAD_KEEP_LINES` lines total (e.g. one giant unterminated
    // line over the byte cap) — without it, `.max(HEAD_KEEP_LINES)` alone
    // could push `tail_start` past `lines.len()` and panic on the slice
    // below.
    let tail_start = lines
        .len()
        .saturating_sub(TAIL_KEEP_LINES)
        .max(HEAD_KEEP_LINES)
        .min(lines.len());
    let tail: Vec<&str> = lines[tail_start..].to_vec();
    let kept: usize = head.iter().chain(tail.iter()).map(|l| l.len() + 1).sum();
    let omitted = text.len().saturating_sub(kept);
    format!(
        "{}\n... [{omitted} bytes omitted -- full output offloaded]\n{}",
        head.join("\n"),
        tail.join("\n")
    )
}

#[derive(Debug, thiserror::Error)]
pub enum BashError {
    #[error("blocked: this command matches a catastrophic, irreversible pattern and cannot be run, with no override (FR-013)")]
    Denylisted,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// research.md §29: a small, fixed set of catastrophic, irreversible
/// command patterns hard-blocked before ever reaching the shell — not a
/// permission gate (FR-013 explicitly keeps agent actions unconfirmed
/// otherwise), just a safety rail against unrecoverable data loss. No
/// prompt, no override.
///
/// This is a pattern denylist, not a full shell-semantics parser — it
/// checks a normalized, tokenized form of the command, which catches
/// straightforward invocations (including combined short flags like
/// `-rf`/`-fr`/`-Rf`, matching-quote-wrapped targets and `bash -c`/`sh
/// -c`/`eval` wrapping, and a token glued directly to a trailing chain
/// operator) but doesn't claim to catch every conceivable obfuscation
/// (e.g. a command string built up dynamically at runtime, or hidden
/// behind a quoted string that's never actually executed).
pub fn is_denylisted(command: &str) -> bool {
    let normalized = command.to_lowercase();
    let collapsed: String = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    if is_rm_recursive_force(&collapsed) {
        let mut targets: Vec<String> = vec!["~".to_string(), "/".to_string(), "$home".to_string()];
        // T092 security pass: the symbolic forms above don't catch a
        // literal reference to the actual resolved home directory (e.g.
        // an agent that already has it in context from an earlier `pwd`),
        // so it's checked too when available.
        if let Ok(home) = std::env::var("HOME") {
            targets.push(home.to_lowercase());
        }
        for target in &targets {
            if targets_path(&collapsed, target) {
                return true;
            }
        }
    }

    let disk_erase_patterns = [
        "diskutil erasedisk",
        "diskutil erasevolume",
        "diskutil partitiondisk",
        "diskutil zerodisk",
        "diskutil secureerase",
        "diskutil apfs deletecontainer",
        "gpt destroy",
        "fdisk /dev/",
    ];
    if disk_erase_patterns.iter().any(|p| collapsed.contains(p)) {
        return true;
    }

    // Low-level format utilities writing directly to a raw disk device —
    // same class/severity as the diskutil/dd checks above.
    let newfs_patterns = ["newfs_hfs", "newfs_apfs", "newfs_msdos"];
    if newfs_patterns.iter().any(|p| collapsed.contains(p)) && collapsed.contains("/dev/") {
        return true;
    }

    // `dd` writing to a raw/whole disk device (not a partition-only image
    // file), e.g. `dd if=... of=/dev/disk0` or `of=/dev/rdisk0` — tokenized
    // and quote-stripped so `of="/dev/disk0"` doesn't slip past a naive
    // substring check.
    if collapsed.contains("dd ") {
        let hits_raw_disk = collapsed.split_whitespace().any(|word| {
            word.strip_prefix("of=").is_some_and(|value| {
                let cleaned = strip_wrapping_quotes(value);
                cleaned.starts_with("/dev/disk") || cleaned.starts_with("/dev/rdisk")
            })
        });
        if hits_raw_disk {
            return true;
        }
    }

    // T092 security pass: `bash -c "..."`/`sh -c "..."`/`zsh -c "..."`/
    // `eval "..."` is a very ordinary way to invoke a nested command, not
    // obfuscation — but the wrapping quote otherwise breaks every
    // leading-token check above (e.g. `is_rm_recursive_force`'s `w ==
    // "rm"` never matches `"rm`). Unwrap and recursively check the inner
    // command instead of trying to special-case every check for it.
    if let Some(inner) = unwrap_shell_c_or_eval(command) {
        if is_denylisted(&inner) {
            return true;
        }
    }

    false
}

/// Unwraps a matching pair of `"..."` or `'...'` around `s`, if present.
fn strip_wrapping_quotes(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Unwraps `bash -c "..."`, `sh -c "..."`, `zsh -c "..."`, or `eval "..."`
/// (single- or double-quoted) to the inner command string, so it can be
/// recursively checked by `is_denylisted` (T092 security pass finding #1).
fn unwrap_shell_c_or_eval(command: &str) -> Option<String> {
    let trimmed = command.trim();
    const PREFIXES: [&str; 4] = ["bash -c ", "sh -c ", "zsh -c ", "eval "];

    for prefix in PREFIXES {
        if trimmed.len() >= prefix.len() && trimmed[..prefix.len()].eq_ignore_ascii_case(prefix) {
            let quoted = trimmed[prefix.len()..].trim();
            let inner = strip_wrapping_quotes(quoted);
            if inner.len() < quoted.len() {
                return Some(inner.to_string());
            }
        }
    }
    None
}

/// True if `collapsed` contains an `rm` invocation with both a recursive
/// flag (`-r`/`-R`/`--recursive`, including combined short-flag clusters
/// like `-rf`) and a force flag (`-f`/`--force`, including combined
/// clusters) as separate whitespace-delimited tokens.
fn is_rm_recursive_force(collapsed: &str) -> bool {
    if !collapsed.split_whitespace().any(|w| w == "rm") {
        return false;
    }

    let mut has_recursive = false;
    let mut has_force = false;
    for word in collapsed.split_whitespace() {
        if word == "--recursive" {
            has_recursive = true;
        }
        if word == "--force" {
            has_force = true;
        }
        if let Some(flags) = word.strip_prefix('-') {
            if !flags.starts_with('-') && !flags.is_empty() {
                // A single-dash short-flag cluster, e.g. "-rf", "-fr".
                if flags.contains('r') {
                    has_recursive = true;
                }
                if flags.contains('f') {
                    has_force = true;
                }
            }
        }
    }
    has_recursive && has_force
}

/// True if `collapsed` contains `target` as a standalone whitespace token
/// (or that token with a trailing slash) — not merely as a substring of
/// some unrelated longer path. Each token has a single layer of wrapping
/// quotes stripped (`"$home"` -> `$home`) and any trailing shell chain
/// operators glued directly onto it stripped (`~;` -> `~`, from `rm -rf
/// ~; echo done` with no space before the `;`) before comparing — T092
/// security pass findings #2/#4.
fn targets_path(collapsed: &str, target: &str) -> bool {
    collapsed.split_whitespace().any(|word| {
        let unquoted = strip_wrapping_quotes(word);
        let cleaned = unquoted.trim_end_matches([';', '&', '|', ')', '"', '\'']);
        cleaned == target || cleaned == format!("{target}/")
    })
}

/// `Bash` (FR-009): runs a shell command with no sandboxing beyond the
/// denylist (FR-013 — agent actions are unconfirmed and unrestricted by
/// design). `timeout_ms` defaults to 120s, capped at 600s per
/// research.md's tool contract table. `cwd` (007-workspace-cwd-resolution)
/// sets the spawned process's starting directory when the conversation
/// has a workspace — just the standard working-directory option already
/// available when spawning a process, not sandboxing: the command itself
/// can still `cd` elsewhere or reference absolute paths, exactly like a
/// real shell opened in that directory would.
pub fn run(
    command: &str,
    timeout_ms: Option<u64>,
    cwd: Option<&std::path::Path>,
) -> Result<BashResult, BashError> {
    if is_denylisted(command) {
        return Err(BashError::Denylisted);
    }

    let timeout = Duration::from_millis(timeout_ms.unwrap_or(120_000).min(600_000));

    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn()?;

    // Drain stdout/stderr on dedicated threads as the child runs, rather
    // than reading them only after it exits: a pipe has a small, fixed OS
    // buffer (commonly 16-64KB) -- a command producing more than that much
    // output before exiting blocks on write() waiting for a reader that,
    // under the old read-after-exit design, never came until the child
    // exited, which it never would while blocked. That's a real deadlock,
    // not a hypothetical: a bash-cap regression test producing ~180KB of
    // stdout (20k short lines) reproduced it directly, hanging until this
    // function's own timeout finally killed the child. Reading
    // concurrently is the fix; the timeout loop below only has to watch
    // for exit/timeout, not also drain output.
    use std::io::Read;
    let stdout_reader = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = out.read_to_string(&mut buf);
            buf
        })
    });
    let stderr_reader = child.stderr.take().map(|mut err| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = err.read_to_string(&mut buf);
            buf
        })
    });

    // A simple poll loop rather than a dedicated timeout crate: this tool
    // is invoked from within the agent loop, which itself runs on a
    // blocking thread (spawn_blocking), so a coarse poll is an acceptable
    // trade-off for the dependency it avoids.
    let start = std::time::Instant::now();
    let exit_code = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status.code().unwrap_or(-1));
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            // Reaps the killed child so its pipes' write ends are fully
            // closed -- otherwise the reader threads joined just below
            // could themselves block waiting for EOF that a lingering
            // zombie's still-open descriptors would never deliver.
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    // Safe to join unconditionally here: on the exit path the pipes are
    // already at EOF (the child closed them by exiting); on the timeout
    // path `child.wait()` above already guaranteed the same. Either way
    // these joins return promptly, they don't reintroduce the deadlock.
    let stdout = stdout_reader
        .and_then(|t| t.join().ok())
        .unwrap_or_default();
    let stderr = stderr_reader
        .and_then(|t| t.join().ok())
        .unwrap_or_default();

    Ok(match exit_code {
        Some(code) => BashResult {
            stdout,
            stderr,
            exit_code: code,
        },
        None => {
            // Unlike the old design, partial output captured before the
            // timeout fired is preserved rather than discarded -- it's
            // real signal (e.g. how far a hung build/test got) that a
            // bare "timed out" notice alone would throw away.
            let notice = format!("command timed out after {}ms", timeout.as_millis());
            let stderr = if stderr.is_empty() {
                notice
            } else {
                format!("{stderr}\n{notice}")
            };
            BashResult {
                stdout,
                stderr,
                exit_code: -1,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_rm_rf_home() {
        assert!(is_denylisted("rm -rf ~"));
        assert!(is_denylisted("rm -rf ~/"));
        assert!(is_denylisted("RM -RF ~"));
        assert!(is_denylisted("rm  -rf   ~"));
    }

    #[test]
    fn blocks_rm_rf_root() {
        assert!(is_denylisted("rm -rf /"));
        assert!(is_denylisted("sudo rm -rf /"));
        assert!(is_denylisted("rm -fr /"));
        assert!(is_denylisted("rm --recursive --force /"));
        assert!(is_denylisted("rm -r -f /"));
    }

    #[test]
    fn blocks_disk_erase_commands() {
        assert!(is_denylisted("diskutil eraseDisk APFS Untitled disk0"));
        assert!(is_denylisted("diskutil partitionDisk disk0 ..."));
        assert!(is_denylisted("gpt destroy disk0"));
    }

    #[test]
    fn blocks_dd_to_raw_disk() {
        assert!(is_denylisted("dd if=/dev/zero of=/dev/disk0 bs=1m"));
        assert!(is_denylisted("dd if=/dev/zero of=/dev/rdisk0"));
    }

    #[test]
    fn does_not_block_dd_to_a_regular_file() {
        assert!(!is_denylisted(
            "dd if=/dev/zero of=/tmp/image.img bs=1m count=10"
        ));
    }

    // Security pass (T092, ahead of v1): all seven gaps below were
    // empirically confirmed as real bypasses within the denylist's own
    // stated scope ("straightforward invocations", not adversarial shell
    // obfuscation) before this fix.
    #[test]
    fn blocks_rm_rf_home_wrapped_in_bash_or_sh_c_or_eval() {
        assert!(is_denylisted("bash -c \"rm -rf ~\""));
        assert!(is_denylisted("sh -c \"rm -rf ~\""));
        assert!(is_denylisted("zsh -c \"rm -rf ~\""));
        assert!(is_denylisted("eval \"rm -rf ~\""));
        assert!(is_denylisted("bash -c 'rm -rf ~'"));
    }

    #[test]
    fn blocks_rm_rf_quoted_home_env_var() {
        // Unlike `'$HOME'` (single-quoted, never expanded by the shell —
        // not a live risk), `"$HOME"` *is* expanded at execution time.
        assert!(is_denylisted("rm -rf \"$HOME\""));
    }

    #[test]
    fn blocks_rm_rf_home_with_no_space_before_a_chained_command() {
        assert!(is_denylisted("rm -rf ~; echo done"));
    }

    #[test]
    fn blocks_rm_rf_the_actual_resolved_home_directory() {
        let home = std::env::var("HOME").expect("HOME must be set to run this test");
        assert!(is_denylisted(&format!("rm -rf {home}")));
        assert!(is_denylisted(&format!("rm -rf {home}/")));
    }

    #[test]
    fn blocks_additional_disk_erase_verbs() {
        assert!(is_denylisted("diskutil secureErase disk0"));
        assert!(is_denylisted("diskutil apfs deleteContainer disk0"));
    }

    #[test]
    fn blocks_newfs_to_a_raw_disk_device() {
        assert!(is_denylisted("newfs_hfs /dev/disk2"));
        assert!(is_denylisted("newfs_apfs /dev/disk2"));
    }

    #[test]
    fn blocks_dd_to_a_quoted_raw_disk_device() {
        assert!(is_denylisted("dd if=/dev/zero of=\"/dev/disk0\" bs=1m"));
    }

    #[test]
    fn does_not_block_symlink_indirection_rm_does_not_recurse_through_a_symlink_target() {
        // Not a real bypass: `rm -rf <symlink>` unlinks the symlink itself,
        // it does not recurse into whatever the symlink points at — a
        // genuine safety property of `rm`, not a denylist gap.
        assert!(!is_denylisted("rm -rf /tmp/some-symlink-to-home"));
    }

    #[test]
    fn does_not_block_ordinary_rm() {
        assert!(!is_denylisted("rm -rf ./build"));
        assert!(!is_denylisted("rm -rf /tmp/scratch-dir"));
        assert!(!is_denylisted("rm somefile.txt"));
        assert!(!is_denylisted("rm -rf node_modules"));
    }

    #[test]
    fn does_not_block_ordinary_commands() {
        assert!(!is_denylisted("ls -la"));
        assert!(!is_denylisted("git status"));
        assert!(!is_denylisted("cargo build"));
    }

    #[test]
    fn run_executes_and_captures_output() {
        let result = run("echo hello", None, None).unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn run_captures_nonzero_exit_code() {
        let result = run("exit 7", None, None).unwrap();
        assert_eq!(result.exit_code, 7);
    }

    #[test]
    fn run_refuses_denylisted_command_before_execution() {
        let err = run("rm -rf ~", None, None).unwrap_err();
        assert!(matches!(err, BashError::Denylisted));
    }

    // 007-workspace-cwd-resolution: the user's own suggested test case —
    // `pwd` (and by extension any relative reference) should reflect the
    // folder passed as `cwd`, not wherever the test process itself happens
    // to be running from.
    #[test]
    fn run_spawns_with_the_given_cwd() {
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        // Resolve symlinks (e.g. macOS's /tmp -> /private/tmp) so the
        // comparison below matches what `pwd` itself reports, which also
        // resolves through any symlinks in the path.
        let canonical = fs::canonicalize(dir.path()).unwrap();

        let result = run("pwd", None, Some(&canonical)).unwrap();
        assert_eq!(result.stdout.trim(), canonical.to_str().unwrap());
    }

    #[test]
    fn run_with_no_cwd_behaves_exactly_as_before() {
        // FR-005 regression guard: omitting cwd must not change behavior.
        let result = run("echo hello", None, None).unwrap();
        assert_eq!(result.stdout.trim(), "hello");
    }
}
