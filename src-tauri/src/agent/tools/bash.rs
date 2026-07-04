use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct BashResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
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

    // A simple poll loop rather than a dedicated timeout crate: this tool
    // is invoked from within the agent loop, which itself runs on a
    // blocking thread (spawn_blocking), so a coarse poll is an acceptable
    // trade-off for the dependency it avoids.
    let start = std::time::Instant::now();
    let output = loop {
        if let Some(status) = child.try_wait()? {
            let mut stdout = String::new();
            let mut stderr = String::new();
            use std::io::Read;
            if let Some(mut out) = child.stdout.take() {
                let _ = out.read_to_string(&mut stdout);
            }
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut stderr);
            }
            break BashResult {
                stdout,
                stderr,
                exit_code: status.code().unwrap_or(-1),
            };
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            break BashResult {
                stdout: String::new(),
                stderr: format!("command timed out after {}ms", timeout.as_millis()),
                exit_code: -1,
            };
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    Ok(output)
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
