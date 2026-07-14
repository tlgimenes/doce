use crate::hardware::{self, HardwareProfile};
use crate::model_registry;
use crate::storage::DbCell;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
#[specta::specta]
pub fn get_hardware_profile() -> HardwareProfile {
    hardware::detect()
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StartModelInstallResult {
    pub model_id: String,
    pub resumed: bool,
}

/// Tracks model ids with a download currently in flight. Guards against
/// two concurrent `start_model_install` calls for the same model racing on
/// the same `.part` file — found via e2e testing (React StrictMode's
/// deliberate double-invocation of effects in dev doubled the downloaded
/// file size). The frontend also guards against this (Onboarding.tsx), but
/// the backend shouldn't rely on frontend discipline alone for correctness.
#[derive(Default)]
pub struct InFlightDownloads(pub Mutex<HashSet<String>>);

#[tauri::command]
#[specta::specta]
pub async fn start_model_install(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    in_flight: State<'_, InFlightDownloads>,
    model_id: Option<String>,
) -> Result<StartModelInstallResult, String> {
    let conn = db_cell.get(&app).await?.clone();
    let registry = model_registry::bundled();
    let profile = hardware::detect();

    let candidate = if let Some(id) = &model_id {
        registry
            .tiers
            .iter()
            .flat_map(|t| &t.models)
            .find(|m| &m.model_id == id)
            .cloned()
    } else {
        model_registry::best_candidate_for_tier(&registry, &profile.tier).cloned()
    }
    .ok_or_else(|| "no matching model candidate found for this hardware tier".to_string())?;

    {
        let mut guard = in_flight.0.lock().unwrap();
        if !guard.insert(candidate.model_id.clone()) {
            // Already downloading — return the same "in progress" shape
            // rather than starting a second overlapping download.
            return Ok(StartModelInstallResult {
                model_id: candidate.model_id,
                resumed: true,
            });
        }
    }

    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{}.gguf", candidate.model_id));
    let resumed = dest.with_extension("part").exists();

    let model_id_owned = candidate.model_id.clone();

    conn.call({
        let candidate = candidate.clone();
        let tier = profile.tier.clone();
        move |conn: &mut Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT OR IGNORE INTO models (id, hardware_tier, source_url, quantization, sha256, capability_tags, is_active) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
                rusqlite::params![
                    candidate.model_id,
                    tier,
                    candidate.source_url,
                    candidate.quantization,
                    candidate.sha256,
                    serde_json::to_string(&candidate.capability_tags).unwrap_or_default(),
                ],
            )?;
            Ok(())
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    tauri::async_runtime::spawn(async move {
        let result = crate::downloader::download_resumable(
            &app,
            &candidate.model_id,
            &candidate.source_url,
            &dest,
            &candidate.sha256,
        )
        .await;

        match &result {
            Ok(path) => {
                let model_id = candidate.model_id.clone();
                let path_str = path.to_string_lossy().to_string();
                let now = now_ms();
                // Zero-config (FR-001/FR-005): the model onboarding just
                // installed becomes the active one automatically — there is
                // no picker step where the user would otherwise choose.
                // `set_active_model` stays available for the settings-only
                // override (FR-005).
                let _ = conn
                    .call(move |conn: &mut Connection| -> rusqlite::Result<()> {
                        let tx = conn.transaction()?;
                        tx.execute(
                            "UPDATE models SET local_path = ?1, installed_at = ?2 WHERE id = ?3",
                            rusqlite::params![path_str, now, model_id],
                        )?;
                        tx.execute("UPDATE models SET is_active = 0 WHERE is_active = 1", [])?;
                        tx.execute("UPDATE models SET is_active = 1 WHERE id = ?1", [&model_id])?;
                        tx.commit()?;
                        Ok(())
                    })
                    .await;
            }
            Err(e) => {
                // Without this, a failed download/verification left the UI
                // showing "Downloading…"/"Verifying…" forever with no
                // feedback — found via e2e testing hanging indefinitely.
                let _ = app.emit(
                    "model-install-progress",
                    crate::downloader::ModelInstallProgress {
                        model_id: candidate.model_id.clone(),
                        bytes_downloaded: 0,
                        bytes_total: 0,
                        state: format!("error: {e}"),
                    },
                );
            }
        }

        // Whether it succeeded or failed, this model id is no longer
        // "in flight" — a later retry must be able to start a fresh attempt.
        if let Some(in_flight) = app.try_state::<InFlightDownloads>() {
            in_flight.0.lock().unwrap().remove(&candidate.model_id);
        }
    });

    Ok(StartModelInstallResult {
        model_id: model_id_owned,
        resumed,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelInstallStatus {
    pub state: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
}

#[tauri::command]
#[specta::specta]
pub async fn get_model_install_status(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    model_id: String,
) -> Result<ModelInstallStatus, String> {
    let conn = db_cell.get(&app).await?;
    let installed: Option<String> = conn
        .call(
            move |conn: &mut Connection| -> rusqlite::Result<Option<String>> {
                conn.query_row(
                    "SELECT installed_at FROM models WHERE id = ?1",
                    [&model_id],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .map(|v| v.map(|_| "installed".to_string()))
            },
        )
        .await
        .map_err(|e: tokio_rusqlite::Error<rusqlite::Error>| e.to_string())?;

    Ok(ModelInstallStatus {
        state: installed.unwrap_or_else(|| "downloading".to_string()),
        bytes_downloaded: 0,
        bytes_total: 0,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    pub id: String,
    pub hardware_tier: String,
    pub is_active: bool,
    pub installed: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn list_models(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
) -> Result<Vec<ModelRow>, String> {
    let conn = db_cell.get(&app).await?;
    conn.call(|conn: &mut Connection| -> rusqlite::Result<Vec<ModelRow>> {
        let mut stmt =
            conn.prepare("SELECT id, hardware_tier, is_active, installed_at FROM models")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ModelRow {
                    id: row.get(0)?,
                    hardware_tier: row.get(1)?,
                    is_active: row.get::<_, i64>(2)? == 1,
                    installed: row.get::<_, Option<i64>>(3)?.is_some(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn set_active_model(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    inference: State<'_, crate::commands::conversations::InferenceState>,
    model_id: String,
) -> Result<(), String> {
    let conn = db_cell.get(&app).await?;
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        let tx = conn.transaction()?;
        tx.execute("UPDATE models SET is_active = 0 WHERE is_active = 1", [])?;
        tx.execute("UPDATE models SET is_active = 1 WHERE id = ?1", [&model_id])?;
        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;

    // The engine is loaded lazily and cached for the app's lifetime —
    // without this, a switch would keep serving the OLD weights until
    // restart. Awaiting the lock also serializes behind any in-flight
    // turn, so a running generation finishes on the model it started with.
    *inference.0.lock().await = None;
    Ok(())
}

/// One registry model as the Settings "Model" section presents it: the
/// bundled registry entry (deduped across tiers) merged with this
/// install's DB state. `recommended` marks the model
/// `best_candidate_for_tier` would pick for this machine.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AvailableModel {
    pub model_id: String,
    pub quantization: String,
    pub capability_tags: Vec<String>,
    pub recommended: bool,
    pub installed: bool,
    pub active: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn list_available_models(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
) -> Result<Vec<AvailableModel>, String> {
    let registry = model_registry::bundled();
    let profile = hardware::detect();
    let recommended = model_registry::best_candidate_for_tier(&registry, &profile.tier)
        .map(|m| m.model_id.clone());

    let conn = db_cell.get(&app).await?;
    let db_rows: Vec<(String, bool, bool)> = conn
        .call(
            |conn: &mut Connection| -> rusqlite::Result<Vec<(String, bool, bool)>> {
                let mut stmt =
                    conn.prepare("SELECT id, is_active, installed_at IS NOT NULL FROM models")?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get(0)?,
                            row.get::<_, i64>(1)? == 1,
                            row.get::<_, i64>(2)? == 1,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            },
        )
        .await
        .map_err(|e| e.to_string())?;

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for tier in &registry.tiers {
        for candidate in &tier.models {
            if !seen.insert(candidate.model_id.clone()) {
                continue;
            }
            let db = db_rows.iter().find(|(id, _, _)| id == &candidate.model_id);
            out.push(AvailableModel {
                model_id: candidate.model_id.clone(),
                quantization: candidate.quantization.clone(),
                capability_tags: candidate.capability_tags.clone(),
                recommended: recommended.as_deref() == Some(candidate.model_id.as_str()),
                installed: db.map(|(_, _, installed)| *installed).unwrap_or(false),
                active: db.map(|(_, active, _)| *active).unwrap_or(false),
            });
        }
    }
    Ok(out)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
