//! Rich message content (009-rich-chat-input, User Story 2): the
//! structured shape of a user message authored via the rich input when it
//! contains at least one non-plain-text segment (a collapsed paste, a
//! file/image attachment, or a skill mention), plus the single function
//! that derives what the model actually sees from it.
//!
//! A message with only plain text is never wrapped in this shape — it
//! keeps persisting exactly as it does today (`content_type = 'text'`,
//! `content` = the raw string). `RichMessageContent` only exists for
//! messages that actually need it (`data-model.md`'s `RichMessageContent`
//! section).

use crate::skills;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The structured content of a user message, serialized as JSON and
/// stored in `messages.content` when `messages.content_type = 'rich_text'`
/// (`data-model.md`'s Persistence section).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RichMessageContent {
    pub segments: Vec<RichTextSegment>,
}

/// One piece of an authored message, in authoring order (order is
/// meaningful — a skill marker typed mid-sentence stays mid-sentence when
/// expanded for the model, FR-012). Tagged by `type` in JSON to mirror the
/// frontend's discriminated union (`src/lib/ipc.ts`) exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RichTextSegment {
    /// An ordinary run of typed characters between chips.
    Text { text: String },
    /// FR-003's collapsed-paste chip. `text` is the full, uncollapsed
    /// original paste (the model always sees the whole thing);
    /// `line_count` is display-only, computed once at paste time.
    PastedText {
        id: String,
        text: String,
        line_count: usize,
    },
    /// FR-006/FR-007's image/file chip. `data` is base64 (no `data:`
    /// prefix) and is for local rendering/hover-preview only — it must
    /// never be part of any model-facing text (FR-009).
    Attachment {
        id: String,
        name: String,
        mime_type: String,
        data: String,
        is_image: bool,
    },
    /// FR-010's "/" mention. Carries only `name` — content is resolved
    /// fresh from disk at the point of use (see `expand_segments`),
    /// matching FR-014's "can no longer be read at send time" language.
    Skill { id: String, name: String },
}

/// Derives one of the two text representations of `segments` — "what the
/// model sees" (`expand_skills = true`) or "display/title text"
/// (`expand_skills = false`, used only by `generate_title`) — from a
/// `RichMessageContent`'s segments. Both modes share identical
/// `text`/`pastedText`/`attachment` handling; only `skill` differs
/// (`data-model.md`'s Model-Text Expansion section).
///
/// `skills_dir` is a plain parameter rather than resolved internally —
/// this function stays pure/testable and callers resolve the app-data
/// skills directory the same way `commands::skills::list_skills` already
/// does (`app.path().app_data_dir()?.join("skills")`).
///
/// Returns `Err` (not a partially-expanded string) if any `skill`
/// segment's file can't be found or read (FR-014) — the caller must not
/// silently drop a broken skill reference and send an incomplete turn.
pub fn expand_segments(
    segments: &[RichTextSegment],
    skills_dir: &Path,
    expand_skills: bool,
) -> Result<String, String> {
    let mut out = String::new();
    for segment in segments {
        match segment {
            RichTextSegment::Text { text } | RichTextSegment::PastedText { text, .. } => {
                out.push_str(text);
            }
            RichTextSegment::Attachment { name, is_image, .. } => {
                // `data` is deliberately never referenced here (FR-009) —
                // only a placeholder naming the attachment is included.
                if *is_image {
                    out.push_str(&format!("[attached image: {name}]"));
                } else {
                    out.push_str(&format!("[attached file: {name}]"));
                }
            }
            RichTextSegment::Skill { name, .. } => {
                if expand_skills {
                    let content = read_skill_md(skills_dir, name)?;
                    out.push_str(&format!("\n<skill name=\"{name}\">\n{content}\n</skill>\n"));
                } else {
                    out.push_str(&format!("/{name}"));
                }
            }
        }
    }
    Ok(out)
}

/// Resolves `name` against `skills_dir` via the existing
/// `skills::discover_skills` convention (rather than hand-constructing
/// `{skills_dir}/{name}/SKILL.md`) and reads its `SKILL.md` content.
fn read_skill_md(skills_dir: &Path, name: &str) -> Result<String, String> {
    let found = skills::discover_skills(&[skills_dir.to_path_buf()]);
    let skill = found
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("skill '{name}' not found in {}", skills_dir.display()))?;
    std::fs::read_to_string(&skill.path).map_err(|e| format!("failed to read skill '{name}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_skill(dir: &Path, skill_name: &str, content: &str) {
        let skill_dir = dir.join(skill_name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    // --- serde round-trips ---

    #[test]
    fn text_segment_round_trips_through_json() {
        let segment = RichTextSegment::Text {
            text: "hello there".to_string(),
        };
        let json = serde_json::to_value(&segment).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "text", "text": "hello there"})
        );
        let round_tripped: RichTextSegment = serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, segment);
    }

    #[test]
    fn pasted_text_segment_round_trips_through_json() {
        let segment = RichTextSegment::PastedText {
            id: "p1".to_string(),
            text: "line1\nline2\nline3".to_string(),
            line_count: 3,
        };
        let json = serde_json::to_value(&segment).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "pastedText",
                "id": "p1",
                "text": "line1\nline2\nline3",
                "lineCount": 3,
            })
        );
        let round_tripped: RichTextSegment = serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, segment);
    }

    #[test]
    fn attachment_segment_round_trips_through_json() {
        let segment = RichTextSegment::Attachment {
            id: "a1".to_string(),
            name: "photo.png".to_string(),
            mime_type: "image/png".to_string(),
            data: "YmFzZTY0".to_string(),
            is_image: true,
        };
        let json = serde_json::to_value(&segment).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "attachment",
                "id": "a1",
                "name": "photo.png",
                "mimeType": "image/png",
                "data": "YmFzZTY0",
                "isImage": true,
            })
        );
        let round_tripped: RichTextSegment = serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, segment);
    }

    #[test]
    fn skill_segment_round_trips_through_json() {
        let segment = RichTextSegment::Skill {
            id: "s1".to_string(),
            name: "my-skill".to_string(),
        };
        let json = serde_json::to_value(&segment).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "skill", "id": "s1", "name": "my-skill"})
        );
        let round_tripped: RichTextSegment = serde_json::from_value(json).unwrap();
        assert_eq!(round_tripped, segment);
    }

    #[test]
    fn rich_message_content_round_trips_with_all_four_variants_in_order() {
        let content = RichMessageContent {
            segments: vec![
                RichTextSegment::Text {
                    text: "intro ".to_string(),
                },
                RichTextSegment::PastedText {
                    id: "p1".to_string(),
                    text: "pasted body".to_string(),
                    line_count: 1,
                },
                RichTextSegment::Attachment {
                    id: "a1".to_string(),
                    name: "f.txt".to_string(),
                    mime_type: "text/plain".to_string(),
                    data: "ZGF0YQ==".to_string(),
                    is_image: false,
                },
                RichTextSegment::Skill {
                    id: "s1".to_string(),
                    name: "reviewer".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&content).unwrap();
        let round_tripped: RichMessageContent = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, content);
        // Order is meaningful and must survive the round trip.
        assert_eq!(round_tripped.segments, content.segments);
    }

    // --- expand_segments: text/pastedText, both modes ---

    #[test]
    fn expand_segments_concatenates_text_and_pasted_text_verbatim_in_order_when_expanding_skills() {
        let segments = vec![
            RichTextSegment::Text {
                text: "before ".to_string(),
            },
            RichTextSegment::PastedText {
                id: "p1".to_string(),
                text: "the pasted block\nwith newlines".to_string(),
                line_count: 2,
            },
            RichTextSegment::Text {
                text: " after".to_string(),
            },
        ];

        let result = expand_segments(&segments, Path::new("/does/not/matter"), true).unwrap();
        assert_eq!(result, "before the pasted block\nwith newlines after");
    }

    #[test]
    fn expand_segments_concatenates_text_and_pasted_text_verbatim_in_order_when_not_expanding_skills(
    ) {
        let segments = vec![
            RichTextSegment::Text {
                text: "before ".to_string(),
            },
            RichTextSegment::PastedText {
                id: "p1".to_string(),
                text: "the pasted block\nwith newlines".to_string(),
                line_count: 2,
            },
            RichTextSegment::Text {
                text: " after".to_string(),
            },
        ];

        let result = expand_segments(&segments, Path::new("/does/not/matter"), false).unwrap();
        assert_eq!(result, "before the pasted block\nwith newlines after");
    }

    // --- expand_segments: skill ---

    #[test]
    fn expand_segments_inlines_a_real_skills_md_content_when_expanding_skills() {
        let dir = tempdir().unwrap();
        write_skill(
            dir.path(),
            "reviewer",
            "---\nname: reviewer\ndescription: Reviews things\n---\n\nReview instructions here.",
        );

        let segments = vec![
            RichTextSegment::Text {
                text: "please use ".to_string(),
            },
            RichTextSegment::Skill {
                id: "s1".to_string(),
                name: "reviewer".to_string(),
            },
            RichTextSegment::Text {
                text: " for this".to_string(),
            },
        ];

        let result = expand_segments(&segments, dir.path(), true).unwrap();
        assert_eq!(
            result,
            "please use \n<skill name=\"reviewer\">\n---\nname: reviewer\ndescription: Reviews things\n---\n\nReview instructions here.\n</skill>\n for this"
        );
    }

    #[test]
    fn expand_segments_renders_the_literal_marker_without_reading_the_file_when_not_expanding_skills(
    ) {
        // No skill directory is created at all — if this mode tried to
        // read the file, it would error. It must not.
        let dir = tempdir().unwrap();
        let segments = vec![
            RichTextSegment::Text {
                text: "please use ".to_string(),
            },
            RichTextSegment::Skill {
                id: "s1".to_string(),
                name: "reviewer".to_string(),
            },
        ];

        let result = expand_segments(&segments, dir.path(), false).unwrap();
        assert_eq!(result, "please use /reviewer");
    }

    #[test]
    fn expand_segments_returns_err_not_a_partial_string_when_a_skill_cannot_be_found() {
        let dir = tempdir().unwrap();
        // A real skill exists, but is not the one referenced — proves the
        // lookup is by name, not just "any skill in the dir".
        write_skill(
            dir.path(),
            "other-skill",
            "---\nname: other-skill\ndescription: Something else\n---\n",
        );

        let segments = vec![
            RichTextSegment::Text {
                text: "please use ".to_string(),
            },
            RichTextSegment::Skill {
                id: "s1".to_string(),
                name: "missing-skill".to_string(),
            },
        ];

        let result = expand_segments(&segments, dir.path(), true);
        assert!(result.is_err());
    }

    // --- expand_segments: attachment ---

    #[test]
    fn expand_segments_never_includes_attachment_data_and_labels_images_correctly() {
        let segments = vec![RichTextSegment::Attachment {
            id: "a1".to_string(),
            name: "photo.png".to_string(),
            mime_type: "image/png".to_string(),
            data: "THIS_MUST_NEVER_APPEAR_IN_OUTPUT".to_string(),
            is_image: true,
        }];

        for expand_skills in [true, false] {
            let result =
                expand_segments(&segments, Path::new("/does/not/matter"), expand_skills).unwrap();
            assert_eq!(result, "[attached image: photo.png]");
            assert!(!result.contains("THIS_MUST_NEVER_APPEAR_IN_OUTPUT"));
        }
    }

    #[test]
    fn expand_segments_never_includes_attachment_data_and_labels_non_images_correctly() {
        let segments = vec![RichTextSegment::Attachment {
            id: "a1".to_string(),
            name: "notes.txt".to_string(),
            mime_type: "text/plain".to_string(),
            data: "THIS_MUST_NEVER_APPEAR_IN_OUTPUT".to_string(),
            is_image: false,
        }];

        for expand_skills in [true, false] {
            let result =
                expand_segments(&segments, Path::new("/does/not/matter"), expand_skills).unwrap();
            assert_eq!(result, "[attached file: notes.txt]");
            assert!(!result.contains("THIS_MUST_NEVER_APPEAR_IN_OUTPUT"));
        }
    }
}
