const MAX_TITLE_LEN: usize = 60;

/// Auto-generated conversation title (FR-012): truncates the first user
/// message at a word boundary around 60 chars, no model call involved.
/// Collapses internal whitespace/newlines so a multi-line first message
/// doesn't produce a title with embedded line breaks.
pub fn generate_title(first_message: &str) -> String {
    let normalized: String = first_message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = normalized.trim();

    if trimmed.is_empty() {
        return "New conversation".to_string();
    }
    if trimmed.chars().count() <= MAX_TITLE_LEN {
        return trimmed.to_string();
    }

    // Truncate at the last word boundary at-or-before MAX_TITLE_LEN chars,
    // falling back to a hard char-boundary cut if the first "word" alone
    // already exceeds the limit (e.g. one long unbroken token).
    let mut cut = 0;
    let mut last_space = None;
    for (byte_idx, ch) in trimmed.char_indices() {
        let char_count = trimmed[..byte_idx].chars().count();
        if char_count >= MAX_TITLE_LEN {
            break;
        }
        if ch == ' ' {
            last_space = Some(byte_idx);
        }
        cut = byte_idx + ch.len_utf8();
    }

    let truncated = match last_space {
        Some(space_idx) if space_idx > 0 => &trimmed[..space_idx],
        _ => &trimmed[..cut],
    };

    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_message_used_verbatim() {
        assert_eq!(generate_title("hello there"), "hello there");
    }

    #[test]
    fn empty_message_falls_back() {
        assert_eq!(generate_title(""), "New conversation");
        assert_eq!(generate_title("   "), "New conversation");
    }

    #[test]
    fn long_message_truncates_at_word_boundary() {
        let msg = "Can you help me refactor this really long function that has way too many responsibilities crammed into it";
        let title = generate_title(msg);
        assert!(title.ends_with('…'));
        assert!(title.chars().count() <= MAX_TITLE_LEN + 1);

        let body = title.trim_end_matches('…');
        let words: Vec<&str> = msg.split_whitespace().collect();
        // The truncated body must be an exact prefix run of whole words
        // from the original message, not a mid-word cut.
        let mut prefix = String::new();
        let mut matched = false;
        for (i, w) in words.iter().enumerate() {
            if i > 0 {
                prefix.push(' ');
            }
            prefix.push_str(w);
            if prefix == body {
                matched = true;
                break;
            }
        }
        assert!(
            matched,
            "title body {body:?} is not a whole-word prefix of the source message"
        );
    }

    #[test]
    fn collapses_internal_whitespace_and_newlines() {
        let msg = "line one\nline two\n\nline three";
        assert_eq!(generate_title(msg), "line one line two line three");
    }

    #[test]
    fn single_long_token_hard_cuts() {
        let msg = "a".repeat(200);
        let title = generate_title(&msg);
        assert!(title.ends_with('…'));
        assert!(title.chars().count() <= MAX_TITLE_LEN + 1);
    }
}
