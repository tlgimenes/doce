use crate::skills;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
}

/// FR-020: lists filesystem-based skills discovered from the user's
/// skills directory (`<app data dir>/skills`, one subfolder per skill with
/// a `SKILL.md`) — there's no bundled default-skills directory shipped in
/// this pass, so only user-added skills are found today.
#[tauri::command]
#[specta::specta]
pub fn list_skills(app: AppHandle) -> Result<Vec<SkillSummary>, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let skills_dir = app_data_dir.join("skills");
    let found = skills::discover_skills(&[skills_dir]);
    Ok(found
        .into_iter()
        .map(|s| SkillSummary {
            name: s.name,
            description: s.description,
        })
        .collect())
}
