use std::fs;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{path}: no match found for the given old_string")]
    NoMatch { path: String },
    #[error("{path}: old_string matches {count} times; pass replace_all or give more context to make it unique")]
    AmbiguousMatch { path: String, count: usize },
}

/// Per-line clamp: a single pathological line (minified JS, one-line JSONL
/// record) must not blow through the total cap on its own.
pub const READ_MAX_LINE_CHARS: usize = 2000;
/// Total output cap: ~2k tokens. The marker names the exact `offset` to
/// continue from, so paging never needs guesswork.
pub const READ_MAX_BYTES: usize = 8192;
/// How the byte-cap marker line begins. Shared with `dispatch.rs`'s Read
/// arm so the `truncated` flag can be derived from the output itself
/// without re-reading the file (and can never drift from the marker's
/// actual format).
pub const READ_CAP_MARKER_PREFIX: &str = "[capped at ";

/// `Read` (FR-009): matches Claude Code's own tool — 1-indexed line
/// numbers, `cat -n`-style, with optional offset/limit for large files.
/// Not sandboxed to any workspace (FR-009 explicitly: "without restricting
/// these actions to the opened workspace folder"). Output is bounded
/// (2026-07-09 payload-files design): long lines are clamped and the total
/// is capped with an honest continue-from marker, because Read results are
/// never payload-staged — this truncation is the only thing standing
/// between a huge file and the model's context window.
pub fn read(path: &Path, offset: Option<usize>, limit: Option<usize>) -> Result<String, ToolError> {
    let content = fs::read_to_string(path)?;
    let start = offset.unwrap_or(0);
    let take = limit.unwrap_or(2000);

    let mut out = String::new();
    for (emitted, (i, line)) in content
        .lines()
        .enumerate()
        .skip(start)
        .take(take)
        .enumerate()
    {
        let clamped: String = if line.chars().count() > READ_MAX_LINE_CHARS {
            let head: String = line.chars().take(READ_MAX_LINE_CHARS).collect();
            format!("{head}… [line truncated]")
        } else {
            line.to_string()
        };
        let rendered = format!("{:>6}\t{clamped}\n", i + 1);
        if out.len() + rendered.len() > READ_MAX_BYTES {
            // `emitted` is the 0-indexed position of the line NOT being
            // appended, i.e. the count of lines already emitted — so
            // `start + emitted` is the absolute skip count to resume from.
            let continue_from = start + emitted;
            out.push_str(&format!(
                "{READ_CAP_MARKER_PREFIX}{} bytes — continue with offset={continue_from}]\n",
                out.len()
            ));
            return Ok(out);
        }
        out.push_str(&rendered);
    }
    Ok(out)
}

/// `Write` (FR-009): creates or overwrites a file, creating parent
/// directories as needed.
pub fn write(path: &Path, content: &str) -> Result<(), ToolError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

/// `Edit` (FR-009): exact-match, targeted in-place replacement. Requires
/// `old_string` to be unique in the file unless `replace_all` is set —
/// same contract as Claude Code's own `Edit` tool, chosen specifically so
/// an ambiguous edit fails loudly instead of silently changing the wrong
/// occurrence.
pub fn edit(
    path: &Path,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<(), ToolError> {
    let content = fs::read_to_string(path)?;
    let count = content.matches(old_string).count();

    if count == 0 {
        return Err(ToolError::NoMatch {
            path: path.display().to_string(),
        });
    }
    if count > 1 && !replace_all {
        return Err(ToolError::AmbiguousMatch {
            path: path.display().to_string(),
            count,
        });
    }

    let updated = if replace_all {
        content.replace(old_string, new_string)
    } else {
        content.replacen(old_string, new_string, 1)
    };
    fs::write(path, updated)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn read_numbers_lines_from_one() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");
        fs::write(&file, "alpha\nbeta\ngamma\n").unwrap();

        let out = read(&file, None, None).unwrap();
        assert!(out.contains("     1\talpha"));
        assert!(out.contains("     2\tbeta"));
        assert!(out.contains("     3\tgamma"));
    }

    #[test]
    fn read_respects_offset_and_limit() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");
        fs::write(&file, "a\nb\nc\nd\ne\n").unwrap();

        let out = read(&file, Some(1), Some(2)).unwrap();
        assert!(out.contains("     2\tb"));
        assert!(out.contains("     3\tc"));
        assert!(!out.contains("\ta"));
        assert!(!out.contains("     4\td"));
    }

    #[test]
    fn write_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("nested/deep/f.txt");

        write(&file, "hello").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "hello");
    }

    #[test]
    fn write_overwrites_existing_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");
        fs::write(&file, "old").unwrap();

        write(&file, "new").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
    }

    #[test]
    fn edit_replaces_unique_match() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");
        fs::write(&file, "hello world").unwrap();

        edit(&file, "world", "there", false).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "hello there");
    }

    #[test]
    fn edit_fails_on_no_match() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");
        fs::write(&file, "hello world").unwrap();

        let err = edit(&file, "nonexistent", "x", false).unwrap_err();
        assert!(matches!(err, ToolError::NoMatch { .. }));
    }

    #[test]
    fn edit_fails_on_ambiguous_match_without_replace_all() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");
        fs::write(&file, "foo foo foo").unwrap();

        let err = edit(&file, "foo", "bar", false).unwrap_err();
        assert!(matches!(err, ToolError::AmbiguousMatch { count: 3, .. }));
        // Unchanged on failure.
        assert_eq!(fs::read_to_string(&file).unwrap(), "foo foo foo");
    }

    #[test]
    fn edit_replace_all_replaces_every_occurrence() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("f.txt");
        fs::write(&file, "foo foo foo").unwrap();

        edit(&file, "foo", "bar", true).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "bar bar bar");
    }

    #[test]
    fn read_clamps_single_long_lines() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("long.txt");
        std::fs::write(&p, format!("{}\nshort", "a".repeat(5000))).unwrap();
        let out = read(&p, None, None).unwrap();
        let first_line = out.lines().next().unwrap();
        assert!(first_line.len() < 2100, "long line must be clamped");
        assert!(first_line.ends_with("… [line truncated]"));
        assert!(out.contains("short"), "later lines still served");
    }

    #[test]
    fn read_caps_total_bytes_with_a_continue_offset() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("big.txt");
        // 1000 lines x ~30 bytes ≈ 30KB > READ_MAX_BYTES.
        std::fs::write(&p, "0123456789012345678901234\n".repeat(1000)).unwrap();
        let out = read(&p, None, None).unwrap();
        assert!(out.len() <= 8192 + 200, "body bounded (allow marker slack)");
        // The marker names the exact offset to continue from: the number of
        // lines already emitted (offset is a skip count).
        let emitted = out.lines().count() - 1; // minus the marker line
        assert!(out
            .trim_end()
            .ends_with(&format!("continue with offset={emitted}]")));
        // And that offset actually continues where this read stopped.
        let next = read(&p, Some(emitted), None).unwrap();
        assert!(next.starts_with(&format!("{:>6}\t", emitted + 1)));
    }
}
