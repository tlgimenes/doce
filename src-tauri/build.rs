use std::fs;
use std::path::Path;

/// The two build-time vars that bake a built-in Google OAuth client into the
/// binary. For a desktop app the `client_secret` is a public identifier (PKCE
/// protects the flow), so embedding it is safe — see `.env.example`.
const VARS: [&str; 2] = ["DOCE_GOOGLE_CLIENT_ID", "DOCE_GOOGLE_CLIENT_SECRET"];

fn main() {
    inject_builtin_google_client();
    tauri_build::build()
}

/// For each of [`VARS`]: prefer the value already in the process env (CI sets
/// these as build env vars); otherwise fall back to a gitignored `.env` at the
/// repo root. When a value is found, forward it to the crate as a compile-time
/// env var so `option_env!` can read it. If neither source provides a var, we
/// emit nothing and the app builds fine with no built-in client.
fn inject_builtin_google_client() {
    // The .env lives one level up from CARGO_MANIFEST_DIR (`src-tauri/`).
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let dotenv_path = Path::new(&manifest_dir).join("../.env");
    let dotenv = load_dotenv(&dotenv_path);

    // Rebuild when the .env or either override changes.
    println!("cargo:rerun-if-changed=../.env");
    for var in VARS {
        println!("cargo:rerun-if-env-changed={var}");
    }

    for var in VARS {
        let value = std::env::var(var)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| dotenv_lookup(&dotenv, var));
        if let Some(value) = value {
            println!("cargo:rustc-env={var}={value}");
        }
    }
}

/// Reads the `.env` file (if present) into `(key, value)` pairs with a tiny
/// `KEY=VALUE` parse: blank lines and `#` comments are skipped, surrounding
/// quotes on the value are stripped, and everything after the first `=` is the
/// value. A missing file is not an error.
fn load_dotenv(path: &Path) -> Vec<(String, String)> {
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

/// Looks up `var` in the parsed `.env` pairs, ignoring empty values.
fn dotenv_lookup(dotenv: &[(String, String)], var: &str) -> Option<String> {
    dotenv
        .iter()
        .find(|(k, _)| k == var)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.trim().is_empty())
}
