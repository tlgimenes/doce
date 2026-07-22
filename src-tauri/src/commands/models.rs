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
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

const SELECTED_MODEL_SETTING: &str = "model.selectedId";
const FALLBACK_NOTICE_SETTING: &str = "model.fallbackNotice";
const LOCAL_SOURCE_KIND: &str = "local";
const CURATED_SOURCE_KIND: &str = "curated";
const ENDPOINT_SOURCE_KIND: &str = "endpoint";

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

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownload {
    pub model_id: String,
    pub display_name: String,
    pub state: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub revision: u32,
    pub error: Option<String>,
}

#[derive(Clone)]
struct ActiveDownload {
    revision: u32,
    cancel: CancellationToken,
    done: watch::Receiver<bool>,
}

/// Process-local download snapshots plus the in-flight set used to dedupe
/// StrictMode, Settings remounts, retries, and fallback recovery. The durable
/// selected model still lives in SQLite; this state only describes work that
/// is currently running (or its most recent terminal result).
#[derive(Default, Clone)]
pub struct ModelSelectionState {
    active_downloads: Arc<Mutex<HashMap<String, ActiveDownload>>>,
    progress: Arc<Mutex<HashMap<String, ModelInstallProgress>>>,
    intent_gate: Arc<tokio::sync::Mutex<()>>,
    download_gate: Arc<tokio::sync::Mutex<()>>,
    restored_downloads: Arc<tokio::sync::Mutex<bool>>,
    activation_failures: Arc<Mutex<HashSet<String>>>,
}

impl ModelSelectionState {
    /// Returns false for an event from an older attempt. Callers must not emit
    /// those events either: a late cancelled worker can otherwise regress a
    /// newly-resumed download in the UI.
    pub(crate) fn record_progress(&self, progress: ModelInstallProgress) -> bool {
        let mut snapshots = self.progress.lock().unwrap();
        if snapshots
            .get(&progress.model_id)
            .is_some_and(|current| current.revision > progress.revision)
        {
            return false;
        }
        snapshots.insert(progress.model_id.clone(), progress);
        true
    }

    fn progress_for(&self, model_id: &str) -> Option<ModelInstallProgress> {
        self.progress.lock().unwrap().get(model_id).cloned()
    }

    fn active_download(&self, model_id: &str) -> Option<ActiveDownload> {
        self.active_downloads.lock().unwrap().get(model_id).cloned()
    }

    fn begin(&self, model_id: &str, download: ActiveDownload) -> bool {
        let mut active = self.active_downloads.lock().unwrap();
        if active.contains_key(model_id) {
            return false;
        }
        active.insert(model_id.to_string(), download);
        true
    }

    fn finish(&self, model_id: &str, revision: u32) {
        let mut active = self.active_downloads.lock().unwrap();
        if active
            .get(model_id)
            .is_some_and(|download| download.revision == revision)
        {
            active.remove(model_id);
        }
    }

    async fn intent_lease(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.intent_gate.lock().await
    }

    async fn download_lease(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.download_gate.lock().await
    }

    fn mark_activation_failed(&self, model_id: &str) {
        self.activation_failures
            .lock()
            .unwrap()
            .insert(model_id.to_string());
    }

    fn clear_activation_failure(&self, model_id: &str) {
        self.activation_failures.lock().unwrap().remove(model_id);
    }

    fn activation_failed(&self, model_id: &str) -> bool {
        self.activation_failures.lock().unwrap().contains(model_id)
    }
}

/// Managed store for endpoint API keys, keyed by model id. Backed by the OS
/// secret store — a [`crate::oauth::KeyringStore`] under the `doce-endpoints`
/// service at runtime, the in-memory impl in tests — via the reusable
/// [`crate::oauth::SecretStore`] trait. Deliberately NOT `OAuthTokenStore`
/// (which is OAuth-credential-specific): the value here is just the raw API key
/// string, never persisted to SQLite, the same discipline OAuth tokens follow.
pub struct EndpointKeyStore {
    secrets: Arc<dyn crate::oauth::SecretStore>,
}

impl EndpointKeyStore {
    pub fn new(secrets: Arc<dyn crate::oauth::SecretStore>) -> Self {
        Self { secrets }
    }

    fn set(&self, model_id: &str, key: &str) -> Result<(), String> {
        self.secrets.set(model_id, key).map_err(|e| e.to_string())
    }

    fn get(&self, model_id: &str) -> Result<Option<String>, String> {
        self.secrets.get(model_id).map_err(|e| e.to_string())
    }

    fn delete(&self, model_id: &str) -> Result<(), String> {
        self.secrets.delete(model_id).map_err(|e| e.to_string())
    }
}

/// The resolved target of the single active model, as consumed by the turn
/// path (`commands::agent::send_agent_message`). `Local` keeps the exact
/// pre-endpoint behavior (a supervised sidecar spawned for the GGUF at `path`);
/// `Endpoint` is the NEW branch, taken only when the active model's
/// `source_kind == "endpoint"` — no sidecar, generation goes straight to `url`.
#[derive(Debug, Clone, PartialEq)]
pub enum ActiveModelTarget {
    Local {
        path: String,
    },
    Endpoint {
        url: String,
        model: String,
        api_key: Option<String>,
        context_window: u32,
        /// `!use_cache_prompt` — strip the llama.cpp-only request extras for a
        /// generic OpenAI-compatible endpoint (see `http::ChatRequest::clean`).
        clean_body: bool,
    },
}

#[derive(Debug, Clone)]
struct StoredModel {
    id: String,
    local_path: Option<String>,
    installed_at: Option<i64>,
    is_active: bool,
    source_kind: String,
    display_name: Option<String>,
    endpoint_url: Option<String>,
    endpoint_model: Option<String>,
    context_window: Option<i64>,
    use_cache_prompt: bool,
}

impl StoredModel {
    fn is_usable(&self) -> bool {
        // An endpoint is usable as soon as it has a URL — there is no local
        // file to check (contrast curated/local, which require a present GGUF).
        if self.source_kind == ENDPOINT_SOURCE_KIND {
            return self.installed_at.is_some() && self.endpoint_url.is_some();
        }
        self.installed_at.is_some()
            && self
                .local_path
                .as_deref()
                .is_some_and(|path| Path::new(path).is_file())
    }

    /// Resolves this (already-usable) row into the turn-path [`ActiveModelTarget`].
    /// `api_key` is looked up separately by the caller from the endpoint key
    /// store; a local row ignores it. Falls back to the sidecar context window
    /// when a stored endpoint window is missing or non-positive.
    fn to_active_target(&self, api_key: Option<String>) -> Result<ActiveModelTarget, String> {
        if self.source_kind == ENDPOINT_SOURCE_KIND {
            let url = self
                .endpoint_url
                .clone()
                .ok_or_else(|| "the active endpoint has no URL".to_string())?;
            let context_window = self
                .context_window
                .filter(|window| *window > 0)
                .map(|window| window as u32)
                .unwrap_or(crate::inference::CONTEXT_WINDOW_TOKENS);
            Ok(ActiveModelTarget::Endpoint {
                url,
                model: self.endpoint_model.clone().unwrap_or_default(),
                api_key,
                context_window,
                clean_body: !self.use_cache_prompt,
            })
        } else {
            let path = self
                .local_path
                .clone()
                .ok_or_else(|| "the active model has no local file".to_string())?;
            Ok(ActiveModelTarget::Local { path })
        }
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
    /// Set only for `source_kind == "endpoint"` rows — the base URL and remote
    /// model id the Settings form renders (and edits). `None` for curated/local.
    pub endpoint_url: Option<String>,
    pub endpoint_model: Option<String>,
    pub state: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
}

/// Result of `test_model_endpoint` — a best-effort `GET {url}/models` probe the
/// Settings form uses to confirm reachability and reveal the endpoint's model
/// ids. `ok == false` with a populated `error` on any network/HTTP failure.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EndpointTestResult {
    pub ok: bool,
    pub models: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelState {
    pub hardware: HardwareProfile,
    pub options: Vec<ModelOption>,
    pub active_id: Option<String>,
    pub selected_id: Option<String>,
    pub fallback_notice: Option<String>,
    pub downloads: Vec<ModelDownload>,
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
                "SELECT id, local_path, installed_at, is_active, source_kind, display_name, \
             endpoint_url, endpoint_model, context_window, use_cache_prompt FROM models",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(StoredModel {
                    id: row.get(0)?,
                    local_path: row.get(1)?,
                    installed_at: row.get(2)?,
                    is_active: row.get::<_, i64>(3)? == 1,
                    source_kind: row.get(4)?,
                    display_name: row.get(5)?,
                    endpoint_url: row.get(6)?,
                    endpoint_model: row.get(7)?,
                    context_window: row.get(8)?,
                    use_cache_prompt: row.get::<_, i64>(9)? == 1,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()
        },
    )
    .await
    .map_err(|error| error.to_string())
}

#[derive(Debug, Clone)]
struct StoredDownload {
    model_id: String,
    display_name: Option<String>,
    state: String,
    bytes_downloaded: u64,
    bytes_total: u64,
    revision: u32,
    error: Option<String>,
}

async fn stored_downloads(
    conn: &tokio_rusqlite::Connection,
) -> Result<Vec<StoredDownload>, String> {
    conn.call(
        |conn: &mut Connection| -> rusqlite::Result<Vec<StoredDownload>> {
            let mut stmt = conn.prepare(
                "SELECT d.model_id, m.display_name, d.state, d.bytes_downloaded, d.bytes_total, d.revision, d.error \
                 FROM model_downloads d JOIN models m ON m.id = d.model_id \
                 ORDER BY d.updated_at, d.model_id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(StoredDownload {
                    model_id: row.get(0)?,
                    display_name: row.get(1)?,
                    state: row.get(2)?,
                    bytes_downloaded: row.get::<_, i64>(3)?.max(0) as u64,
                    bytes_total: row.get::<_, i64>(4)?.max(0) as u64,
                    revision: row.get::<_, i64>(5)?.max(0) as u32,
                    error: row.get(6)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()
        },
    )
    .await
    .map_err(|error| error.to_string())
}

async fn stored_download(
    conn: &tokio_rusqlite::Connection,
    model_id: &str,
) -> Result<Option<StoredDownload>, String> {
    let model_id = model_id.to_string();
    Ok(stored_downloads(conn)
        .await?
        .into_iter()
        .find(|download| download.model_id == model_id))
}

fn download_display_name(download: &StoredDownload) -> String {
    download.display_name.clone().unwrap_or_else(|| {
        model_registry::find_candidate(&model_registry::bundled(), &download.model_id)
            .map(|candidate| candidate.display_name.clone())
            .unwrap_or_else(|| download.model_id.clone())
    })
}

fn merge_download_snapshot(
    stored: &StoredDownload,
    live: Option<ModelInstallProgress>,
) -> ModelDownload {
    let use_live = live
        .as_ref()
        .is_some_and(|progress| progress.revision >= stored.revision);
    ModelDownload {
        model_id: stored.model_id.clone(),
        display_name: download_display_name(stored),
        state: if use_live {
            live.as_ref().unwrap().state.clone()
        } else {
            stored.state.clone()
        },
        bytes_downloaded: if use_live {
            live.as_ref().unwrap().bytes_downloaded
        } else {
            stored.bytes_downloaded
        },
        bytes_total: if use_live {
            live.as_ref().unwrap().bytes_total
        } else {
            stored.bytes_total
        },
        revision: if use_live {
            live.as_ref().unwrap().revision
        } else {
            stored.revision
        },
        error: if use_live {
            live.as_ref().unwrap().error.clone()
        } else {
            stored.error.clone()
        },
    }
}

async fn model_downloads(
    conn: &tokio_rusqlite::Connection,
    selection_state: &ModelSelectionState,
) -> Result<Vec<ModelDownload>, String> {
    Ok(stored_downloads(conn)
        .await?
        .into_iter()
        .map(|stored| {
            let live = selection_state.progress_for(&stored.model_id);
            merge_download_snapshot(&stored, live)
        })
        .collect())
}

async fn persist_download_state(
    conn: &tokio_rusqlite::Connection,
    model_id: &str,
    state: &str,
    bytes_downloaded: u64,
    bytes_total: u64,
    revision: u32,
    error: Option<String>,
) -> Result<(), String> {
    let model_id = model_id.to_string();
    let state = state.to_string();
    let now = now_ms();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO model_downloads (model_id, state, bytes_downloaded, bytes_total, revision, error, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(model_id) DO UPDATE SET state = excluded.state, bytes_downloaded = excluded.bytes_downloaded, \
             bytes_total = excluded.bytes_total, revision = excluded.revision, error = excluded.error, updated_at = excluded.updated_at \
             WHERE excluded.revision >= model_downloads.revision",
            rusqlite::params![
                model_id,
                state,
                bytes_downloaded as i64,
                bytes_total as i64,
                revision as i64,
                error,
                now
            ],
        )?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())
}

async fn begin_download_revision(
    conn: &tokio_rusqlite::Connection,
    model_id: &str,
    bytes_downloaded: u64,
    bytes_total: u64,
) -> Result<u32, String> {
    let model_id = model_id.to_string();
    let now = now_ms();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<u32> {
        let tx = conn.transaction()?;
        let previous = tx
            .query_row(
                "SELECT revision FROM model_downloads WHERE model_id = ?1",
                [&model_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        let revision = previous.saturating_add(1).min(u32::MAX as i64);
        tx.execute(
            "INSERT INTO model_downloads (model_id, state, bytes_downloaded, bytes_total, revision, error, updated_at) \
             VALUES (?1, 'queued', ?2, ?3, ?4, NULL, ?5) \
             ON CONFLICT(model_id) DO UPDATE SET state = 'queued', bytes_downloaded = excluded.bytes_downloaded, \
             bytes_total = excluded.bytes_total, revision = excluded.revision, error = NULL, updated_at = excluded.updated_at",
            rusqlite::params![model_id, bytes_downloaded as i64, bytes_total as i64, revision, now],
        )?;
        tx.commit()?;
        Ok(revision as u32)
    })
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

fn publish_download_state(
    app: &AppHandle,
    model_id: &str,
    revision: u32,
    state: impl Into<String>,
    bytes_downloaded: u64,
    bytes_total: u64,
    error: Option<String>,
) {
    downloader::publish_model_progress(
        app,
        ModelInstallProgress {
            model_id: model_id.to_string(),
            bytes_downloaded,
            bytes_total,
            state: state.into(),
            revision,
            error,
        },
    );
}

/// Activation still uses the historical install event so existing onboarding
/// can observe `preparing`/`active`, but these phases are deliberately not
/// recorded as download state. The durable transfer remains `completed` even
/// if loading the model into llama-server later fails.
fn emit_activation_state(
    app: &AppHandle,
    model_id: &str,
    state: impl Into<String>,
    error: Option<String>,
) {
    if error.is_some() {
        if let Some(selection) = app.try_state::<ModelSelectionState>() {
            selection.mark_activation_failed(model_id);
        }
    }
    let revision = app
        .try_state::<ModelSelectionState>()
        .and_then(|selection| selection.progress_for(model_id))
        .map(|progress| progress.revision)
        .unwrap_or(0);
    let _ = app.emit(
        "model-install-progress",
        ModelInstallProgress {
            model_id: model_id.to_string(),
            bytes_downloaded: 0,
            bytes_total: 0,
            state: state.into(),
            revision,
            error,
        },
    );
}

fn model_destination(app: &AppHandle, model_id: &str) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("models")
        .join(format!("{model_id}.gguf")))
}

fn partial_bytes(dest: &Path) -> u64 {
    let (part_path, _) = downloader::partial_paths(dest);
    std::fs::metadata(part_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
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
        emit_activation_state(
            app,
            model_id,
            if was_active { "active" } else { "installed" },
            None,
        );
        return Ok(false);
    }

    emit_activation_state(app, model_id, "preparing", None);
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
            emit_activation_state(app, model_id, "active", None);
        } else {
            restore_previous_server(app, conn, server_state, previous_path.as_deref(), false).await;
            emit_activation_state(app, model_id, "installed", None);
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

    if let Some(selection) = app.try_state::<ModelSelectionState>() {
        selection.clear_activation_failure(model_id);
    }
    emit_activation_state(app, model_id, "active", None);
    Ok(true)
}

#[derive(Debug, Clone, Copy)]
enum DownloadStartPolicy {
    /// A direct user intent (Select/Resume) or required missing-model
    /// fallback may start from any prior state.
    Explicit,
    /// Normal reconciliation may create a missing job or continue a running
    /// one, but must preserve durable Pause/Stop/Failure intent.
    Recoverable,
    /// Relaunch restoration is valid only for the exact running row observed
    /// by the restore scan. Any intervening control makes the snapshot stale.
    RestoreRunning { revision: u32 },
}

impl DownloadStartPolicy {
    fn allows(self, download: Option<&StoredDownload>) -> bool {
        match self {
            Self::Explicit => true,
            Self::Recoverable => !download.is_some_and(|download| {
                matches!(download.state.as_str(), "paused" | "stopped" | "failed")
            }),
            Self::RestoreRunning { revision } => download.is_some_and(|download| {
                download.revision == revision
                    && matches!(
                        download.state.as_str(),
                        "queued" | "downloading" | "verifying"
                    )
            }),
        }
    }
}

async fn begin_install(
    app: &AppHandle,
    conn: &tokio_rusqlite::Connection,
    selection_state: &ModelSelectionState,
    profile: &HardwareProfile,
    candidate: &ModelCandidate,
    start_policy: DownloadStartPolicy,
) -> Result<bool, String> {
    upsert_curated_model(conn, &profile.tier, candidate).await?;
    let dest = model_destination(app, &candidate.model_id)?;
    let dir = dest
        .parent()
        .ok_or_else(|| "model destination has no parent".to_string())?;
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;

    // One command gate makes the active-map check, durable revision bump,
    // reservation, and spawn atomic. Different model ids may still transfer
    // concurrently after this short critical section is released.
    let _download_lease = selection_state.download_lease().await;
    if selection_state
        .active_download(&candidate.model_id)
        .is_some()
    {
        return Ok(true);
    }

    // Automatic recovery makes its decision from a snapshot taken before it
    // reaches this gate. Pause/Stop may have committed while it waited, so
    // re-read the durable row here, at the same linearization point as the
    // revision bump and writer reservation. Explicit Select/Resume is the
    // only policy allowed to revive a terminal job.
    let durable_download = stored_download(conn, &candidate.model_id).await?;
    if !start_policy.allows(durable_download.as_ref()) {
        return Ok(false);
    }

    if stored_models(conn)
        .await?
        .into_iter()
        .find(|model| model.id == candidate.model_id)
        .is_some_and(|model| model.is_usable())
    {
        return Ok(false);
    }

    let bytes_downloaded = partial_bytes(&dest);
    let resumed = bytes_downloaded > 0;
    let revision = begin_download_revision(
        conn,
        &candidate.model_id,
        bytes_downloaded,
        candidate.size_bytes,
    )
    .await?;
    let cancel = CancellationToken::new();
    let (done_tx, done_rx) = watch::channel(false);
    let active = ActiveDownload {
        revision,
        cancel: cancel.clone(),
        done: done_rx,
    };
    if !selection_state.begin(&candidate.model_id, active) {
        return Ok(true);
    }

    publish_download_state(
        app,
        &candidate.model_id,
        revision,
        "queued",
        bytes_downloaded,
        candidate.size_bytes,
        None,
    );

    let app = app.clone();
    let conn = conn.clone();
    let selection_state = selection_state.clone();
    let candidate = candidate.clone();
    tauri::async_runtime::spawn(async move {
        let result = downloader::download_resumable(
            &app,
            downloader::DownloadRequest {
                model_id: &candidate.model_id,
                url: &candidate.source_url,
                dest: &dest,
                expected_sha256: &candidate.sha256,
                expected_size: candidate.size_bytes,
                revision,
            },
            &cancel,
        )
        .await;

        let mut should_activate = false;
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
                    let message = error.to_string();
                    let bytes = partial_bytes(&dest);
                    let _ = persist_download_state(
                        &conn,
                        &candidate.model_id,
                        "failed",
                        bytes,
                        candidate.size_bytes,
                        revision,
                        Some(message.clone()),
                    )
                    .await;
                    publish_download_state(
                        &app,
                        &candidate.model_id,
                        revision,
                        "failed",
                        bytes,
                        candidate.size_bytes,
                        Some(message),
                    );
                } else {
                    let completed_bytes = std::fs::metadata(&path)
                        .map(|metadata| metadata.len())
                        .unwrap_or(candidate.size_bytes);
                    let _ = persist_download_state(
                        &conn,
                        &candidate.model_id,
                        "completed",
                        completed_bytes,
                        completed_bytes,
                        revision,
                        None,
                    )
                    .await;
                    publish_download_state(
                        &app,
                        &candidate.model_id,
                        revision,
                        "completed",
                        completed_bytes,
                        completed_bytes,
                        None,
                    );
                    should_activate = true;
                }
            }
            Err(downloader::DownloadError::Cancelled) => {}
            Err(error) => {
                let message = error.to_string();
                let bytes = partial_bytes(&dest);
                let _ = persist_download_state(
                    &conn,
                    &candidate.model_id,
                    "failed",
                    bytes,
                    candidate.size_bytes,
                    revision,
                    Some(message.clone()),
                )
                .await;
                publish_download_state(
                    &app,
                    &candidate.model_id,
                    revision,
                    "failed",
                    bytes,
                    candidate.size_bytes,
                    Some(message),
                );
            }
        }

        // Controls wait on this exact signal before touching `.part`, so a
        // resumed worker can never overlap the cancelled writer.
        selection_state.finish(&candidate.model_id, revision);
        let _ = done_tx.send(true);

        if should_activate
            && setting_string(&conn, SELECTED_MODEL_SETTING)
                .await
                .ok()
                .flatten()
                .as_deref()
                == Some(candidate.model_id.as_str())
        {
            if let Some(server_state) = app.try_state::<crate::inference::server::ServerState>() {
                if let Err(error) =
                    activate_model(&app, &conn, &server_state, &candidate.model_id, true).await
                {
                    if error.previous_healthy {
                        let _ = restore_healthy_active_selection_if_current(
                            &conn,
                            &selection_state,
                            &candidate.model_id,
                        )
                        .await;
                    }
                    let message = error.to_string();
                    emit_activation_state(
                        &app,
                        &candidate.model_id,
                        format!("error: {message}"),
                        Some(message),
                    );
                }
            } else {
                emit_activation_state(&app, &candidate.model_id, "installed", None);
            }
        } else if should_activate {
            emit_activation_state(&app, &candidate.model_id, "installed", None);
        }
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
            let message = error.to_string();
            emit_activation_state(
                &app,
                &candidate.model_id,
                format!("error: {message}"),
                Some(message),
            );
            return Err(error.to_string());
        }
        false
    } else {
        begin_install(
            &app,
            &conn,
            &selection_state,
            &profile,
            &candidate,
            DownloadStartPolicy::Explicit,
        )
        .await?
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
    let conn = db_cell.get(&app).await?;
    let model = stored_models(conn)
        .await?
        .into_iter()
        .find(|model| model.id == model_id);
    if let Some(model) = model.as_ref() {
        if model.is_active && model.is_usable() {
            return Ok(ModelInstallStatus {
                state: "active".to_string(),
                bytes_downloaded: 0,
                bytes_total: 0,
            });
        }
        if model.is_usable() {
            return Ok(ModelInstallStatus {
                state: "installed".to_string(),
                bytes_downloaded: 0,
                bytes_total: 0,
            });
        }
    }
    if let Some(stored) = stored_download(conn, &model_id).await? {
        let download = merge_download_snapshot(&stored, selection_state.progress_for(&model_id));
        return Ok(ModelInstallStatus {
            state: download.state,
            bytes_downloaded: download.bytes_downloaded,
            bytes_total: download.bytes_total,
        });
    }
    Ok(ModelInstallStatus {
        state: "idle".to_string(),
        bytes_downloaded: 0,
        bytes_total: 0,
    })
}

async fn authoritative_download(
    conn: &tokio_rusqlite::Connection,
    selection_state: &ModelSelectionState,
    model_id: &str,
) -> Result<ModelDownload, String> {
    let stored = stored_download(conn, model_id)
        .await?
        .ok_or_else(|| "that model has no download to control".to_string())?;
    Ok(merge_download_snapshot(
        &stored,
        selection_state.progress_for(model_id),
    ))
}

async fn cancel_and_wait(active: ActiveDownload) -> u32 {
    let revision = active.revision;
    active.cancel.cancel();
    let mut done = active.done;
    while !*done.borrow() {
        if done.changed().await.is_err() {
            break;
        }
    }
    revision
}

async fn installed_model_is_usable(
    conn: &tokio_rusqlite::Connection,
    model_id: &str,
) -> Result<bool, String> {
    Ok(stored_models(conn)
        .await?
        .into_iter()
        .find(|model| model.id == model_id)
        .is_some_and(|model| model.is_usable()))
}

#[tauri::command]
#[specta::specta]
pub async fn pause_model_download(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    selection_state: State<'_, ModelSelectionState>,
    model_id: String,
) -> Result<ModelDownload, String> {
    let conn = db_cell.get(&app).await?.clone();
    let _download_lease = selection_state.download_lease().await;
    if let Some(active) = selection_state.active_download(&model_id) {
        let revision = cancel_and_wait(active).await;
        selection_state.finish(&model_id, revision);
    }

    let current = authoritative_download(&conn, &selection_state, &model_id).await?;
    if installed_model_is_usable(&conn, &model_id).await? {
        return Ok(current);
    }
    if !matches!(
        current.state.as_str(),
        "queued" | "downloading" | "verifying"
    ) {
        return Ok(current);
    }
    let dest = model_destination(&app, &model_id)?;
    let bytes = partial_bytes(&dest);
    let revision = current.revision.saturating_add(1);
    persist_download_state(
        &conn,
        &model_id,
        "paused",
        bytes,
        current.bytes_total,
        revision,
        None,
    )
    .await?;
    publish_download_state(
        &app,
        &model_id,
        revision,
        "paused",
        bytes,
        current.bytes_total,
        None,
    );
    authoritative_download(&conn, &selection_state, &model_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn resume_model_download(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    selection_state: State<'_, ModelSelectionState>,
    model_id: String,
) -> Result<ModelDownload, String> {
    let conn = db_cell.get(&app).await?.clone();
    let registry = model_registry::bundled();
    let candidate = model_registry::find_candidate(&registry, &model_id)
        .cloned()
        .ok_or_else(|| "that curated model is no longer available".to_string())?;
    let profile = hardware::detect();
    begin_install(
        &app,
        &conn,
        &selection_state,
        &profile,
        &candidate,
        DownloadStartPolicy::Explicit,
    )
    .await?;
    authoritative_download(&conn, &selection_state, &model_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_model_download(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    selection_state: State<'_, ModelSelectionState>,
    model_id: String,
) -> Result<ModelDownload, String> {
    let conn = db_cell.get(&app).await?.clone();
    let _download_lease = selection_state.download_lease().await;
    if let Some(active) = selection_state.active_download(&model_id) {
        let revision = cancel_and_wait(active).await;
        selection_state.finish(&model_id, revision);
    }

    let current = authoritative_download(&conn, &selection_state, &model_id).await?;
    // The verified final rename is the commit point. If completion won the
    // race, Stop is an idempotent no-op and never removes the final GGUF.
    if current.state == "completed" || installed_model_is_usable(&conn, &model_id).await? {
        return Ok(current);
    }
    if current.state == "stopped" {
        return Ok(current);
    }

    let dest = model_destination(&app, &model_id)?;
    downloader::remove_partial_files(&dest).map_err(|error| error.to_string())?;
    let revision = current.revision.saturating_add(1);
    persist_download_state(
        &conn,
        &model_id,
        "stopped",
        0,
        current.bytes_total,
        revision,
        None,
    )
    .await?;
    publish_download_state(
        &app,
        &model_id,
        revision,
        "stopped",
        0,
        current.bytes_total,
        None,
    );
    authoritative_download(&conn, &selection_state, &model_id).await
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
    downloads: &[ModelDownload],
    model_id: &str,
    installed: bool,
    active: bool,
) -> (String, u64, u64) {
    if active {
        return ("active".to_string(), 0, 0);
    }
    if let Some(progress) = downloads
        .iter()
        .find(|download| download.model_id == model_id)
    {
        if !(progress.state == "completed" && installed) {
            return (
                progress.state.clone(),
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
    let downloads = model_downloads(conn, selection_state).await?;
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
            progress_state(&downloads, &candidate.model_id, installed, active);
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
            endpoint_url: None,
            endpoint_model: None,
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
            progress_state(&downloads, &model.id, installed, active);
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
            endpoint_url: None,
            endpoint_model: None,
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
            progress_state(&downloads, &model.id, installed, active);
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
            endpoint_url: None,
            endpoint_model: None,
            state,
            bytes_downloaded,
            bytes_total,
        });
    }

    // Endpoint models: surfaced like the local loop above (active or the
    // durable selection). No file, no download — just the URL/model the form
    // stored. `is_usable`/`active_id` already treat a URL-bearing endpoint as
    // ready, so `progress_state` renders it "ready"/"active".
    for model in stored.iter().filter(|model| {
        model.source_kind == ENDPOINT_SOURCE_KIND
            && (model.is_active || selected_id.as_deref() == Some(model.id.as_str()))
    }) {
        let installed = model.is_usable();
        let active = active_id.as_deref() == Some(model.id.as_str());
        let selected = selected_id.as_deref() == Some(model.id.as_str());
        let (state, bytes_downloaded, bytes_total) =
            progress_state(&downloads, &model.id, installed, active);
        options.push(ModelOption {
            id: model.id.clone(),
            display_name: model
                .display_name
                .clone()
                .unwrap_or_else(|| "Custom endpoint".to_string()),
            description: "An OpenAI-compatible model endpoint.".to_string(),
            technical_name: model.endpoint_model.clone().unwrap_or_default(),
            parameter_count: "Endpoint".to_string(),
            quantization: "API".to_string(),
            size_bytes: 0,
            recommended: false,
            installed,
            active,
            selected,
            source_kind: ENDPOINT_SOURCE_KIND.to_string(),
            local_path: None,
            endpoint_url: model.endpoint_url.clone(),
            endpoint_model: model.endpoint_model.clone(),
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
        downloads,
    })
}

enum ReconcileAction {
    None,
    Activate(String),
    Install(Box<ModelCandidate>, DownloadStartPolicy),
}

async fn restore_running_downloads(
    app: &AppHandle,
    conn: &tokio_rusqlite::Connection,
    selection_state: &ModelSelectionState,
) -> Result<(), String> {
    let mut restored = selection_state.restored_downloads.lock().await;
    if *restored {
        return Ok(());
    }

    let registry = model_registry::bundled();
    let profile = hardware::detect();
    let downloads = stored_downloads(conn).await?;
    for download in downloads.into_iter().filter(|download| {
        matches!(
            download.state.as_str(),
            "queued" | "downloading" | "verifying"
        )
    }) {
        if installed_model_is_usable(conn, &download.model_id).await? {
            let dest = model_destination(app, &download.model_id)?;
            let bytes = std::fs::metadata(dest)
                .map(|metadata| metadata.len())
                .unwrap_or(download.bytes_total);
            persist_download_state(
                conn,
                &download.model_id,
                "completed",
                bytes,
                bytes,
                download.revision,
                None,
            )
            .await?;
            continue;
        }
        let Some(candidate) = model_registry::find_candidate(&registry, &download.model_id) else {
            persist_download_state(
                conn,
                &download.model_id,
                "failed",
                download.bytes_downloaded,
                download.bytes_total,
                download.revision,
                Some("the curated model is no longer available".to_string()),
            )
            .await?;
            continue;
        };
        begin_install(
            app,
            conn,
            selection_state,
            &profile,
            candidate,
            DownloadStartPolicy::RestoreRunning {
                revision: download.revision,
            },
        )
        .await?;
    }
    *restored = true;
    Ok(())
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
    restore_running_downloads(app, conn, selection_state).await?;
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
                ReconcileAction::Install(Box::new(target), DownloadStartPolicy::Explicit)
            }
        } else if let Some(selected_id) = selected {
            // Reconstruct work after a relaunch: a durable selection can point
            // to an installed model awaiting handoff or to a curated download
            // whose `.part` file should resume. The old active model remains
            // available while that work proceeds.
            let blocked_download = selection_state.activation_failed(&selected_id)
                || stored_download(conn, &selected_id)
                    .await?
                    .is_some_and(|download| {
                        matches!(download.state.as_str(), "paused" | "stopped" | "failed")
                    });
            if models
                .iter()
                .any(|model| model.id == selected_id && model.is_active && model.is_usable())
            {
                ReconcileAction::None
            } else if blocked_download {
                // Paused, stopped, and failed are durable user-visible states.
                // Snapshot reads and generation preflight must not turn them
                // back into downloads; explicit Select/Resume is the trigger.
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
                        ReconcileAction::Install(
                            Box::new(candidate),
                            DownloadStartPolicy::Recoverable,
                        )
                    }
                } else {
                    ReconcileAction::None
                }
            } else if let Some(candidate) =
                model_registry::find_candidate(&registry, &selected_id).cloned()
            {
                upsert_curated_model(conn, &profile.tier, &candidate).await?;
                ReconcileAction::Install(Box::new(candidate), DownloadStartPolicy::Recoverable)
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
                let message = error.to_string();
                emit_activation_state(app, &model_id, format!("error: {message}"), Some(message));
            }
        }
        ReconcileAction::Install(candidate, start_policy) => {
            begin_install(
                app,
                conn,
                selection_state,
                &profile,
                &candidate,
                start_policy,
            )
            .await?;
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

/// Endpoint-aware sibling of `ensure_usable_model_path`'s PREFLIGHT half, run
/// before a turn takes the generation lease (a fallback activation needs the
/// exclusive side of the same gate, so reconciliation must happen here, not
/// while the lease is held). Heals a stale active pointer, then confirms a
/// usable active model exists — a present GGUF for curated/local, or a
/// URL-bearing endpoint. Does NOT return a path (an endpoint has none); the
/// turn re-resolves the concrete target after the lease via
/// [`active_model_target`].
pub async fn ensure_active_model_ready(
    app: &AppHandle,
    conn: &tokio_rusqlite::Connection,
    server_state: &crate::inference::server::ServerState,
) -> Result<(), String> {
    let selection_state = app
        .try_state::<ModelSelectionState>()
        .ok_or_else(|| "model selection state is unavailable".to_string())?;
    reconcile_active_model(app, conn, &selection_state, server_state).await?;
    if stored_models(conn)
        .await?
        .iter()
        .any(|model| model.is_active && model.is_usable())
    {
        Ok(())
    } else {
        Err("The selected model is still being prepared. Try again in a moment.".to_string())
    }
}

/// Read the active model's turn-path target again after a generation lease has
/// been acquired (a switch may have completed between preflight reconciliation
/// and lease acquisition, so callers must not reuse the preflight result). Pure
/// read — no reconcile, no server work — so it never tries to take the switch
/// lease while a generation lease is held. `Endpoint` for an endpoint active
/// model (with its API key pulled from `endpoint_keys`), `Local` otherwise.
pub async fn active_model_target(
    conn: &tokio_rusqlite::Connection,
    endpoint_keys: &EndpointKeyStore,
) -> Result<ActiveModelTarget, String> {
    let model = stored_models(conn)
        .await?
        .into_iter()
        .find(|model| model.is_active && model.is_usable())
        .ok_or_else(|| "No model is ready to use yet.".to_string())?;
    let api_key = if model.source_kind == ENDPOINT_SOURCE_KIND {
        endpoint_keys.get(&model.id).ok().flatten()
    } else {
        None
    };
    model.to_active_target(api_key)
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
            let message = error.to_string();
            emit_activation_state(
                &app,
                &candidate.model_id,
                format!("error: {message}"),
                Some(message),
            );
            return Err(error.to_string());
        }
    } else {
        begin_install(
            &app,
            &conn,
            &selection_state,
            &profile,
            &candidate,
            DownloadStartPolicy::Explicit,
        )
        .await?;
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
        let message = error.to_string();
        emit_activation_state(&app, &model_id, format!("error: {message}"), Some(message));
        return Err(error.to_string());
    }
    build_model_state(&conn, &selection_state).await
}

/// Rejects a non-http(s) endpoint URL and returns the trimmed value (trailing
/// slash removed so the `/v1/chat/completions` and `/models` joins never
/// double-slash). The Settings form validates too, but this is the trust
/// boundary — the command never persists an unusable base URL.
fn validate_endpoint_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Ok(trimmed.to_string())
    } else {
        Err("Endpoint URL must start with http:// or https://".to_string())
    }
}

/// A stable, generated id for an endpoint row: same URL + model reuses the same
/// row (the command upserts). `kind` is deliberately excluded so re-classifying
/// the same endpoint edits in place rather than orphaning a row.
fn endpoint_model_id(url: &str, model: &str) -> String {
    let digest = Sha256::digest(format!("{url}\n{model}").as_bytes());
    format!("endpoint-{:x}", digest)
}

/// Parses an OpenAI `GET /models` body (`{"data":[{"id":"..."}]}`) into the
/// list of model ids. Tolerant: a body that isn't that shape yields an empty
/// list rather than an error, so `test_model_endpoint` still reports `ok`.
fn parse_model_ids(body: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .as_ref()
        .and_then(|value| value.get("data"))
        .and_then(|data| data.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("id").and_then(|id| id.as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_endpoint_models(url: &str, api_key: Option<&str>) -> EndpointTestResult {
    let base = url.trim().trim_end_matches('/');
    let models_url = format!("{base}/models");
    let mut request = reqwest::Client::new().get(&models_url);
    if let Some(key) = api_key.filter(|key| !key.is_empty()) {
        request = request.header("authorization", format!("Bearer {key}"));
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) if status.is_success() => EndpointTestResult {
                    ok: true,
                    models: parse_model_ids(&body),
                    error: None,
                },
                Ok(body) => EndpointTestResult {
                    ok: false,
                    models: Vec::new(),
                    error: Some(format!(
                        "HTTP {status}: {}",
                        body.chars().take(200).collect::<String>()
                    )),
                },
                Err(error) => EndpointTestResult {
                    ok: false,
                    models: Vec::new(),
                    error: Some(error.to_string()),
                },
            }
        }
        Err(error) => EndpointTestResult {
            ok: false,
            models: Vec::new(),
            error: Some(error.to_string()),
        },
    }
}

#[allow(clippy::too_many_arguments)]
async fn upsert_endpoint_model(
    conn: &tokio_rusqlite::Connection,
    id: &str,
    display_name: &str,
    url: &str,
    model: &str,
    context_window: Option<i64>,
    use_cache_prompt: bool,
    now: i64,
) -> Result<(), String> {
    let id = id.to_string();
    let display_name = display_name.to_string();
    let url = url.to_string();
    let model = model.to_string();
    conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO models (id, hardware_tier, source_url, quantization, sha256, capability_tags, installed_at, is_active, source_kind, display_name, endpoint_url, endpoint_model, context_window, use_cache_prompt)\
             VALUES (?1, 'endpoint', '', 'API', '', '[\"endpoint\"]', ?2, 0, 'endpoint', ?3, ?4, ?5, ?6, ?7)\
             ON CONFLICT(id) DO UPDATE SET installed_at = excluded.installed_at, source_kind = 'endpoint',\
             display_name = excluded.display_name, endpoint_url = excluded.endpoint_url,\
             endpoint_model = excluded.endpoint_model, context_window = excluded.context_window,\
             use_cache_prompt = excluded.use_cache_prompt",
            rusqlite::params![
                id,
                now,
                display_name,
                url,
                model,
                context_window,
                use_cache_prompt as i64,
            ],
        )?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())
}

/// Selects a custom OpenAI-compatible endpoint as the active model. Upserts an
/// `endpoint` row (installed at once, so it reads as usable), stores the API
/// key in the endpoint key store keyed by the row id (never in SQLite), records
/// the durable selection, and marks the row the single active model WITHOUT
/// spawning a sidecar (and shuts any running one down). `kind` is informational
/// (local/hosted/lan); the behavioral bit is `use_cache_prompt`.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn select_endpoint_model(
    app: AppHandle,
    db_cell: State<'_, DbCell>,
    server_state: State<'_, crate::inference::server::ServerState>,
    kind: String,
    url: String,
    model: String,
    api_key: Option<String>,
    context_window: u32,
    use_cache_prompt: bool,
) -> Result<ModelState, String> {
    // `kind` is stored implicitly via the row's `source_kind = 'endpoint'`;
    // its local/hosted/lan value is informational and not persisted separately.
    let _ = &kind;
    let url = validate_endpoint_url(&url)?;
    let model = model.trim().to_string();
    let id = endpoint_model_id(&url, &model);
    let display_name = if model.is_empty() {
        url.clone()
    } else {
        model.clone()
    };
    let stored_window = (context_window > 0).then_some(context_window as i64);
    let now = now_ms();

    let conn = db_cell.get(&app).await?.clone();
    let selection_state = app
        .try_state::<ModelSelectionState>()
        .ok_or_else(|| "model selection state is unavailable".to_string())?;
    let endpoint_keys = app
        .try_state::<EndpointKeyStore>()
        .ok_or_else(|| "endpoint key store is unavailable".to_string())?;

    {
        let _intent_lease = selection_state.intent_lease().await;
        upsert_endpoint_model(
            &conn,
            &id,
            &display_name,
            &url,
            &model,
            stored_window,
            use_cache_prompt,
            now,
        )
        .await?;
        // The API key lives in the OS secret store only. A None/empty key
        // clears any prior key for this row (re-selecting without a key).
        match api_key.as_deref().filter(|key| !key.is_empty()) {
            Some(key) => endpoint_keys.set(&id, key)?,
            None => endpoint_keys.delete(&id)?,
        }
        write_setting_string(&conn, SELECTED_MODEL_SETTING, &id).await?;
        clear_setting(&conn, FALLBACK_NOTICE_SETTING).await?;
    }

    // Endpoints never spawn a sidecar. Take the switch lease (so an in-flight
    // generation drains first), tear down any running local sidecar, and flip
    // the single active row — the whole point being to skip `ensure_running`.
    {
        let _switch_lease = server_state.switch_lease().await;
        server_state.shutdown(&app).await;
        set_active_model_row(&conn, &id).await?;
    }
    if let Some(selection) = app.try_state::<ModelSelectionState>() {
        selection.clear_activation_failure(&id);
    }
    emit_activation_state(&app, &id, "active", None);

    build_model_state(&conn, &selection_state).await
}

/// Best-effort `GET {url}/models` probe for the Settings form's "Test" button:
/// reports reachability and the endpoint's model ids (Bearer auth when a key is
/// given). Network/HTTP failures come back as `ok: false` + `error`, never a
/// command error, so the form can show the reason inline.
#[tauri::command]
#[specta::specta]
pub async fn test_model_endpoint(
    url: String,
    api_key: Option<String>,
) -> Result<EndpointTestResult, String> {
    let url = validate_endpoint_url(&url)?;
    Ok(fetch_endpoint_models(&url, api_key.as_deref()).await)
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
        assert!(state.record_progress(ModelInstallProgress {
            model_id: "advanced".to_string(),
            bytes_downloaded: 50,
            bytes_total: 100,
            state: "downloading".to_string(),
            revision: 2,
            error: None,
        }));
        let downloads = vec![ModelDownload {
            model_id: "advanced".to_string(),
            display_name: "Advanced".to_string(),
            state: "downloading".to_string(),
            bytes_downloaded: 50,
            bytes_total: 100,
            revision: 2,
            error: None,
        }];
        assert_eq!(
            progress_state(&downloads, "advanced", false, false),
            ("downloading".to_string(), 50, 100)
        );
        assert_eq!(
            progress_state(&downloads, "balanced", true, true),
            ("active".to_string(), 0, 0)
        );

        assert!(!state.record_progress(ModelInstallProgress {
            model_id: "advanced".to_string(),
            bytes_downloaded: 25,
            bytes_total: 100,
            state: "downloading".to_string(),
            revision: 1,
            error: None,
        }));
        assert_eq!(state.progress_for("advanced").unwrap().bytes_downloaded, 50);
    }

    #[test]
    fn one_writer_reservation_is_idempotent_and_revision_guarded() {
        let state = ModelSelectionState::default();
        let (_done_tx, done) = watch::channel(false);
        let first = ActiveDownload {
            revision: 4,
            cancel: CancellationToken::new(),
            done: done.clone(),
        };
        assert!(state.begin("balanced", first));
        assert!(!state.begin(
            "balanced",
            ActiveDownload {
                revision: 5,
                cancel: CancellationToken::new(),
                done,
            }
        ));
        state.finish("balanced", 3);
        assert_eq!(state.active_download("balanced").unwrap().revision, 4);
        state.finish("balanced", 4);
        assert!(state.active_download("balanced").is_none());
    }

    #[test]
    fn automatic_start_policies_preserve_intervening_pause_and_stop() {
        let stored = |state: &str, revision: u32| StoredDownload {
            model_id: "balanced".to_string(),
            display_name: Some("Balanced".to_string()),
            state: state.to_string(),
            bytes_downloaded: 40,
            bytes_total: 100,
            revision,
            error: None,
        };
        let paused = stored("paused", 8);
        let stopped = stored("stopped", 9);
        let running = stored("downloading", 7);

        assert!(!DownloadStartPolicy::Recoverable.allows(Some(&paused)));
        assert!(!DownloadStartPolicy::Recoverable.allows(Some(&stopped)));
        assert!(DownloadStartPolicy::Explicit.allows(Some(&stopped)));
        assert!(DownloadStartPolicy::Recoverable.allows(None));

        let restore = DownloadStartPolicy::RestoreRunning { revision: 7 };
        assert!(restore.allows(Some(&running)));
        assert!(!restore.allows(Some(&paused)));
        assert!(!DownloadStartPolicy::RestoreRunning { revision: 6 }.allows(Some(&running)));
        assert!(!restore.allows(None));
    }

    fn endpoint_row(id: &str) -> StoredModel {
        StoredModel {
            id: id.to_string(),
            local_path: None,
            installed_at: Some(1),
            is_active: true,
            source_kind: ENDPOINT_SOURCE_KIND.to_string(),
            display_name: Some("My API".to_string()),
            endpoint_url: Some("https://api.example.test".to_string()),
            endpoint_model: Some("gpt-4o-mini".to_string()),
            context_window: Some(32768),
            use_cache_prompt: false,
        }
    }

    #[test]
    fn endpoint_is_usable_with_a_url_and_no_local_file() {
        let endpoint = endpoint_row("endpoint-x");
        assert!(endpoint.is_usable(), "a URL-bearing endpoint is usable");

        let no_url = StoredModel {
            endpoint_url: None,
            ..endpoint.clone()
        };
        assert!(!no_url.is_usable(), "an endpoint with no URL is not usable");

        // A local row with a missing file stays unusable — endpoints don't
        // change the local file check.
        let local = StoredModel {
            id: "local-x".to_string(),
            local_path: Some("/nope/missing.gguf".to_string()),
            installed_at: Some(1),
            is_active: false,
            source_kind: LOCAL_SOURCE_KIND.to_string(),
            display_name: None,
            endpoint_url: None,
            endpoint_model: None,
            context_window: None,
            use_cache_prompt: false,
        };
        assert!(!local.is_usable());
    }

    #[test]
    fn endpoint_target_derives_clean_body_and_window_from_the_row() {
        // use_cache_prompt=false -> clean_body=true; stored window is honored.
        let target = endpoint_row("e1")
            .to_active_target(Some("sk-key".to_string()))
            .unwrap();
        assert_eq!(
            target,
            ActiveModelTarget::Endpoint {
                url: "https://api.example.test".to_string(),
                model: "gpt-4o-mini".to_string(),
                api_key: Some("sk-key".to_string()),
                context_window: 32768,
                clean_body: true,
            }
        );

        // use_cache_prompt=true -> clean_body=false; missing/zero window falls
        // back to the sidecar default; no key -> None.
        let row = StoredModel {
            use_cache_prompt: true,
            context_window: None,
            ..endpoint_row("e2")
        };
        match row.to_active_target(None).unwrap() {
            ActiveModelTarget::Endpoint {
                clean_body,
                context_window,
                api_key,
                ..
            } => {
                assert!(!clean_body);
                assert_eq!(context_window, crate::inference::CONTEXT_WINDOW_TOKENS);
                assert!(api_key.is_none());
            }
            other => panic!("expected Endpoint, got {other:?}"),
        }
    }

    #[test]
    fn validate_endpoint_url_requires_an_http_scheme_and_trims() {
        assert_eq!(
            validate_endpoint_url("  https://x.test/  ").unwrap(),
            "https://x.test"
        );
        assert_eq!(
            validate_endpoint_url("http://127.0.0.1:1234").unwrap(),
            "http://127.0.0.1:1234"
        );
        assert!(validate_endpoint_url("ftp://x.test").is_err());
        assert!(validate_endpoint_url("x.test").is_err());
    }

    #[test]
    fn parse_model_ids_reads_the_openai_models_shape() {
        let body = r#"{"object":"list","data":[{"id":"gpt-4o-mini","object":"model"},{"id":"llama-3.1","object":"model"}]}"#;
        assert_eq!(
            parse_model_ids(body),
            vec!["gpt-4o-mini".to_string(), "llama-3.1".to_string()]
        );
        assert!(parse_model_ids("not json").is_empty());
        assert!(parse_model_ids(r#"{"error":"nope"}"#).is_empty());
    }

    #[tokio::test]
    async fn endpoint_selection_upserts_activates_and_resolves_without_a_file() {
        // Exercises the pieces `select_endpoint_model` composes, against the
        // in-memory secret store (never the real Keychain).
        let conn = crate::storage::test_async_connection().await;
        let keys = EndpointKeyStore::new(std::sync::Arc::new(crate::oauth::InMemoryStore::new()));

        let url = validate_endpoint_url("https://api.example.test/").unwrap();
        let id = endpoint_model_id(&url, "gpt-4o-mini");
        upsert_endpoint_model(
            &conn,
            &id,
            "gpt-4o-mini",
            &url,
            "gpt-4o-mini",
            Some(32768),
            false,
            123,
        )
        .await
        .unwrap();
        keys.set(&id, "sk-secret").unwrap();
        write_setting_string(&conn, SELECTED_MODEL_SETTING, &id)
            .await
            .unwrap();
        set_active_model_row(&conn, &id).await.unwrap();

        // No local GGUF exists, yet the endpoint resolves as the active target
        // with its key pulled from the store.
        let target = active_model_target(&conn, &keys).await.unwrap();
        assert_eq!(
            target,
            ActiveModelTarget::Endpoint {
                url: "https://api.example.test".to_string(),
                model: "gpt-4o-mini".to_string(),
                api_key: Some("sk-secret".to_string()),
                context_window: 32768,
                clean_body: true,
            }
        );

        let models = stored_models(&conn).await.unwrap();
        assert_eq!(
            models.iter().filter(|model| model.is_active).count(),
            1,
            "single active row invariant holds"
        );
        assert!(models
            .iter()
            .any(|model| model.id == id && model.is_active && model.is_usable()));
    }

    #[tokio::test]
    async fn active_model_target_returns_local_for_a_local_active_model() {
        let conn = crate::storage::test_async_connection().await;
        let keys = EndpointKeyStore::new(std::sync::Arc::new(crate::oauth::InMemoryStore::new()));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.gguf");
        std::fs::write(&path, b"GGUF\x03\x00\x00\x00").unwrap();
        let path = path.to_string_lossy().to_string();
        let row_path = path.clone();
        conn.call(move |conn: &mut Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO models (id, hardware_tier, source_url, quantization, sha256, local_path, capability_tags, installed_at, is_active, source_kind)\
                 VALUES ('local-a', 'local', '', 'GGUF', '', ?1, '[]', 1, 1, 'local')",
                [row_path],
            )?;
            Ok(())
        })
        .await
        .unwrap();

        let target = active_model_target(&conn, &keys).await.unwrap();
        assert_eq!(target, ActiveModelTarget::Local { path });
    }

    #[tokio::test]
    async fn durable_paused_download_round_trips_with_live_overlay() {
        let conn = crate::storage::test_async_connection().await;
        conn.call(|conn: &mut Connection| -> rusqlite::Result<()> {
            conn.execute(
                "INSERT INTO models (id, hardware_tier, source_url, quantization, sha256, capability_tags, source_kind, display_name)\
                 VALUES ('balanced', '32gb', 'https://example.test/model', 'Q4_K_M', 'sha', '[]', 'curated', 'Balanced')",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        persist_download_state(&conn, "balanced", "paused", 40, 100, 2, None)
            .await
            .unwrap();
        let state = ModelSelectionState::default();
        let snapshot = authoritative_download(&conn, &state, "balanced")
            .await
            .unwrap();
        assert_eq!(snapshot.state, "paused");
        assert_eq!(snapshot.bytes_downloaded, 40);
        assert_eq!(snapshot.revision, 2);

        assert!(state.record_progress(ModelInstallProgress {
            model_id: "balanced".to_string(),
            bytes_downloaded: 75,
            bytes_total: 100,
            state: "downloading".to_string(),
            revision: 3,
            error: None,
        }));
        let live = authoritative_download(&conn, &state, "balanced")
            .await
            .unwrap();
        assert_eq!(live.state, "downloading");
        assert_eq!(live.bytes_downloaded, 75);
        assert_eq!(live.revision, 3);
    }
}
