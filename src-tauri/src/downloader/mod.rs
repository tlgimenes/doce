use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

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

/// Resumable, checksum-verified download (FR-003): HTTP range requests
/// against a `.part` file, verified against `expected_sha256` before being
/// renamed into place (research.md §5).
pub async fn download_resumable(
    app: &AppHandle,
    model_id: &str,
    url: &str,
    dest: &Path,
    expected_sha256: &str,
) -> Result<PathBuf, DownloadError> {
    let part_path = dest.with_extension("part");
    let client = reqwest::Client::new();

    let mut existing_len = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);

    let head = client.head(url).send().await?;
    let total_len = head
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    // A `.part` already at (or past — shouldn't happen, but be defensive)
    // the expected length is a completed download, not a stale one: go
    // straight to verification instead of discarding it and re-downloading
    // the whole file from scratch.
    let already_complete = total_len > 0 && existing_len >= total_len;
    if existing_len > total_len && total_len > 0 {
        existing_len = 0; // genuinely corrupt (bigger than expected): restart cleanly
    }

    let downloaded = if already_complete {
        existing_len
    } else {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part_path)?;
        file.seek(SeekFrom::Start(existing_len))?;

        let request = if existing_len > 0 {
            client
                .get(url)
                .header(reqwest::header::RANGE, format!("bytes={}-", existing_len))
        } else {
            client.get(url)
        };

        let mut response = request.send().await?;
        let mut downloaded = existing_len;

        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            let _ = app.emit(
                "model-install-progress",
                ModelInstallProgress {
                    model_id: model_id.to_string(),
                    bytes_downloaded: downloaded,
                    bytes_total: total_len,
                    state: "downloading".into(),
                },
            );
        }
        downloaded
    };

    // Verification of a multi-GB file is slow enough on a memory-constrained
    // machine that a flat "Verifying…" spinner is a real regression from the
    // download progress bar the user was just watching — emit real
    // byte-level progress the same way the download loop does.
    let app_for_hash = app.clone();
    let model_id_for_hash = model_id.to_string();
    let part_path_for_hash = part_path.clone();
    let actual_sha256 = tauri::async_runtime::spawn_blocking(move || {
        sha256_file_with_progress(&part_path_for_hash, |bytes_read| {
            let _ = app_for_hash.emit(
                "model-install-progress",
                ModelInstallProgress {
                    model_id: model_id_for_hash.clone(),
                    bytes_downloaded: bytes_read,
                    bytes_total: total_len,
                    state: "verifying".into(),
                },
            );
        })
    })
    .await
    .map_err(|e| DownloadError::Io(std::io::Error::other(e.to_string())))??;
    if actual_sha256 != expected_sha256 {
        return Err(DownloadError::ChecksumMismatch {
            expected: expected_sha256.to_string(),
            actual: actual_sha256,
        });
    }

    std::fs::rename(&part_path, dest)?;
    let _ = app.emit(
        "model-install-progress",
        ModelInstallProgress {
            model_id: model_id.to_string(),
            bytes_downloaded: downloaded,
            bytes_total: total_len,
            state: "installed".into(),
        },
    );

    Ok(dest.to_path_buf())
}

fn sha256_file_with_progress(
    path: &Path,
    mut on_progress: impl FnMut(u64),
) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
        on_progress(total);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
