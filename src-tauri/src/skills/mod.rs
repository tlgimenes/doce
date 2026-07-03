//! Filesystem skill-pack discovery (User Story 4, FR-020). Skills are not
//! a SQLite entity (`data-model.md`'s `Skill` section) — they're
//! discovered from disk at agent-loop time, matching the `SKILL.md`
//! convention this repository's own `.claude/skills` already uses:
//! a directory containing a `SKILL.md` with a YAML frontmatter block
//! (`---`-delimited) providing at least `name` and `description`.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Frontmatter {
    name: String,
    description: String,
}

/// Scans `dirs` (typically a bundled skills directory and a user skills
/// directory) for immediate subdirectories containing a `SKILL.md` with a
/// parseable frontmatter block. Doesn't recurse past one level — a skill
/// is a top-level folder, not nested arbitrarily deep. Malformed or
/// missing frontmatter is skipped (not an error): one broken skill
/// shouldn't prevent every other skill from loading.
pub fn discover_skills(dirs: &[PathBuf]) -> Vec<SkillInfo> {
    let mut skills = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let skill_md = entry.path().join("SKILL.md");
            if let Some(skill) = parse_skill_md(&skill_md) {
                skills.push(skill);
            }
        }
    }
    skills
}

fn parse_skill_md(path: &Path) -> Option<SkillInfo> {
    let content = std::fs::read_to_string(path).ok()?;
    let frontmatter_yaml = extract_frontmatter(&content)?;
    let frontmatter: Frontmatter = serde_yaml::from_str(&frontmatter_yaml).ok()?;
    Some(SkillInfo {
        name: frontmatter.name,
        description: frontmatter.description,
        path: path.to_path_buf(),
    })
}

/// Extracts the YAML block between the first pair of `---` lines.
fn extract_frontmatter(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut yaml_lines = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            return Some(yaml_lines.join("\n"));
        }
        yaml_lines.push(line);
    }
    None
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

    #[test]
    fn discovers_a_well_formed_skill() {
        let dir = tempdir().unwrap();
        write_skill(
            dir.path(),
            "my-skill",
            "---\nname: my-skill\ndescription: Does a thing\n---\n\nInstructions here.",
        );

        let skills = discover_skills(&[dir.path().to_path_buf()]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "my-skill");
        assert_eq!(skills[0].description, "Does a thing");
    }

    #[test]
    fn skips_a_folder_with_no_skill_md() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("not-a-skill")).unwrap();

        assert!(discover_skills(&[dir.path().to_path_buf()]).is_empty());
    }

    #[test]
    fn skips_malformed_frontmatter_without_failing_the_whole_scan() {
        let dir = tempdir().unwrap();
        write_skill(dir.path(), "broken", "no frontmatter here at all");
        write_skill(
            dir.path(),
            "good",
            "---\nname: good\ndescription: Works fine\n---\n",
        );

        let skills = discover_skills(&[dir.path().to_path_buf()]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "good");
    }

    #[test]
    fn merges_skills_from_multiple_directories() {
        let bundled = tempdir().unwrap();
        let user = tempdir().unwrap();
        write_skill(
            bundled.path(),
            "bundled-one",
            "---\nname: bundled-one\ndescription: A\n---\n",
        );
        write_skill(
            user.path(),
            "user-one",
            "---\nname: user-one\ndescription: B\n---\n",
        );

        let skills = discover_skills(&[bundled.path().to_path_buf(), user.path().to_path_buf()]);
        assert_eq!(skills.len(), 2);
    }

    #[test]
    fn nonexistent_directory_is_skipped_not_an_error() {
        let skills = discover_skills(&[PathBuf::from("/does/not/exist/at/all")]);
        assert!(skills.is_empty());
    }
}
