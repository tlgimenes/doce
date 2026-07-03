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
/// `-rf`/`-fr`/`-Rf`) but doesn't claim to catch every conceivable
/// obfuscation (e.g. a command string built up dynamically at runtime, or
/// hidden behind a quoted string that's never actually executed).
pub fn is_denylisted(command: &str) -> bool {
    let normalized = command.to_lowercase();
    let collapsed: String = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    if is_rm_recursive_force(&collapsed) {
        for target in ["~", "/", "$home"] {
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
        "gpt destroy",
        "fdisk /dev/",
    ];
    if disk_erase_patterns.iter().any(|p| collapsed.contains(p)) {
        return true;
    }

    // `dd` writing to a raw/whole disk device (not a partition-only image
    // file), e.g. `dd if=... of=/dev/disk0` or `of=/dev/rdisk0`.
    if collapsed.contains("dd ")
        && (collapsed.contains("of=/dev/disk") || collapsed.contains("of=/dev/rdisk"))
    {
        return true;
    }

    false
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
/// some unrelated longer path.
fn targets_path(collapsed: &str, target: &str) -> bool {
    collapsed
        .split_whitespace()
        .any(|word| word == target || word == format!("{target}/"))
}

/// `Bash` (FR-009): runs a shell command with no sandboxing beyond the
/// denylist (FR-013 — agent actions are unconfirmed and unrestricted by
/// design). `timeout_ms` defaults to 120s, capped at 600s per
/// research.md's tool contract table.
pub fn run(command: &str, timeout_ms: Option<u64>) -> Result<BashResult, BashError> {
    if is_denylisted(command) {
        return Err(BashError::Denylisted);
    }

    let timeout = Duration::from_millis(timeout_ms.unwrap_or(120_000).min(600_000));

    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

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
        let result = run("echo hello", None).unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn run_captures_nonzero_exit_code() {
        let result = run("exit 7", None).unwrap();
        assert_eq!(result.exit_code, 7);
    }

    #[test]
    fn run_refuses_denylisted_command_before_execution() {
        let err = run("rm -rf ~", None).unwrap_err();
        assert!(matches!(err, BashError::Denylisted));
    }
}
