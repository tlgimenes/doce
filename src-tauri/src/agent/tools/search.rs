use grep_matcher::LineTerminator;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, thiserror::Error)]
pub enum SearchToolError {
    #[error("invalid glob pattern: {0}")]
    Glob(#[from] glob::PatternError),
    #[error("invalid regex pattern: {0}")]
    Regex(#[from] grep_regex::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

const GLOB_RESULT_CAP: usize = 100;

/// Public (unlike `GLOB_RESULT_CAP`) because `dispatch`'s Grep arm names
/// this number in the truncation notice it appends to `model_text` — the
/// message and the behavior must not drift apart.
pub const GREP_RESULT_CAP: usize = 100;

/// Per-file size ceiling for `Grep`. Found necessary in production: a Grep
/// over a conversation with no workspace cwd defaulted to the user's home
/// directory, where the old `read_to_string`-based implementation tried to
/// slurp a 64GB Docker.raw disk image into memory and wedged the whole
/// agent turn (and, via the global inference lock, the app). Matches the
/// frontend's own 10MB `ATTACHMENT_MAX_BYTES` convention for "too big to
/// be a useful text payload".
pub const GREP_MAX_FILE_LEN: u64 = 10 * 1024 * 1024;

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

/// What a `grep` run found, plus honesty signals about what it deliberately
/// didn't look at — so a caller can tell "complete results" apart from
/// "capped/partial results" instead of presenting a prefix as the whole
/// truth (the same reasoning as the `Read` tool's `truncated` flag).
#[derive(Debug, Clone)]
pub struct GrepOutcome {
    pub matches: Vec<GrepMatch>,
    /// True only when more matches genuinely existed beyond
    /// `GREP_RESULT_CAP` — exactly-at-the-cap complete results stay
    /// `false` (the search runs until one match past the cap before
    /// stopping, so this is exact, not a `len == cap` guess).
    pub truncated: bool,
    /// Files skipped without being opened because they exceed
    /// `GREP_MAX_FILE_LEN` — a match inside one of these would never be
    /// found, which "No matches found" alone would misrepresent.
    pub skipped_oversized: usize,
}

/// `Grep` (research.md's tool contract table): ripgrep-style regex search,
/// respecting `.gitignore` (via the `ignore` crate's walker — the same
/// crate ripgrep itself is built on).
///
/// The per-file search runs on ripgrep's own engine (`grep-searcher` +
/// `grep-regex`, same author as `ignore`) rather than a hand-rolled
/// `read_to_string` loop, specifically for its resource bounds: it streams
/// line-by-line without ever materializing the whole file, and
/// `BinaryDetection::quit` abandons a file at the first NUL byte. Two caps
/// of this function's own on top: files over `GREP_MAX_FILE_LEN` are
/// skipped outright (checked via metadata, before opening), and the walk
/// stops entirely once `GREP_RESULT_CAP` matches have accumulated —
/// mirroring `Glob`'s existing 100-result cap.
pub fn grep(
    pattern: &str,
    base: &Path,
    glob_filter: Option<&str>,
) -> Result<GrepOutcome, SearchToolError> {
    // `multi_line(true)` + `crlf(true)` on the matcher AND
    // `LineTerminator::crlf()` on the searcher is ripgrep's own `--crlf`
    // configuration, needed as a set: without it, `$`-anchored patterns
    // silently stop matching lines that end in `\r\n` (the old
    // `str::lines()`-based implementation stripped the `\r`, so they used
    // to match) — and setting only one half of the pair is worse than
    // neither, because the searcher rejects a matcher whose line
    // terminator disagrees with its own and that error is swallowed below,
    // silently producing zero matches for everything.
    let matcher = RegexMatcherBuilder::new()
        .multi_line(true)
        .crlf(true)
        .build(pattern)?;
    let glob_matcher = glob_filter.map(glob::Pattern::new).transpose()?;
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .line_number(true)
        .line_terminator(LineTerminator::crlf())
        .build();

    let mut results = Vec::new();
    let mut skipped_oversized = 0usize;
    // `require_git(false)`: honor a `.gitignore` file present in the
    // workspace even if it isn't (yet) an actual git repository — the
    // `ignore` crate otherwise silently skips all gitignore processing
    // without a `.git` directory, which would make this "respects
    // .gitignore" claim false for a freshly-created project.
    let walker = ignore::WalkBuilder::new(base).require_git(false).build();
    for entry in walker {
        // `>` (one past the cap), not `>=`: the search deliberately runs
        // until it has proof of a 101st match, so `truncated` below is
        // exact rather than a "len == cap" guess that would falsely flag
        // an exactly-100-match complete result.
        if results.len() > GREP_RESULT_CAP {
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if let Some(filter) = &glob_matcher {
            let name = entry.file_name().to_string_lossy();
            if !filter.matches(&name) {
                continue;
            }
        }
        // Metadata failure counts as "skip": a file whose size can't even
        // be stat'd is not worth opening blind.
        match entry.metadata() {
            Ok(m) if m.len() > GREP_MAX_FILE_LEN => {
                skipped_oversized += 1;
                continue;
            }
            Ok(_) => {}
            Err(_) => continue,
        }

        let path = entry.path();
        // Per-file search errors are deliberately non-fatal: an unreadable
        // file contributes nothing, and matches already collected are
        // kept. Note the UTF8 sink only validates lines that *match* — a
        // file with invalid UTF-8 confined to non-matching lines is still
        // fully searched, and an invalid matched line aborts only the
        // remainder of that one file (keeping its earlier matches). That's
        // deliberately more useful than the old read_to_string
        // implementation, which dropped such files entirely.
        let _ = searcher.search_path(
            &matcher,
            path,
            UTF8(|line_number, line| {
                results.push(GrepMatch {
                    path: path.to_path_buf(),
                    line_number: line_number as usize,
                    line: line.trim_end_matches(['\r', '\n']).to_string(),
                });
                // `false` stops this file's search once the cap is
                // overshot by one; the walk loop's own check above stops
                // the remaining tree.
                Ok(results.len() <= GREP_RESULT_CAP)
            }),
        );
    }

    let truncated = results.len() > GREP_RESULT_CAP;
    results.truncate(GREP_RESULT_CAP);
    Ok(GrepOutcome {
        matches: results,
        truncated,
        skipped_oversized,
    })
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

        let outcome = grep("println", dir.path(), None).unwrap();
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].line_number, 2);
        assert!(outcome.matches[0].line.contains("println"));
    }

    #[test]
    fn grep_respects_gitignore() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.rs\n").unwrap();
        fs::write(dir.path().join("ignored.rs"), "needle").unwrap();
        fs::write(dir.path().join("kept.rs"), "needle").unwrap();

        let outcome = grep("needle", dir.path(), None).unwrap();
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].path.file_name().unwrap(), "kept.rs");
    }

    #[test]
    fn grep_respects_glob_filter() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "needle").unwrap();
        fs::write(dir.path().join("b.txt"), "needle").unwrap();

        let outcome = grep("needle", dir.path(), Some("*.rs")).unwrap();
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].path.file_name().unwrap(), "a.rs");
    }

    #[test]
    fn grep_no_match_returns_empty() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "hello").unwrap();

        assert!(grep("nonexistentpattern", dir.path(), None)
            .unwrap()
            .matches
            .is_empty());
    }

    #[test]
    fn grep_skips_binary_files() {
        // Root cause of a real in-production hang: a Grep over the home
        // directory tried to slurp a 64GB Docker.raw disk image. A file
        // containing NUL bytes is not line-oriented text — matching inside
        // it is never useful, and reading it is arbitrarily expensive.
        let dir = tempdir().unwrap();
        let mut binary = b"needle".to_vec();
        binary.push(0u8);
        binary.extend_from_slice(b"more data with needle inside");
        fs::write(dir.path().join("blob.bin"), &binary).unwrap();
        fs::write(dir.path().join("text.txt"), "a needle in text\n").unwrap();

        let outcome = grep("needle", dir.path(), None).unwrap();
        assert_eq!(outcome.matches.len(), 1, "binary file must be skipped");
        assert_eq!(outcome.matches[0].path.file_name().unwrap(), "text.txt");
    }

    #[test]
    fn grep_skips_files_larger_than_the_size_cap_and_counts_them() {
        let dir = tempdir().unwrap();
        // Just over the 10MB cap, with a match right at the start — if the
        // file is read at all, the match would be found.
        let oversized = format!("needle\n{}", "a".repeat(10 * 1024 * 1024));
        fs::write(dir.path().join("huge.txt"), oversized).unwrap();
        fs::write(dir.path().join("small.txt"), "needle\n").unwrap();

        let outcome = grep("needle", dir.path(), None).unwrap();
        assert_eq!(outcome.matches.len(), 1, "oversized file must be skipped");
        assert_eq!(outcome.matches[0].path.file_name().unwrap(), "small.txt");
        assert_eq!(
            outcome.skipped_oversized, 1,
            "the skip must be counted, not silent"
        );
    }

    #[test]
    fn grep_dollar_anchored_pattern_matches_crlf_line_endings() {
        // Parity with the old implementation, which matched against
        // str::lines() output (\r stripped): `needle$` must match a line
        // ending "\r\n", and the reported line text must not carry the \r.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("crlf.txt"), "has needle\r\nother\r\n").unwrap();

        let outcome = grep("needle$", dir.path(), None).unwrap();
        assert_eq!(
            outcome.matches.len(),
            1,
            "$ must match before \\r\\n, not just \\n"
        );
        assert_eq!(outcome.matches[0].line, "has needle");
    }

    #[test]
    fn grep_caps_total_matches_and_reports_the_truncation() {
        let dir = tempdir().unwrap();
        let many_matches = "needle here\n".repeat(150);
        fs::write(dir.path().join("many.txt"), many_matches).unwrap();

        let outcome = grep("needle", dir.path(), None).unwrap();
        assert_eq!(
            outcome.matches.len(),
            100,
            "matches must be capped like Glob's existing 100-result cap"
        );
        assert!(
            outcome.truncated,
            "hitting the cap with more matches left must be reported"
        );
    }

    #[test]
    fn grep_with_exactly_the_cap_count_is_complete_not_truncated() {
        // "Exactly 100 matches, complete" and "capped at 100, more exist"
        // must be distinguishable — a conservative len == cap heuristic
        // would falsely flag this case.
        let dir = tempdir().unwrap();
        let exactly_cap = "needle here\n".repeat(100);
        fs::write(dir.path().join("exact.txt"), exactly_cap).unwrap();

        let outcome = grep("needle", dir.path(), None).unwrap();
        assert_eq!(outcome.matches.len(), 100);
        assert!(!outcome.truncated, "a complete result set is not truncated");
    }
}
