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

/// Byte-window fallback for `truncate_tail_biased` when the line-based
/// head/tail windows can't get under the byte cap: with ≤ ~220 total
/// lines (a handful of very long lines, minified JSON, base64, or one
/// giant unterminated line) head-20 + tail-200 covers EVERY line, so the
/// "capped" text used to pass through whole -- over the cap -- plus a
/// spurious "[0 bytes omitted]" marker. Same tail bias as the line
/// windows, sized so head + tail + marker always land safely under
/// `BASH_OUTPUT_MAX_BYTES` (8KB + 48KB + a short marker line).
const BYTE_FALLBACK_HEAD_BYTES: usize = BASH_OUTPUT_MAX_BYTES / 8;
const BYTE_FALLBACK_TAIL_BYTES: usize = BASH_OUTPUT_MAX_BYTES / 2 + BASH_OUTPUT_MAX_BYTES / 4;

/// Largest index `<= i` that lies on a UTF-8 char boundary of `s`
/// (`str::floor_char_boundary` is still unstable). `i` must be `<= s.len()`.
fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest index `>= i` that lies on a UTF-8 char boundary of `s`.
/// `i` must be `<= s.len()`.
fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Tail-biased truncation used by `dispatch::execute`'s Bash arm for the
/// model-facing `model_text` only (see `BASH_OUTPUT_MAX_BYTES`'s doc
/// comment for why this isn't applied inside `run()`). The cap is HARD:
/// when the line-based windows still exceed it (see
/// `BYTE_FALLBACK_HEAD_BYTES`), plain byte-sliced head/tail windows
/// (snapped to UTF-8 boundaries) bound the result instead.
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
    if kept > BASH_OUTPUT_MAX_BYTES {
        // Line windows weren't enough (this also covers the
        // omitted-would-be-zero passthrough shape: covering every line
        // means `kept ≈ text.len()`, which is over the cap in this
        // branch by construction). `text.len() > BASH_OUTPUT_MAX_BYTES >=
        // head + tail windows`, so `head_end < tail_begin` always holds
        // and `omitted` is always positive -- no marker suppression case
        // survives the fallback.
        let head_end = floor_char_boundary(text, BYTE_FALLBACK_HEAD_BYTES);
        let tail_begin = ceil_char_boundary(text, text.len() - BYTE_FALLBACK_TAIL_BYTES);
        let omitted = tail_begin - head_end;
        return format!(
            "{}\n... [{omitted} bytes omitted -- full output preserved in the conversation transcript]\n{}",
            &text[..head_end],
            &text[tail_begin..]
        );
    }
    let omitted = text.len().saturating_sub(kept);
    format!(
        "{}\n... [{omitted} bytes omitted -- full output preserved in the conversation transcript]\n{}",
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
    // Spawn the child as the leader of its own new process group (pgid ==
    // its own pid) instead of inheriting ours. A compound command --
    // `a && b`, a pipeline, `cmd &` -- forks descendants that inherit that
    // same pgid from their parent (/bin/sh), so signalling the whole group
    // on timeout (see below) reaches them too. Without this, `child.kill()`
    // only ever signals the immediate /bin/sh; any forked descendant
    // survives it as an orphan, still holding an inherited copy of the
    // stdout/stderr pipes' write end open.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
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
    //
    // Each reader sends its result over an mpsc channel rather than only
    // being joinable via its `JoinHandle`, so the timeout path below can
    // bound how long it waits on a reader instead of blocking on it
    // unconditionally -- see that path's comment for why a bare wait isn't
    // safe there even with the process-group kill above. The normal-exit
    // path uses a plain (non-timeout) `recv()`, which is every bit as safe
    // as a `join()` would be there: the child has already exited and
    // closed its own copy of the pipes, so the reader threads are always
    // moments from sending.
    use std::io::Read;
    use std::sync::mpsc;
    let (stdout_tx, stdout_rx) = mpsc::channel::<String>();
    let (stderr_tx, stderr_rx) = mpsc::channel::<String>();
    // The `None` arms are unreachable today (both streams are always piped
    // above), but each tx must still be dropped there: an undropped sender
    // with no reader thread would make the normal-exit path's blocking
    // `recv()` below wait forever on a message that can never come, instead
    // of returning the disconnected-channel default.
    match child.stdout.take() {
        Some(mut out) => {
            std::thread::spawn(move || {
                let mut buf = String::new();
                let _ = out.read_to_string(&mut buf);
                let _ = stdout_tx.send(buf);
            });
        }
        None => drop(stdout_tx),
    }
    match child.stderr.take() {
        Some(mut err) => {
            std::thread::spawn(move || {
                let mut buf = String::new();
                let _ = err.read_to_string(&mut buf);
                let _ = stderr_tx.send(buf);
            });
        }
        None => drop(stderr_tx),
    }

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
            // Signal the whole process group first (see the
            // `process_group(0)` comment above) so a background/forked
            // descendant that outlives the immediate /bin/sh is killed
            // too, not just orphaned.
            #[cfg(unix)]
            {
                let pid = child.id();
                // SAFETY: `libc::kill` is a plain syscall wrapper; passing
                // the negated pid targets the whole process group rather
                // than a single process, which is exactly the documented
                // meaning of a negative pid argument to kill(2).
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            let _ = child.kill();
            // Reaps the killed child so its own pipe descriptors are fully
            // closed -- otherwise the reader threads awaited just below
            // could themselves block waiting for EOF that a lingering
            // zombie's still-open descriptors would never deliver.
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    let (stdout, stderr) = match exit_code {
        Some(_) => {
            // Normal exit: the pipes are already at EOF (the child closed
            // them by exiting), so these are always moments from a send --
            // this doesn't reintroduce the deadlock the comment above
            // describes.
            let stdout = stdout_rx.recv().unwrap_or_default();
            let stderr = stderr_rx.recv().unwrap_or_default();
            (stdout, stderr)
        }
        None => {
            // Timeout: belt-and-suspenders on top of the process-group
            // kill above. Never wait unboundedly on a reader thread here --
            // even with the group kill, a descendant that ignored/blocked
            // SIGKILL (or one the group kill somehow missed, e.g. a
            // process that re-parented into its own new group) can still
            // hold the pipe's write end open, and `read_to_string` only
            // returns once every holder of that write end has actually
            // closed it. Bound the wait instead of trusting it: if a
            // reader doesn't finish within the window, that stream's
            // capture falls back to EMPTY -- each reader sends one
            // complete buffer only at EOF, so an expired `recv_timeout`
            // yields nothing, not a partial read. The thread itself is
            // left running in that case (not aborted) -- it will still
            // exit on its own once the pipe eventually closes, it's just
            // no longer waited on.
            let stdout = stdout_rx
                .recv_timeout(Duration::from_millis(500))
                .unwrap_or_default();
            let stderr = stderr_rx
                .recv_timeout(Duration::from_millis(500))
                .unwrap_or_default();
            (stdout, stderr)
        }
    };

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

    // --- truncate_tail_biased: the byte cap must be HARD (F3, final
    // whole-branch review). Pre-fix, any over-cap output that head-20 +
    // tail-200 lines fully covered (≤ ~220 lines, or one giant line)
    // passed through WHOLE, plus a spurious "[0 bytes omitted]" marker. ---

    #[test]
    fn truncate_hard_caps_a_single_giant_line() {
        // One 100KB unterminated line -- minified JSON / base64 shape.
        let text = "x".repeat(100 * 1024);
        let out = truncate_tail_biased(&text);
        assert!(
            out.len() <= BASH_OUTPUT_MAX_BYTES,
            "the cap must be a real bound, got {} bytes",
            out.len()
        );
        assert!(out.starts_with("x"), "head window preserved");
        assert!(out.ends_with("x"), "tail window preserved");
        assert!(out.contains("bytes omitted"));
        assert!(
            !out.contains("[0 bytes omitted"),
            "the marker must never claim zero omitted bytes on a truncated output"
        );
    }

    #[test]
    fn truncate_hard_caps_a_few_hundred_long_lines() {
        // 200 lines of ~1KB each (~200KB): fewer total lines than
        // HEAD_KEEP_LINES + TAIL_KEEP_LINES, so the line windows cover
        // everything -- the exact pre-fix passthrough shape.
        let long = "y".repeat(1024);
        let text = (0..200)
            .map(|i| format!("{i}:{long}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.len() > BASH_OUTPUT_MAX_BYTES);

        let out = truncate_tail_biased(&text);
        assert!(
            out.len() <= BASH_OUTPUT_MAX_BYTES,
            "line windows covering every line must fall back to the byte cap, got {} bytes",
            out.len()
        );
        assert!(out.starts_with("0:"), "head window keeps the start");
        assert!(out.ends_with(&long), "tail window keeps the end");
        assert!(out.contains("bytes omitted") && !out.contains("[0 bytes omitted"));
    }

    #[test]
    fn truncate_byte_fallback_respects_utf8_boundaries() {
        // 120KB of 2-byte chars on one line: the byte windows land
        // mid-char unless snapped to boundaries -- a raw slice would panic.
        let text = "é".repeat(60 * 1024);
        let out = truncate_tail_biased(&text);
        assert!(out.len() <= BASH_OUTPUT_MAX_BYTES);
        assert!(out.contains("bytes omitted"));
    }

    #[test]
    fn truncate_line_path_still_reports_real_omissions() {
        // Many short lines (the original regression shape) stay on the
        // line-based path: head + tail lines intact, honest marker.
        let text = (0..20000)
            .map(|i| format!("line-{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = truncate_tail_biased(&text);
        assert!(out.contains("line-0\n"), "head lines preserved");
        assert!(out.ends_with("line-19999"), "tail lines preserved");
        assert!(out.contains("bytes omitted") && !out.contains("[0 bytes omitted"));
        assert!(out.len() <= BASH_OUTPUT_MAX_BYTES);
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

    // Review finding (critical): timeout must bound run()'s wall-clock
    // duration even when the command forks a descendant that outlives the
    // immediate /bin/sh -- `sleep 5 & wait` is exactly that shape: `wait`
    // keeps /bin/sh alive (and blocked) until the background `sleep 5`
    // finishes, so the 300ms timeout fires long before either would exit
    // on their own. Before the process-group kill + bounded reader-recv
    // fix, `child.kill()` only reached /bin/sh; the orphaned `sleep`
    // process kept the stdout/stderr pipes' write end open, so the reader
    // threads' `read_to_string` (and a bare `join()` on them) blocked for
    // the remaining ~5s. Bound generously (well under the 5s a hang would
    // take, comfortably above the 300ms timeout) to keep this deterministic
    // under CI/machine load without being a tight timing assertion.
    #[test]
    fn run_timeout_bounds_duration_even_with_a_backgrounded_grandchild() {
        let start = std::time::Instant::now();
        let result = run("sleep 5 & wait", Some(300), None).unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(3),
            "run() should return well within 3s once its 300ms timeout fires, took {elapsed:?}"
        );
        assert_eq!(result.exit_code, -1);
        assert!(
            result.stderr.contains("command timed out"),
            "expected a timeout notice in stderr, got: {:?}",
            result.stderr
        );
    }
}
