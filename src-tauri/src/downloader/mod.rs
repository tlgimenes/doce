use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
}

#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ModelInstallProgress {
    pub model_id: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PartialIdentity {
    source_url: String,
    sha256: String,
    size: u64,
}

fn remove_file_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Keep the recoverable backend snapshot and the live frontend event in the
/// same order. Settings can therefore close/reopen without losing the current
/// phase or byte counts.
pub fn publish_model_progress(app: &AppHandle, progress: ModelInstallProgress) {
    if let Some(state) = app.try_state::<crate::commands::models::ModelSelectionState>() {
        state.record_progress(progress.clone());
    }
    let _ = app.emit("model-install-progress", progress);
}

fn publish(app: &AppHandle, model_id: &str, bytes_downloaded: u64, bytes_total: u64, state: &str) {
    publish_model_progress(
        app,
        ModelInstallProgress {
            model_id: model_id.to_string(),
            bytes_downloaded,
            bytes_total,
            state: state.to_string(),
        },
    );
}

/// Resumable, checksum-verified download. A final GGUF left by a prior crash
/// is verified and adopted; partial downloads resume only when the server
/// actually honors the Range request. Corrupt partial/final files are reset
/// so Retry can make progress instead of re-verifying the same bad bytes.
pub async fn download_resumable(
    app: &AppHandle,
    model_id: &str,
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    expected_size: u64,
) -> Result<PathBuf, DownloadError> {
    let client = reqwest::Client::new();

    if dest.is_file() {
        let total = std::fs::metadata(dest)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let actual = hash_file_with_progress(app, model_id, dest, total).await?;
        if actual == expected_sha256 {
            publish(app, model_id, total, total, "downloaded");
            return Ok(dest.to_path_buf());
        }
        std::fs::remove_file(dest)?;
    }

    let part_path = dest.with_extension("part");
    let identity_path = part_path.with_extension("part.meta");
    let identity = PartialIdentity {
        source_url: url.to_string(),
        sha256: expected_sha256.to_string(),
        size: expected_size,
    };
    let mut existing_len = std::fs::metadata(&part_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if existing_len > 0 {
        match std::fs::read_to_string(&identity_path) {
            Ok(raw) => {
                let matches =
                    serde_json::from_str::<PartialIdentity>(&raw).ok().as_ref() == Some(&identity);
                if !matches {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .open(&part_path)?;
                    existing_len = 0;
                }
            }
            // A partial created by an older Doce version has no sidecar. It
            // remains resumable and will be protected by SHA verification.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    std::fs::write(
        &identity_path,
        serde_json::to_vec(&identity).map_err(std::io::Error::other)?,
    )?;

    // HEAD is an optimization, not a prerequisite: several otherwise valid
    // artifact hosts reject it. The signed registry size keeps progress and
    // resume decisions determinate when HEAD fails or omits the header.
    let head_len = match client.head(url).send().await {
        Ok(response) if response.status().is_success() => response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok()),
        _ => None,
    };
    let total_len = head_len.filter(|size| *size > 0).unwrap_or(expected_size);
    let already_complete = total_len > 0 && existing_len == total_len;
    let downloaded = if already_complete {
        existing_len
    } else {
        let can_resume = existing_len > 0 && (total_len == 0 || existing_len < total_len);
        let request = if can_resume {
            client
                .get(url)
                .header(reqwest::header::RANGE, format!("bytes={existing_len}-"))
        } else {
            client.get(url)
        };
        let mut response = request.send().await?;
        let mut resume_accepted =
            can_resume && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;

        // Some hosts omit Content-Length on HEAD. If our `.part` already
        // contains the complete object, `Range: bytes=<len>-` answers 416.
        // Verify and adopt it instead of turning every relaunch into the same
        // terminal retry error. If it is not complete/correct, restart with a
        // normal GET rather than appending unrelated bytes.
        if can_resume && response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            let actual = hash_file_with_progress(app, model_id, &part_path, total_len).await?;
            if actual == expected_sha256 {
                std::fs::rename(&part_path, dest)?;
                remove_file_if_present(&identity_path)?;
                publish(app, model_id, existing_len, total_len, "downloaded");
                return Ok(dest.to_path_buf());
            }
            response = client.get(url).send().await?;
            resume_accepted = false;
        }
        let mut response = response.error_for_status()?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resume_accepted)
            .truncate(!resume_accepted)
            .open(&part_path)?;
        let mut downloaded = if resume_accepted { existing_len } else { 0 };
        let mut last_published_bytes = downloaded;
        let mut last_published_at = Instant::now();

        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            let should_publish = downloaded.saturating_sub(last_published_bytes) >= 4 * 1024 * 1024
                || last_published_at.elapsed() >= Duration::from_millis(200)
                || (total_len > 0 && downloaded >= total_len);
            if should_publish {
                publish(app, model_id, downloaded, total_len, "downloading");
                last_published_bytes = downloaded;
                last_published_at = Instant::now();
            }
        }
        downloaded
    };

    let actual_sha256 = hash_file_with_progress(app, model_id, &part_path, total_len).await?;
    if actual_sha256 != expected_sha256 {
        // The partial file is app-owned and known-corrupt. Removing it is
        // what makes the next retry a real retry rather than an infinite
        // checksum-failure loop.
        std::fs::remove_file(&part_path)?;
        remove_file_if_present(&identity_path)?;
        return Err(DownloadError::ChecksumMismatch {
            expected: expected_sha256.to_string(),
            actual: actual_sha256,
        });
    }

    std::fs::rename(&part_path, dest)?;
    remove_file_if_present(&identity_path)?;
    publish(app, model_id, downloaded, total_len, "downloaded");
    Ok(dest.to_path_buf())
}

async fn hash_file_with_progress(
    app: &AppHandle,
    model_id: &str,
    path: &Path,
    bytes_total: u64,
) -> Result<String, DownloadError> {
    publish(app, model_id, 0, bytes_total, "verifying");
    let app = app.clone();
    let model_id = model_id.to_string();
    let path = path.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let mut last_published_bytes = 0u64;
        let mut last_published_at = Instant::now();
        sha256_file_with_progress(&path, |bytes_read| {
            let should_publish = bytes_read.saturating_sub(last_published_bytes)
                >= 16 * 1024 * 1024
                || last_published_at.elapsed() >= Duration::from_millis(200)
                || (bytes_total > 0 && bytes_read >= bytes_total);
            if should_publish {
                publish(&app, &model_id, bytes_read, bytes_total, "verifying");
                last_published_bytes = bytes_read;
                last_published_at = Instant::now();
            }
        })
    })
    .await
    .map_err(|error| DownloadError::Io(std::io::Error::other(error.to_string())))?
    .map_err(DownloadError::Io)
}

fn sha256_file_with_progress(
    path: &Path,
    mut on_progress: impl FnMut(u64),
) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        total += count as u64;
        on_progress(total);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
