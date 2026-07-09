//! User Story 4 (FR-006/FR-007) — `read_attached_file`: reads local file
//! bytes for both rich-input `attachment` segments (`data-model.md`) and
//! chat-side Read previews. The picker/drag-drop flow yields absolute paths
//! from user-selected files, and Read previews pass through paths already
//! returned by the agent's Read tool. No plugin-fs dependency is installed in
//! this project (research.md's "Native image picker" decision), so this
//! narrowly-scoped command reads the bytes instead.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachedFile {
    pub data: String,
    pub mime_type: String,
    pub name: String,
}

/// Reads the file at `path` and returns its bytes base64-encoded (no
/// `data:` prefix), its MIME type (detected from the extension, see
/// `detect_mime_type`), and its basename. Callers use this for rich-input
/// attachments and for native previews of successful Read tool results.
/// `path` is trusted the same way the existing `Read` agent tool already is
/// (plan.md's Constitution Check): it originates either from a user-selected
/// local attachment path or from a path already present in a successful Read
/// result.
#[tauri::command]
#[specta::specta]
pub fn read_attached_file(path: String) -> Result<AttachedFile, String> {
    let file_path = Path::new(&path);
    if file_path.is_dir() {
        return Err(format!("{path} is a directory, not a file"));
    }

    let bytes = std::fs::read(file_path).map_err(|e| format!("failed to read {path}: {e}"))?;
    let name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());

    Ok(AttachedFile {
        data: STANDARD.encode(bytes),
        mime_type: detect_mime_type(file_path),
        name,
    })
}

/// Simple extension-based MIME sniffing — no dedicated MIME-detection crate
/// for this narrow need. Covers image/video/audio extensions used by
/// attachments and Read previews, plus a reasonable fallback for anything
/// else.
fn detect_mime_type(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase());

    match extension.as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ogg") => "video/ogg",
        Some("mov") => "video/quicktime",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("m4a") => "audio/mp4",
        Some("flac") => "audio/flac",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn reads_a_real_file_as_base64_with_detected_mime_and_basename() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("photo.png");
        let bytes: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 255, 0,
        ];
        fs::write(&file_path, &bytes).unwrap();

        let result = read_attached_file(file_path.to_string_lossy().to_string()).unwrap();

        assert_eq!(result.name, "photo.png");
        assert_eq!(result.mime_type, "image/png");
        let decoded = STANDARD.decode(&result.data).unwrap();
        assert_eq!(decoded, bytes);
    }

    #[test]
    fn detects_native_preview_mime_types_by_extension() {
        let cases = [
            ("diagram.svg", "image/svg+xml"),
            ("clip.mp4", "video/mp4"),
            ("clip.webm", "video/webm"),
            ("clip.ogg", "video/ogg"),
            ("clip.mov", "video/quicktime"),
            ("sound.mp3", "audio/mpeg"),
            ("sound.wav", "audio/wav"),
            ("sound.m4a", "audio/mp4"),
            ("sound.flac", "audio/flac"),
        ];

        for (name, expected_mime) in cases {
            let dir = tempdir().unwrap();
            let file_path = dir.path().join(name);
            fs::write(&file_path, b"preview bytes").unwrap();

            let result = read_attached_file(file_path.to_string_lossy().to_string()).unwrap();

            assert_eq!(result.mime_type, expected_mime, "wrong MIME for {name}");
        }
    }

    #[test]
    fn falls_back_to_octet_stream_for_an_unrecognized_extension() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("notes.txt");
        fs::write(&file_path, b"hello world").unwrap();

        let result = read_attached_file(file_path.to_string_lossy().to_string()).unwrap();

        assert_eq!(result.mime_type, "application/octet-stream");
        let decoded = STANDARD.decode(&result.data).unwrap();
        assert_eq!(decoded, b"hello world");
    }

    #[test]
    fn errors_for_a_path_that_does_not_exist() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist.png");

        let result = read_attached_file(missing.to_string_lossy().to_string());

        assert!(result.is_err());
    }

    #[test]
    fn errors_for_a_path_that_is_a_directory_not_a_file() {
        let dir = tempdir().unwrap();

        let result = read_attached_file(dir.path().to_string_lossy().to_string());

        assert!(result.is_err());
    }
}
