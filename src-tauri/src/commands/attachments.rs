//! User Story 4 (FR-006/FR-007) — `read_attached_file`: reads a
//! user-selected local file's bytes for the `attachment` segment
//! (`data-model.md`). The picker/drag-drop flow on the frontend only ever
//! yields an absolute path (`@tauri-apps/plugin-dialog`'s `open()`, or a
//! dropped/pasted `File`'s path) — no plugin-fs dependency is installed in
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

/// contracts/rich-chat-input.md's `read_attached_file`: reads the file at
/// `path` and returns its bytes base64-encoded (no `data:` prefix — the
/// `attachment` segment's `data` field is raw base64, matching
/// data-model.md), its MIME type (detected from the extension, see
/// `detect_mime_type`), and its basename. `path` is trusted the same way
/// the existing `Read` agent tool already is (plan.md's Constitution
/// Check) — it only ever originates from a path the user explicitly
/// picked or dropped, never an arbitrary agent-supplied value.
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

/// Simple extension-based MIME sniffing — no dedicated MIME-detection
/// crate for this narrow need (task instructions). Covers the image
/// extensions the file picker filters to (research.md's `Image` filter:
/// png/jpg/jpeg/gif/webp) plus a reasonable fallback for anything else
/// (e.g. a non-image attachment, whose `mimeType` is stored but not
/// otherwise interpreted — FR-008 only distinguishes image vs. non-image
/// via the segment's separate `isImage` flag).
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
