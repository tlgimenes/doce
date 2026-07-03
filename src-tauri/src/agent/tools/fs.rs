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

/// `Read` (FR-009): matches Claude Code's own tool — 1-indexed line
/// numbers, `cat -n`-style, with optional offset/limit for large files.
/// Not sandboxed to any workspace (FR-009 explicitly: "without restricting
/// these actions to the opened workspace folder").
pub fn read(path: &Path, offset: Option<usize>, limit: Option<usize>) -> Result<String, ToolError> {
    let content = fs::read_to_string(path)?;
    let start = offset.unwrap_or(0);
    let take = limit.unwrap_or(2000);

    let numbered: String = content
        .lines()
        .enumerate()
        .skip(start)
        .take(take)
        .map(|(i, line)| format!("{:>6}\t{line}\n", i + 1))
        .collect();
    Ok(numbered)
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
}
