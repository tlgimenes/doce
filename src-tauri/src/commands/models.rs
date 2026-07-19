use crate::downloader::{self, ModelInstallProgress};
use crate::hardware::{self, HardwareProfile};
use crate::model_registry::{self, ModelCandidate, Registry};
use crate::storage::DbCell;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

const SELECTED_MODEL_SETTING: &str = "model.selectedId";
const FALLBACK_NOTICE_SETTING: &str = "model.fallbackNotice";
const LOCAL_SOURCE_KIND: &str = "local";
const CURATED_SOURCE_KIND: &str = "curated";

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

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelInstallStatus {
    pub state: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
}

/// Process-local download snapshots plus the in-flight set used to dedupe
/// StrictMode, Settings remounts, retries, and fallback recovery. The durable
/// selected model still lives in SQLite; this state only describes work that
/// is currently running (or its most recent terminal result).
#[derive(Default, Clone)]
pub struct ModelSelectionState {
    in_flight: Arc<Mutex<HashSet<String>>>,
    progress: Arc<Mutex<HashMap<String, ModelInstallProgress>>>,
    intent_gate: Arc<tokio::sync::Mutex<()>>,
}

impl ModelSelectionState {
    pub(crate) fn record_progress(&self, progress: ModelInstallProgress) {
        self.progress
            .lock()
            .unwrap()
            .insert(progress.model_id.clone(), progress);
    }

    fn progress_for(&self, model_id: &str) -> Option<ModelInstallProgress> {
        self.progress.lock().unwrap().get(model_id).cloned()
    }

    fn begin(&self, model_id: &str) -> bool {
        self.in_flight.lock().unwrap().insert(model_id.to_string())
    }

    fn finish(&self, model_id: &str) {
        self.in_flight.lock().unwrap().remove(model_id);
    }

    async fn intent_lease(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.intent_gate.lock().await
    }
}

#[derive(Debug, Clone)]
struct StoredModel {
    id: String,
    local_path: Option<String>,
    installed_at: Option<i64>,
    is_active: bool,
    source_kind: String,
    display_name: Option<String>,
}

impl StoredModel {
    fn is_usable(&self) -> bool {
        self.installed_at.is_some()
            && self
                .local_path
                .as_deref()
                .is_some_and(|path| Path::new(path).is_file())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub technical_name: String,
    pub parameter_count: String,
    pub quantization: String,
    pub size_bytes: u64,
    pub recommended: bool,
    pub installed: bool,
    pub active: bool,
    pub selected: bool,
    pub source_kind: String,
    pub local_path: Option<String>,
    pub state: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelState {
    pub hardware: HardwareProfile,
    pub options: Vec<ModelOption>,
    pub active_id: Option<String>,
    pub selected_id: Option<String>,
    pub fallback_notice: Option<String>,
}

fn candidates_for_tier<'a>(registry: &'a Registry, tier: &str) -> Vec<&'a ModelCandidate> {
    let mut candidates = registry
        .tiers
        .iter()
        .find(|candidate_tier| candidate_tier.tier_id == tier)
        .map(|candidate_tier| candidate_tier.models.iter().collect::<Vec<_>>())
        .unwrap_or_default();
    candidates.sort_by_key(|candidate| candidate.priority);
    candidates
}

fn candidate_for_tier<'a>(
    registry: &'a Registry,
    tier: &str,
    model_id: &str,
) -> Option<&'a ModelCandidate> {
    candidates_for_tier(registry, tier)
        .into_iter()
        .find(|candidate| candidate.model_id == model_id)
}

async fn stored_models(conn: &tokio_rusqlite::Connection) -> Result<Vec<StoredModel>, String> {
    conn.call(
        |conn: &mut Connection| -> rusqlite::Result<Vec<StoredModel>> {
            let mut stmt = conn.prepare(
            "SELECT id, local_path, installed_at, is_active, source_kind, display_name FROM models",
        )?;
            let rows = stmt.query_map([], |row| {
                Ok(StoredModel {
                    id: row.get(0)?,
                    local_path: row.get(1)?,
                    installed_at: row.get(2)?,
                    is_active: row.get::<_, i64>(3)? == 1,
                    source_kind: row.get(4)?,
                    display_name: row.get(5)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()
        },
    )
    .await
    .map_err(|error| error.to_string())
}

async fn setting_string(
    conn: &tokio_rusqlite::Connection,
    key: &'static str,
) -> Result<Option<String>, String> {
    conn.call(
        move |conn: &mut Connection| -> rusqlite::Result<Option<String>> {
            let raw = conn
                .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                    row.get::<_, String>(0)
                })
                .optional()?;
            Ok(raw.and_then(|value| serde_json::from_str::<String>(&value).ok()))
        },
    )
    .await
    .map_err(|error| error.to_string())
}

async fn write_setting_string(
    conn: &tokio_rusqlite::Connection,
    key: &'static str,
    value: &str,
) -> Result<(), String> {
    let value = serde_json::to_string(value).map_err(|error| error.to_string())?;
    let now = now_ms();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)\
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            rusqlite::params![key, value, now],
        )?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())
}

async fn clear_setting(conn: &tokio_rusqlite::Connection, key: &'static str) -> Result<(), String> {
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())
}

async fn selected_model_id(
    conn: &tokio_rusqlite::Connection,
    models: &[StoredModel],
) -> Result<Option<String>, String> {
    Ok(setting_string(conn, SELECTED_MODEL_SETTING)
        .await?
        .or_else(|| {
            models
                .iter()
                .find(|model| model.is_active)
                .map(|model| model.id.clone())
        }))
}

async fn upsert_curated_model(
    conn: &tokio_rusqlite::Connection,
    tier: &str,
    candidate: &ModelCandidate,
) -> Result<(), String> {
    let candidate = candidate.clone();
    let tier = tier.to_string();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO models (id, hardware_tier, source_url, quantization, sha256, capability_tags, is_active, source_kind, display_name)\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 'curated', ?7)\
             ON CONFLICT(id) DO UPDATE SET hardware_tier = excluded.hardware_tier, source_url = excluded.source_url,\
             quantization = excluded.quantization, sha256 = excluded.sha256, capability_tags = excluded.capability_tags,\
             source_kind = 'curated', display_name = excluded.display_name",
            rusqlite::params![
                candidate.model_id,
                tier,
                candidate.source_url,
                candidate.quantization,
                candidate.sha256,
                serde_json::to_string(&candidate.capability_tags).unwrap_or_default(),
                candidate.display_name,
            ],
        )?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())
}

async fn mark_model_missing(
    conn: &tokio_rusqlite::Connection,
    model_id: &str,
) -> Result<(), String> {
    let model_id = model_id.to_string();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE models SET installed_at = NULL, is_active = 0 WHERE id = ?1",
            [model_id],
        )?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())
}

async fn set_active_model_row(
    conn: &tokio_rusqlite::Connection,
    model_id: &str,
) -> Result<(), String> {
    let model_id = model_id.to_string();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        let tx = conn.transaction()?;
        tx.execute("UPDATE models SET is_active = 0 WHERE is_active = 1", [])?;
        let changed = tx.execute(
            "UPDATE models SET is_active = 1 WHERE id = ?1 AND installed_at IS NOT NULL",
            [&model_id],
        )?;
        if changed != 1 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())
}

async fn clear_active_model_row(conn: &tokio_rusqlite::Connection) -> Result<(), String> {
    conn.call(|conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute("UPDATE models SET is_active = 0 WHERE is_active = 1", [])?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
struct ActivationError {
    message: String,
    previous_healthy: bool,
}

impl ActivationError {
    fn new(message: impl Into<String>, previous_healthy: bool) -> Self {
        Self {
            message: message.into(),
            previous_healthy,
        }
    }
}

/// If a requested model fails its health check, put the selector back on the
/// last healthy active model. The compare-before-write prevents an older
/// failure from undoing a newer user selection.
async fn restore_healthy_active_selection_if_current(
    conn: &tokio_rusqlite::Connection,
    selection_state: &ModelSelectionState,
    failed_model_id: &str,
) -> Result<(), String> {
    let _intent_lease = selection_state.intent_lease().await;
    if setting_string(conn, SELECTED_MODEL_SETTING)
        .await?
        .as_deref()
        != Some(failed_model_id)
    {
        return Ok(());
    }

    if let Some(active_id) = stored_models(conn)
        .await?
        .into_iter()
        .find(|model| model.is_active && model.is_usable())
        .map(|model| model.id)
    {
        write_setting_string(conn, SELECTED_MODEL_SETTING, &active_id).await?;
    }
    Ok(())
}

fn publish_state(
    app: &AppHandle,
    model_id: &str,
    state: impl Into<String>,
    bytes_downloaded: u64,
    bytes_total: u64,
) {
    downloader::publish_model_progress(
        app,
        ModelInstallProgress {
            model_id: model_id.to_string(),
            bytes_downloaded,
            bytes_total,
            state: state.into(),
        },
    );
}

/// Restore the last active server after a candidate had already loaded. The
/// caller owns the switch lease. A failed restore clears the active database
/// pointer so UI and generation preflight never claim an unhealthy server is
/// still available.
async fn restore_previous_server(
    app: &AppHandle,
    conn: &tokio_rusqlite::Connection,
    server_state: &crate::inference::server::ServerState,
    previous_path: Option<&Path>,
    candidate_was_active: bool,
) -> bool {
    if candidate_was_active {
        return true;
    }
    if let Some(previous_path) = previous_path {
        let restored = server_state
            .restart_with_rollback(app, previous_path, None)
            .await
            .is_ok();
        if !restored {
            let _ = clear_active_model_row(conn).await;
        }
        restored
    } else {
        server_state.shutdown(app).await;
        false
    }
}

/// Health-gate a candidate while all existing model work drains behind the
/// server's write lease. SQLite flips only after the new sidecar is healthy.
/// If loading fails, `restart_with_rollback` restores the prior model and the
/// old active row remains untouched.
async fn activate_model(
    app: &AppHandle,
    conn: &tokio_rusqlite::Connection,
    server_state: &crate::inference::server::ServerState,
    model_id: &str,
    require_selected: bool,
) -> Result<bool, ActivationError> {
    let _switch_lease = server_state.switch_lease().await;
    let models = stored_models(conn)
        .await
        .map_err(|error| ActivationError::new(error, false))?;
    let previous = models
        .iter()
        .find(|model| model.is_active && model.is_usable());
    let previous_path = previous
        .and_then(|model| model.local_path.as_deref())
        .map(PathBuf::from);
    let was_active = previous.is_some_and(|model| model.id == model_id);
    let candidate = models
        .iter()
        .find(|model| model.id == model_id && model.is_usable())
        .ok_or_else(|| {
            ActivationError::new(
                "the selected model is not available on disk",
                previous.is_some(),
            )
        })?;
    let candidate_path = PathBuf::from(candidate.local_path.as_deref().ok_or_else(|| {
        ActivationError::new("the selected model has no local file", previous.is_some())
    })?);

    if require_selected
        && setting_string(conn, SELECTED_MODEL_SETTING)
            .await
            .map_err(|error| ActivationError::new(error, false))?
            .as_deref()
            != Some(model_id)
    {
        publish_state(
            app,
            model_id,
            if was_active { "active" } else { "installed" },
            0,
            0,
        );
        return Ok(false);
    }

    publish_state(app, model_id, "preparing", 0, 0);
    if was_active {
        if let Err(error) = server_state.ensure_running(app, &candidate_path).await {
            let _ = clear_active_model_row(conn).await;
            return Err(ActivationError::new(error, false));
        }
    } else {
        if let Err(error) = server_state
            .restart_with_rollback(app, &candidate_path, previous_path.as_deref())
            .await
        {
            if previous_path.is_some() && !error.previous_restored {
                let _ = clear_active_model_row(conn).await;
            }
            return Err(ActivationError::new(
                error.candidate_error,
                error.previous_restored,
            ));
        }
    }

    // A later click may have updated the durable intent while this model was
    // loading. Restore the previous server and let the later request perform
    // its own handoff; a stale completion must never win.
    let selected_after_load = match setting_string(conn, SELECTED_MODEL_SETTING).await {
        Ok(selected) => selected,
        Err(error) => {
            let previous_healthy = restore_previous_server(
                app,
                conn,
                server_state,
                previous_path.as_deref(),
                was_active,
            )
            .await;
            return Err(ActivationError::new(error, previous_healthy));
        }
    };
    if require_selected && selected_after_load.as_deref() != Some(model_id) {
        if was_active {
            publish_state(app, model_id, "active", 0, 0);
        } else {
            restore_previous_server(app, conn, server_state, previous_path.as_deref(), false).await;
            publish_state(app, model_id, "installed", 0, 0);
        }
        return Ok(false);
    }

    if let Err(error) = set_active_model_row(conn, model_id).await {
        let previous_healthy = restore_previous_server(
            app,
            conn,
            server_state,
            previous_path.as_deref(),
            was_active,
        )
        .await;
        return Err(ActivationError::new(error, previous_healthy));
    }

    publish_state(app, model_id, "active", 0, 0);
    Ok(true)
}

async fn begin_install(
    app: &AppHandle,
    conn: &tokio_rusqlite::Connection,
    selection_state: &ModelSelectionState,
    profile: &HardwareProfile,
    candidate: &ModelCandidate,
) -> Result<bool, String> {
    upsert_curated_model(conn, &profile.tier, candidate).await?;
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("models");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let dest = dir.join(format!("{}.gguf", candidate.model_id));
    let resumed = dest.with_extension("part").exists();

    // The intent check, in-flight reservation, and task spawn are one atomic
    // step relative to later clicks. A reconciliation action computed for A
    // therefore cannot begin a multi-GB download after the user has selected
    // B, while a download that was legitimately started remains reusable.
    let _intent_lease = selection_state.intent_lease().await;
    if setting_string(conn, SELECTED_MODEL_SETTING)
        .await?
        .as_deref()
        != Some(candidate.model_id.as_str())
    {
        return Ok(false);
    }
    if !selection_state.begin(&candidate.model_id) {
        return Ok(true);
    }

    publish_state(app, &candidate.model_id, "queued", 0, candidate.size_bytes);

    let app = app.clone();
    let conn = conn.clone();
    let selection_state = selection_state.clone();
    let candidate = candidate.clone();
    tauri::async_runtime::spawn(async move {
        let result = downloader::download_resumable(
            &app,
            &candidate.model_id,
            &candidate.source_url,
            &dest,
            &candidate.sha256,
            candidate.size_bytes,
        )
        .await;

        match result {
            Ok(path) => {
                let model_id = candidate.model_id.clone();
                let path_string = path.to_string_lossy().to_string();
                let now = now_ms();
                let write_result = conn
                    .call(move |conn: &mut Connection| -> rusqlite::Result<()> {
                        conn.execute(
                            "UPDATE models SET local_path = ?1, installed_at = ?2 WHERE id = ?3",
                            rusqlite::params![path_string, now, model_id],
                        )?;
                        Ok(())
                    })
                    .await;

                if let Err(error) = write_result {
                    publish_state(&app, &candidate.model_id, format!("error: {error}"), 0, 0);
                } else if setting_string(&conn, SELECTED_MODEL_SETTING)
                    .await
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some(candidate.model_id.as_str())
                {
                    if let Some(server_state) =
                        app.try_state::<crate::inference::server::ServerState>()
                    {
                        match activate_model(&app, &conn, &server_state, &candidate.model_id, true)
                            .await
                        {
                            Ok(_) => {}
                            Err(error) => {
                                if error.previous_healthy {
                                    let _ = restore_healthy_active_selection_if_current(
                                        &conn,
                                        &selection_state,
                                        &candidate.model_id,
                                    )
                                    .await;
                                }
                                publish_state(
                                    &app,
                                    &candidate.model_id,
                                    format!("error: {error}"),
                                    0,
                                    0,
                                );
                            }
                        }
                    } else {
                        publish_state(&app, &candidate.model_id, "installed", 0, 0);
                    }
                } else {
                    publish_state(&app, &candidate.model_id, "installed", 0, 0);
                }
            }
            Err(error) => publish_state(&app, &candidate.model_id, format!("error: {error}"), 0, 0),
        }

        selection_state.finish(&candidate.model_id);
    });

    Ok(resumed)
}

#[tauri::command]
#[specta::specta]
pub async fn start_model_install(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    selection_state: State<'_, ModelSelectionState>,
    server_state: State<'_, crate::inference::server::ServerState>,
    model_id: Option<String>,
) -> Result<StartModelInstallResult, String> {
    let conn = db_cell.get(&app).await?.clone();
    let registry = model_registry::bundled();
    let profile = hardware::detect();
    let candidates = candidates_for_tier(&registry, &profile.tier);
    let candidate = if let Some(model_id) = model_id.as_deref() {
        candidates
            .into_iter()
            .find(|candidate| candidate.model_id == model_id)
    } else {
        candidates.into_iter().next()
    }
    .cloned()
    .ok_or_else(|| "no matching model candidate found for this Mac".to_string())?;

    {
        let _intent_lease = selection_state.intent_lease().await;
        write_setting_string(&conn, SELECTED_MODEL_SETTING, &candidate.model_id).await?;
        clear_setting(&conn, FALLBACK_NOTICE_SETTING).await?;
        upsert_curated_model(&conn, &profile.tier, &candidate).await?;
    }

    let installed = stored_models(&conn)
        .await?
        .into_iter()
        .find(|model| model.id == candidate.model_id)
        .is_some_and(|model| model.is_usable());
    let resumed = if installed {
        if let Err(error) =
            activate_model(&app, &conn, &server_state, &candidate.model_id, true).await
        {
            if error.previous_healthy {
                restore_healthy_active_selection_if_current(
                    &conn,
                    &selection_state,
                    &candidate.model_id,
                )
                .await?;
            }
            publish_state(&app, &candidate.model_id, format!("error: {error}"), 0, 0);
            return Err(error.to_string());
        }
        false
    } else {
        begin_install(&app, &conn, &selection_state, &profile, &candidate).await?
    };

    Ok(StartModelInstallResult {
        model_id: candidate.model_id,
        resumed,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn get_model_install_status(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    selection_state: State<'_, ModelSelectionState>,
    model_id: String,
) -> Result<ModelInstallStatus, String> {
    if let Some(progress) = selection_state.progress_for(&model_id) {
        return Ok(ModelInstallStatus {
            state: progress.state,
            bytes_downloaded: progress.bytes_downloaded,
            bytes_total: progress.bytes_total,
        });
    }

    let conn = db_cell.get(&app).await?;
    let model = stored_models(conn)
        .await?
        .into_iter()
        .find(|model| model.id == model_id);
    Ok(ModelInstallStatus {
        state: match model {
            Some(model) if model.is_active && model.is_usable() => "active".to_string(),
            Some(model) if model.is_usable() => "installed".to_string(),
            _ => "idle".to_string(),
        },
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
        let mut stmt = conn
            .prepare("SELECT id, hardware_tier, is_active, installed_at, local_path FROM models")?;
        let rows = stmt.query_map([], |row| {
            let installed_at = row.get::<_, Option<i64>>(3)?;
            let local_path = row.get::<_, Option<String>>(4)?;
            let installed = installed_at.is_some()
                && local_path
                    .as_deref()
                    .is_some_and(|path| Path::new(path).is_file());
            Ok(ModelRow {
                id: row.get(0)?,
                hardware_tier: row.get(1)?,
                is_active: row.get::<_, i64>(2)? == 1 && installed,
                installed,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
    })
    .await
    .map_err(|error| error.to_string())
}

fn progress_state(
    selection_state: &ModelSelectionState,
    model_id: &str,
    installed: bool,
    active: bool,
) -> (String, u64, u64) {
    if active {
        return ("active".to_string(), 0, 0);
    }
    if let Some(progress) = selection_state.progress_for(model_id) {
        let terminal_ready = matches!(progress.state.as_str(), "active" | "installed");
        if !(terminal_ready && installed) {
            return (
                progress.state,
                progress.bytes_downloaded,
                progress.bytes_total,
            );
        }
    }
    if installed {
        ("ready".to_string(), 0, 0)
    } else {
        ("idle".to_string(), 0, 0)
    }
}

async fn build_model_state(
    conn: &tokio_rusqlite::Connection,
    selection_state: &ModelSelectionState,
) -> Result<ModelState, String> {
    let hardware = hardware::detect();
    let registry = model_registry::bundled();
    let candidates = candidates_for_tier(&registry, &hardware.tier);
    let recommended_id = candidates
        .first()
        .map(|candidate| candidate.model_id.as_str());
    let stored = stored_models(conn).await?;
    let selected_id = selected_model_id(conn, &stored).await?;
    let active_id = stored
        .iter()
        .find(|model| model.is_active && model.is_usable())
        .map(|model| model.id.clone());
    let fallback_notice = setting_string(conn, FALLBACK_NOTICE_SETTING).await?;

    let mut options = Vec::new();
    for candidate in candidates {
        let record = stored.iter().find(|model| model.id == candidate.model_id);
        let installed = record.is_some_and(StoredModel::is_usable);
        let active = active_id.as_deref() == Some(candidate.model_id.as_str());
        let selected = selected_id.as_deref() == Some(candidate.model_id.as_str());
        let (state, bytes_downloaded, bytes_total) =
            progress_state(selection_state, &candidate.model_id, installed, active);
        options.push(ModelOption {
            id: candidate.model_id.clone(),
            display_name: candidate.display_name.clone(),
            description: candidate.description.clone(),
            technical_name: candidate.technical_name.clone(),
            parameter_count: candidate.parameter_count.clone(),
            quantization: candidate.quantization.clone(),
            size_bytes: candidate.size_bytes,
            recommended: recommended_id == Some(candidate.model_id.as_str()),
            installed,
            active,
            selected,
            source_kind: CURATED_SOURCE_KIND.to_string(),
            local_path: record.and_then(|model| model.local_path.clone()),
            state,
            bytes_downloaded,
            bytes_total,
        });
    }

    // Keep an active or pending curated model visible if the detected
    // hardware tier changes (for example, a transient unknown-memory read on
    // launch). Its metadata still comes from the signed bundled catalog, but
    // it is not presented as the recommendation for the current tier.
    let mut shown_curated = options
        .iter()
        .map(|option| option.id.clone())
        .collect::<HashSet<_>>();
    for model in stored.iter().filter(|model| {
        model.source_kind == CURATED_SOURCE_KIND
            && (model.is_active || selected_id.as_deref() == Some(model.id.as_str()))
    }) {
        if !shown_curated.insert(model.id.clone()) {
            continue;
        }
        let Some(candidate) = model_registry::find_candidate(&registry, &model.id) else {
            continue;
        };
        let installed = model.is_usable();
        let active = active_id.as_deref() == Some(model.id.as_str());
        let selected = selected_id.as_deref() == Some(model.id.as_str());
        let (state, bytes_downloaded, bytes_total) =
            progress_state(selection_state, &model.id, installed, active);
        options.push(ModelOption {
            id: model.id.clone(),
            display_name: candidate.display_name.clone(),
            description: candidate.description.clone(),
            technical_name: candidate.technical_name.clone(),
            parameter_count: candidate.parameter_count.clone(),
            quantization: candidate.quantization.clone(),
            size_bytes: candidate.size_bytes,
            recommended: false,
            installed,
            active,
            selected,
            source_kind: CURATED_SOURCE_KIND.to_string(),
            local_path: model.local_path.clone(),
            state,
            bytes_downloaded,
            bytes_total,
        });
    }

    for model in stored.iter().filter(|model| {
        model.source_kind == LOCAL_SOURCE_KIND
            && (model.is_active || selected_id.as_deref() == Some(model.id.as_str()))
    }) {
        let installed = model.is_usable();
        let active = active_id.as_deref() == Some(model.id.as_str());
        let selected = selected_id.as_deref() == Some(model.id.as_str());
        let (state, bytes_downloaded, bytes_total) =
            progress_state(selection_state, &model.id, installed, active);
        let path = model.local_path.clone();
        let technical_name = path
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Local GGUF")
            .to_string();
        let size_bytes = path
            .as_deref()
            .and_then(|path| std::fs::metadata(path).ok())
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        options.push(ModelOption {
            id: model.id.clone(),
            display_name: model
                .display_name
                .clone()
                .unwrap_or_else(|| "Model from this Mac".to_string()),
            description: "A compatible model file from this Mac.".to_string(),
            technical_name,
            parameter_count: "Local".to_string(),
            quantization: "GGUF".to_string(),
            size_bytes,
            recommended: false,
            installed,
            active,
            selected,
            source_kind: LOCAL_SOURCE_KIND.to_string(),
            local_path: path,
            state,
            bytes_downloaded,
            bytes_total,
        });
    }

    Ok(ModelState {
        hardware,
        options,
        active_id,
        selected_id,
        fallback_notice,
    })
}

enum ReconcileAction {
    None,
    Activate(String),
    Install(Box<ModelCandidate>),
}

/// Reconcile a stale active path before Settings renders or any new model
/// operation begins. The fallback is deterministic: the hardware
/// recommendation when present on disk, then the first other usable curated
/// option, otherwise the recommendation is downloaded and activated.
pub async fn reconcile_active_model(
    app: &AppHandle,
    conn: &tokio_rusqlite::Connection,
    selection_state: &ModelSelectionState,
    server_state: &crate::inference::server::ServerState,
) -> Result<(), String> {
    let registry = model_registry::bundled();
    let profile = hardware::detect();
    let candidates = candidates_for_tier(&registry, &profile.tier);

    // Serialize the read/compare/write portion with explicit user choices.
    // The slower download and server handoff happen after this guard is
    // released; both paths compare the durable intent again before activation.
    let action = {
        let _intent_lease = selection_state.intent_lease().await;
        let mut models = stored_models(conn).await?;
        let selected = setting_string(conn, SELECTED_MODEL_SETTING).await?;
        let missing_active = models
            .iter()
            .find(|model| model.is_active && !model.is_usable())
            .cloned();
        let missing_selected_local = models
            .iter()
            .find(|model| {
                model.source_kind == LOCAL_SOURCE_KIND
                    && !model.is_usable()
                    && selected.as_deref() == Some(model.id.as_str())
            })
            .cloned();

        let mut marked = HashSet::new();
        for missing in [missing_active.as_ref(), missing_selected_local.as_ref()]
            .into_iter()
            .flatten()
        {
            if marked.insert(missing.id.clone()) {
                mark_model_missing(conn, &missing.id).await?;
            }
        }
        if !marked.is_empty() {
            models = stored_models(conn).await?;
        }

        // A missing old active model must not override a newer explicit
        // selection. Only recover it when it is still the selected intent (or
        // when upgrading a legacy database that has no selected-id setting).
        let fallback_subject = missing_selected_local.clone().or_else(|| {
            missing_active.clone().filter(|missing| {
                selected.is_none() || selected.as_deref() == Some(missing.id.as_str())
            })
        });

        if let Some(missing) = fallback_subject {
            let ready = candidates.iter().find(|candidate| {
                models
                    .iter()
                    .find(|model| model.id == candidate.model_id)
                    .is_some_and(StoredModel::is_usable)
            });
            let target = ready
                .copied()
                .or_else(|| candidates.first().copied())
                .cloned()
                .ok_or_else(|| "no curated fallback is available for this Mac".to_string())?;
            let target_is_ready = ready.is_some();

            upsert_curated_model(conn, &profile.tier, &target).await?;
            write_setting_string(conn, SELECTED_MODEL_SETTING, &target.model_id).await?;
            let missing_name = missing
                .display_name
                .as_deref()
                .unwrap_or("Your local model");
            let notice = if target_is_ready {
                format!(
                    "{missing_name} is no longer available. Doce switched to {}.",
                    target.display_name
                )
            } else {
                format!(
                    "{missing_name} is no longer available. Doce is getting {} ready.",
                    target.display_name
                )
            };
            write_setting_string(conn, FALLBACK_NOTICE_SETTING, &notice).await?;

            if target_is_ready {
                ReconcileAction::Activate(target.model_id)
            } else {
                ReconcileAction::Install(Box::new(target))
            }
        } else if let Some(selected_id) = selected {
            // Reconstruct work after a relaunch: a durable selection can point
            // to an installed model awaiting handoff or to a curated download
            // whose `.part` file should resume. The old active model remains
            // available while that work proceeds.
            let failed_this_run = selection_state
                .progress_for(&selected_id)
                .is_some_and(|progress| progress.state.starts_with("error"));
            if models
                .iter()
                .any(|model| model.id == selected_id && model.is_active && model.is_usable())
            {
                ReconcileAction::None
            } else if failed_this_run {
                // Do not repeat an OOM, incompatible-model, or network
                // failure on every Settings snapshot. Explicit Retry invokes
                // the selection command directly; a relaunch gets one fresh
                // attempt because progress snapshots are process-local.
                ReconcileAction::None
            } else if let Some(selected_model) = models.iter().find(|model| model.id == selected_id)
            {
                if selected_model.source_kind == LOCAL_SOURCE_KIND {
                    if selected_model.is_usable() {
                        ReconcileAction::Activate(selected_id)
                    } else {
                        ReconcileAction::None
                    }
                } else if let Some(candidate) =
                    model_registry::find_candidate(&registry, &selected_id).cloned()
                {
                    upsert_curated_model(conn, &profile.tier, &candidate).await?;
                    if selected_model.is_usable() {
                        ReconcileAction::Activate(selected_id)
                    } else {
                        ReconcileAction::Install(Box::new(candidate))
                    }
                } else {
                    ReconcileAction::None
                }
            } else if let Some(candidate) =
                model_registry::find_candidate(&registry, &selected_id).cloned()
            {
                upsert_curated_model(conn, &profile.tier, &candidate).await?;
                ReconcileAction::Install(Box::new(candidate))
            } else {
                ReconcileAction::None
            }
        } else {
            ReconcileAction::None
        }
    };

    match action {
        ReconcileAction::None => {}
        ReconcileAction::Activate(model_id) => {
            if let Err(error) = activate_model(app, conn, server_state, &model_id, true).await {
                if error.previous_healthy {
                    let _ = restore_healthy_active_selection_if_current(
                        conn,
                        selection_state,
                        &model_id,
                    )
                    .await;
                }
                publish_state(app, &model_id, format!("error: {error}"), 0, 0);
            }
        }
        ReconcileAction::Install(candidate) => {
            begin_install(app, conn, selection_state, &profile, &candidate).await?;
        }
    }
    Ok(())
}

/// Called by generation entry points before they take a read lease. It heals
/// deleted paths first, then returns only a genuinely present active GGUF.
pub async fn ensure_usable_model_path(
    app: &AppHandle,
    conn: &tokio_rusqlite::Connection,
    server_state: &crate::inference::server::ServerState,
) -> Result<String, String> {
    let selection_state = app
        .try_state::<ModelSelectionState>()
        .ok_or_else(|| "model selection state is unavailable".to_string())?;
    reconcile_active_model(app, conn, &selection_state, server_state).await?;
    stored_models(conn)
        .await?
        .into_iter()
        .find(|model| model.is_active && model.is_usable())
        .and_then(|model| model.local_path)
        .ok_or_else(|| {
            "The selected model is still being prepared. Try again in a moment.".to_string()
        })
}

/// Read the active path again after a generation lease has been acquired.
/// A switch may have completed between preflight reconciliation and lease
/// acquisition, so callers must not reuse the preflight path.
pub async fn active_model_path(conn: &tokio_rusqlite::Connection) -> Result<String, String> {
    stored_models(conn)
        .await?
        .into_iter()
        .find(|model| model.is_active && model.is_usable())
        .and_then(|model| model.local_path)
        .ok_or_else(|| "No model is ready to use yet.".to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn get_model_state(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    selection_state: State<'_, ModelSelectionState>,
    server_state: State<'_, crate::inference::server::ServerState>,
) -> Result<ModelState, String> {
    let conn = db_cell.get(&app).await?.clone();
    reconcile_active_model(&app, &conn, &selection_state, &server_state).await?;
    build_model_state(&conn, &selection_state).await
}

#[tauri::command]
#[specta::specta]
pub async fn select_curated_model(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    selection_state: State<'_, ModelSelectionState>,
    server_state: State<'_, crate::inference::server::ServerState>,
    model_id: String,
) -> Result<ModelState, String> {
    let conn = db_cell.get(&app).await?.clone();
    let registry = model_registry::bundled();
    let profile = hardware::detect();
    let candidate = candidate_for_tier(&registry, &profile.tier, &model_id)
        .cloned()
        .ok_or_else(|| "that model is not available for this Mac".to_string())?;

    {
        let _intent_lease = selection_state.intent_lease().await;
        write_setting_string(&conn, SELECTED_MODEL_SETTING, &candidate.model_id).await?;
        clear_setting(&conn, FALLBACK_NOTICE_SETTING).await?;
        upsert_curated_model(&conn, &profile.tier, &candidate).await?;
    }

    let installed = stored_models(&conn)
        .await?
        .into_iter()
        .find(|model| model.id == candidate.model_id)
        .is_some_and(|model| model.is_usable());
    if installed {
        if let Err(error) =
            activate_model(&app, &conn, &server_state, &candidate.model_id, true).await
        {
            if error.previous_healthy {
                restore_healthy_active_selection_if_current(
                    &conn,
                    &selection_state,
                    &candidate.model_id,
                )
                .await?;
            }
            publish_state(&app, &candidate.model_id, format!("error: {error}"), 0, 0);
            return Err(error.to_string());
        }
    } else {
        begin_install(&app, &conn, &selection_state, &profile, &candidate).await?;
    }
    build_model_state(&conn, &selection_state).await
}

fn validate_local_gguf(path: &str) -> Result<(PathBuf, String, u64), String> {
    let path = Path::new(path);
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
    {
        return Err("Choose a GGUF model file.".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "The selected model file is no longer available.".to_string())?;
    let metadata = canonical
        .metadata()
        .map_err(|error| format!("The selected model file cannot be read: {error}"))?;
    if !metadata.is_file() {
        return Err("The selected path is not a model file.".to_string());
    }
    let mut header = [0u8; 8];
    File::open(&canonical)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|_| "The selected file is not a valid GGUF model.".to_string())?;
    if &header[..4] != b"GGUF" {
        return Err("The selected file is not a valid GGUF model.".to_string());
    }
    let version = u32::from_le_bytes(header[4..8].try_into().unwrap());
    if !(2..=3).contains(&version) {
        return Err(format!("GGUF version {version} is not supported."));
    }
    let display_name = canonical
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Local model")
        .to_string();
    Ok((canonical, display_name, metadata.len()))
}

fn local_model_id(path: &Path) -> String {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    format!("local-{:x}", digest)
}

#[tauri::command]
#[specta::specta]
pub async fn select_local_model(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    selection_state: State<'_, ModelSelectionState>,
    server_state: State<'_, crate::inference::server::ServerState>,
    path: String,
) -> Result<ModelState, String> {
    let (canonical, display_name, _size_bytes) = validate_local_gguf(&path)?;
    let model_id = local_model_id(&canonical);
    let path = canonical.to_string_lossy().to_string();
    let now = now_ms();
    let conn = db_cell.get(&app).await?.clone();
    let row_model_id = model_id.clone();
    let row_display_name = display_name.clone();
    {
        let _intent_lease = selection_state.intent_lease().await;
        conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO models (id, hardware_tier, source_url, quantization, sha256, local_path, capability_tags, installed_at, is_active, source_kind, display_name)\
                 VALUES (?1, 'local', '', 'GGUF', '', ?2, '[\"local\"]', ?3, 0, 'local', ?4)\
                 ON CONFLICT(id) DO UPDATE SET local_path = excluded.local_path, installed_at = excluded.installed_at,\
                 source_kind = 'local', display_name = excluded.display_name",
                rusqlite::params![row_model_id, path, now, row_display_name],
            )?;
            Ok(())
        })
        .await
        .map_err(|error| error.to_string())?;
        write_setting_string(&conn, SELECTED_MODEL_SETTING, &model_id).await?;
        clear_setting(&conn, FALLBACK_NOTICE_SETTING).await?;
    }

    if let Err(error) = activate_model(&app, &conn, &server_state, &model_id, true).await {
        if error.previous_healthy {
            restore_healthy_active_selection_if_current(&conn, &selection_state, &model_id).await?;
        }
        publish_state(&app, &model_id, format!("error: {error}"), 0, 0);
        return Err(error.to_string());
    }
    build_model_state(&conn, &selection_state).await
}

#[tauri::command]
#[specta::specta]
pub async fn dismiss_model_notice(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
) -> Result<(), String> {
    let conn = db_cell.get(&app).await?;
    clear_setting(conn, FALLBACK_NOTICE_SETTING).await
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn validates_only_readable_supported_gguf_files() {
        let dir = tempfile::tempdir().unwrap();
        let valid = dir.path().join("My Model.gguf");
        let mut file = File::create(&valid).unwrap();
        file.write_all(b"GGUF\x03\x00\x00\x00").unwrap();
        let (canonical, display_name, size) = validate_local_gguf(valid.to_str().unwrap()).unwrap();
        assert_eq!(canonical, valid.canonicalize().unwrap());
        assert_eq!(display_name, "My Model");
        assert_eq!(size, 8);

        let invalid = dir.path().join("not-a-model.gguf");
        std::fs::write(&invalid, b"nope").unwrap();
        assert!(validate_local_gguf(invalid.to_str().unwrap()).is_err());
        assert!(validate_local_gguf(dir.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn local_ids_are_path_stable_and_path_specific() {
        let a = local_model_id(Path::new("/tmp/a.gguf"));
        assert_eq!(a, local_model_id(Path::new("/tmp/a.gguf")));
        assert_ne!(a, local_model_id(Path::new("/tmp/b.gguf")));
    }

    #[tokio::test]
    async fn durable_selection_is_separate_from_the_last_healthy_active_model() {
        let conn = crate::storage::test_async_connection().await;
        let dir = tempfile::tempdir().unwrap();
        let active_path = dir.path().join("active.gguf");
        let selected_path = dir.path().join("selected.gguf");
        std::fs::write(&active_path, b"GGUF\x03\x00\x00\x00").unwrap();
        std::fs::write(&selected_path, b"GGUF\x03\x00\x00\x00").unwrap();
        let active_path = active_path.to_string_lossy().to_string();
        let selected_path = selected_path.to_string_lossy().to_string();
        conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO models (id, hardware_tier, source_url, quantization, sha256, local_path, capability_tags, installed_at, is_active, source_kind)\
                 VALUES ('active', 'local', '', 'GGUF', '', ?1, '[]', 1, 1, 'local')",
                [active_path],
            )?;
            conn.execute(
                "INSERT INTO models (id, hardware_tier, source_url, quantization, sha256, local_path, capability_tags, installed_at, is_active, source_kind)\
                 VALUES ('selected', 'local', '', 'GGUF', '', ?1, '[]', 1, 0, 'local')",
                [selected_path],
            )?;
            Ok(())
        })
        .await
        .unwrap();

        write_setting_string(&conn, SELECTED_MODEL_SETTING, "selected")
            .await
            .unwrap();
        let models = stored_models(&conn).await.unwrap();
        assert_eq!(
            selected_model_id(&conn, &models).await.unwrap().as_deref(),
            Some("selected")
        );
        assert!(models
            .iter()
            .any(|model| model.id == "active" && model.is_active));

        set_active_model_row(&conn, "selected").await.unwrap();
        let models = stored_models(&conn).await.unwrap();
        assert_eq!(models.iter().filter(|model| model.is_active).count(), 1);
        assert!(models
            .iter()
            .any(|model| model.id == "selected" && model.is_active));
    }

    #[tokio::test]
    async fn failed_activation_restore_uses_compare_before_write() {
        let conn = crate::storage::test_async_connection().await;
        let dir = tempfile::tempdir().unwrap();
        let active_path = dir.path().join("active.gguf");
        std::fs::write(&active_path, b"GGUF\x03\x00\x00\x00").unwrap();
        let active_path = active_path.to_string_lossy().to_string();
        conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO models (id, hardware_tier, source_url, quantization, sha256, local_path, capability_tags, installed_at, is_active, source_kind, display_name)\
                 VALUES ('healthy', 'test', '', 'GGUF', '', ?1, '[]', 1, 1, 'local', 'Healthy')",
                [active_path],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        let selection_state = ModelSelectionState::default();

        write_setting_string(&conn, SELECTED_MODEL_SETTING, "newer-choice")
            .await
            .unwrap();
        restore_healthy_active_selection_if_current(&conn, &selection_state, "failed-choice")
            .await
            .unwrap();
        assert_eq!(
            setting_string(&conn, SELECTED_MODEL_SETTING)
                .await
                .unwrap()
                .as_deref(),
            Some("newer-choice"),
            "an older failure must not overwrite a newer click"
        );

        write_setting_string(&conn, SELECTED_MODEL_SETTING, "failed-choice")
            .await
            .unwrap();
        restore_healthy_active_selection_if_current(&conn, &selection_state, "failed-choice")
            .await
            .unwrap();
        assert_eq!(
            setting_string(&conn, SELECTED_MODEL_SETTING)
                .await
                .unwrap()
                .as_deref(),
            Some("healthy"),
            "the failed request should return to the confirmed active model"
        );
    }

    #[test]
    fn progress_snapshot_keeps_active_and_pending_states_distinct() {
        let state = ModelSelectionState::default();
        state.record_progress(ModelInstallProgress {
            model_id: "advanced".to_string(),
            bytes_downloaded: 50,
            bytes_total: 100,
            state: "downloading".to_string(),
        });
        assert_eq!(
            progress_state(&state, "advanced", false, false),
            ("downloading".to_string(), 50, 100)
        );
        assert_eq!(
            progress_state(&state, "balanced", true, true),
            ("active".to_string(), 0, 0)
        );
    }
}
