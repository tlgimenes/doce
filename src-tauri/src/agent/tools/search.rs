use regex::Regex;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, thiserror::Error)]
pub enum SearchToolError {
    #[error("invalid glob pattern: {0}")]
    Glob(#[from] glob::PatternError),
    #[error("invalid regex pattern: {0}")]
    Regex(#[from] regex::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

const GLOB_RESULT_CAP: usize = 100;

/// `Glob` (research.md's tool contract table): sorted by mtime (newest
/// first), capped at 100 results, does not respect `.gitignore` — matches
/// Claude Code's own behavior, where `Glob` is a raw filename-pattern
/// search and `Grep`'s gitignore-awareness is the one that filters noise.
pub fn glob_search(pattern: &str, base: &Path) -> Result<Vec<PathBuf>, SearchToolError> {
    let full_pattern = base.join(pattern);
    let full_pattern_str = full_pattern.to_string_lossy().to_string();

    let mut matches: Vec<(PathBuf, SystemTime)> = glob::glob(&full_pattern_str)?
        .filter_map(|entry| entry.ok())
        .map(|path| {
            let mtime = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (path, mtime)
        })
        .collect();

    matches.sort_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));
    matches.truncate(GLOB_RESULT_CAP);
    Ok(matches.into_iter().map(|(path, _)| path).collect())
}

#[derive(Debug, Clone)]
pub struct GrepMatch {
    pub path: PathBuf,
    pub line_number: usize,
    pub line: String,
}

/// `Grep` (research.md's tool contract table): ripgrep-style regex search,
/// respecting `.gitignore` (via the `ignore` crate's walker — the same
/// crate ripgrep itself is built on).
pub fn grep(
    pattern: &str,
    base: &Path,
    glob_filter: Option<&str>,
) -> Result<Vec<GrepMatch>, SearchToolError> {
    let re = Regex::new(pattern)?;
    let glob_matcher = glob_filter.map(glob::Pattern::new).transpose()?;

    let mut results = Vec::new();
    // `require_git(false)`: honor a `.gitignore` file present in the
    // workspace even if it isn't (yet) an actual git repository — the
    // `ignore` crate otherwise silently skips all gitignore processing
    // without a `.git` directory, which would make this "respects
    // .gitignore" claim false for a freshly-created project.
    let walker = ignore::WalkBuilder::new(base).require_git(false).build();
    for entry in walker {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if let Some(matcher) = &glob_matcher {
            let name = entry.file_name().to_string_lossy();
            if !matcher.matches(&name) {
                continue;
            }
        }

        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for (i, line) in content.lines().enumerate() {
            if re.is_match(line) {
                results.push(GrepMatch {
                    path: entry.path().to_path_buf(),
                    line_number: i + 1,
                    line: line.to_string(),
                });
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn glob_finds_matching_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "").unwrap();
        fs::write(dir.path().join("b.rs"), "").unwrap();
        fs::write(dir.path().join("c.txt"), "").unwrap();

        let results = glob_search("*.rs", dir.path()).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|p| p.extension().unwrap() == "rs"));
    }

    #[test]
    fn glob_sorts_newest_first() {
        let dir = tempdir().unwrap();
        let older = dir.path().join("older.rs");
        fs::write(&older, "").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let newer = dir.path().join("newer.rs");
        fs::write(&newer, "").unwrap();

        let results = glob_search("*.rs", dir.path()).unwrap();
        assert_eq!(results[0], newer);
        assert_eq!(results[1], older);
    }

    #[test]
    fn glob_caps_at_100_results() {
        let dir = tempdir().unwrap();
        for i in 0..150 {
            fs::write(dir.path().join(format!("f{i}.rs")), "").unwrap();
        }

        let results = glob_search("*.rs", dir.path()).unwrap();
        assert_eq!(results.len(), 100);
    }

    #[test]
    fn grep_finds_matching_lines() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("f.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();

        let results = grep("println", dir.path(), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_number, 2);
        assert!(results[0].line.contains("println"));
    }

    #[test]
    fn grep_respects_gitignore() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.rs\n").unwrap();
        fs::write(dir.path().join("ignored.rs"), "needle").unwrap();
        fs::write(dir.path().join("kept.rs"), "needle").unwrap();

        let results = grep("needle", dir.path(), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path.file_name().unwrap(), "kept.rs");
    }

    #[test]
    fn grep_respects_glob_filter() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "needle").unwrap();
        fs::write(dir.path().join("b.txt"), "needle").unwrap();

        let results = grep("needle", dir.path(), Some("*.rs")).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path.file_name().unwrap(), "a.rs");
    }

    #[test]
    fn grep_no_match_returns_empty() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "hello").unwrap();

        assert!(grep("nonexistentpattern", dir.path(), None)
            .unwrap()
            .is_empty());
    }
}
