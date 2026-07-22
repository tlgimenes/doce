//! Provider-agnostic desktop OAuth 2.0 + PKCE (S256, loopback redirect)
//! engine — the real replacement for the static `auth_token` stub, so an MCP
//! server can be linked to an OAuth account and get a fresh bearer token on
//! each connect.
//!
//! The flow is deliberately split into pure, testable pieces:
//!   * [`begin_flow`] — generate PKCE verifier + S256 challenge, a CSRF
//!     `state`, bind a loopback listener on `127.0.0.1:0`, build the
//!     authorize URL, and open the system browser. Returns a [`PendingFlow`].
//!   * [`await_callback`] — accept ONE loopback connection, validate the CSRF
//!     `state`, and exchange the `code` + `code_verifier` at the token
//!     endpoint for a [`TokenSet`].
//!   * [`refresh`] — trade a refresh token for a new access token.
//!
//! Nothing here is Google-specific: [`OAuthProviderConfig`] carries the
//! endpoints + credentials, and the [`google`] preset only fills in Google's
//! well-known auth/token URLs and scope constants. NO client_id/secret is
//! hardcoded — the user supplies those (registering a Google Cloud OAuth
//! client is human-gated and out of scope).
//!
//! `oauth2` is used ONLY for its pure PKCE/CSRF primitives; all HTTP is driven
//! through doce's existing reqwest client (see `Cargo.toml`), so there is no
//! second reqwest major in the tree.

pub mod google_workspace;
pub mod store;

pub use store::{InMemoryStore, KeyringStore, OAuthTokenStore, SecretStore, StoredCredential};

use crate::mcp::McpTransportConfig;
use oauth2::{CsrfToken, PkceCodeChallenge};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Unix-epoch milliseconds — the timestamp unit used everywhere in doce.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("http error: {0}")]
    Http(String),
    #[error("token endpoint returned an error: {0}")]
    Token(String),
    #[error("CSRF state mismatch — authorization response rejected")]
    StateMismatch,
    #[error("malformed loopback callback request")]
    MalformedCallback,
    #[error("keyring error: {0}")]
    Keyring(String),
    #[error("no OAuth account with that id")]
    AccountNotFound,
    #[error("stored account has no refresh token")]
    NoRefreshToken,
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
    #[error(
        "No Google client configured — enter one, or build doce with DOCE_GOOGLE_CLIENT_ID/SECRET"
    )]
    NoClientConfigured,
}

impl From<reqwest::Error> for OAuthError {
    fn from(e: reqwest::Error) -> Self {
        OAuthError::Http(e.to_string())
    }
}

/// A provider's OAuth endpoints + client credentials. Provider-agnostic: the
/// [`google`] preset fills the endpoints/scopes but leaves credentials as
/// user-supplied params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthProviderConfig {
    pub client_id: String,
    /// Optional for PKCE/desktop clients. Google issues a `client_secret` for
    /// installed apps that "is not treated as confidential".
    pub client_secret: Option<String>,
    pub auth_uri: String,
    pub token_uri: String,
    pub scopes: Vec<String>,
}

/// A set of OAuth tokens plus the derived absolute expiry. `expires_at` is
/// Unix-epoch **ms** (computed from the response's `expires_in` seconds), so
/// the store can compare it against [`now_ms`] directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: i64,
    pub scopes: Vec<String>,
}

/// A begun-but-not-yet-completed authorization: the state [`await_callback`]
/// needs to finish the exchange. Holds the loopback listener open so the
/// OS-assigned port stays bound between opening the browser and the redirect.
pub struct PendingFlow {
    pub config: OAuthProviderConfig,
    /// The PKCE `code_verifier` (raw secret) to send at the token endpoint.
    verifier: String,
    /// The CSRF `state` this flow expects echoed back.
    state: String,
    /// `http://127.0.0.1:<port>` — must match what was sent to `authorize`.
    redirect_uri: String,
    /// The bound loopback listener; [`await_callback`] accepts one connection.
    listener: TcpListener,
    /// The fully-built authorize URL (also opened in the browser).
    pub authorize_url: String,
}

/// The token-endpoint JSON response shape (Google / RFC 6749). All fields but
/// `access_token` are optional across grant types / providers.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    /// Lifetime in **seconds**.
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

/// Default access-token lifetime (seconds) when the endpoint omits
/// `expires_in`. Google always sends it (~1h); this is a safety net.
const DEFAULT_EXPIRES_IN_SECS: i64 = 3600;

/// Well-known Google OAuth endpoints + scope constants + a preset builder.
/// Credentials are NOT baked in — `config` takes the user-supplied
/// `client_id`/`client_secret`.
pub mod google {
    use super::OAuthProviderConfig;

    pub const PROVIDER: &str = "google";
    pub const AUTH_URI: &str = "https://accounts.google.com/o/oauth2/v2/auth";
    pub const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

    // Minimal, least-privilege scopes (see the implementation map):
    // Gmail read + compose-draft (NOT modify/send), Calendar read + create,
    // Drive per-file (non-restricted).
    pub const SCOPE_GMAIL_READONLY: &str = "https://www.googleapis.com/auth/gmail.readonly";
    pub const SCOPE_GMAIL_COMPOSE: &str = "https://www.googleapis.com/auth/gmail.compose";
    pub const SCOPE_CALENDAR_EVENTS_READONLY: &str =
        "https://www.googleapis.com/auth/calendar.events.readonly";
    pub const SCOPE_CALENDAR_EVENTS: &str = "https://www.googleapis.com/auth/calendar.events";
    pub const SCOPE_DRIVE_FILE: &str = "https://www.googleapis.com/auth/drive.file";

    /// The full default scope set doce requests for a Google account.
    pub fn default_scopes() -> Vec<String> {
        [
            SCOPE_GMAIL_READONLY,
            SCOPE_GMAIL_COMPOSE,
            SCOPE_CALENDAR_EVENTS_READONLY,
            SCOPE_CALENDAR_EVENTS,
            SCOPE_DRIVE_FILE,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    /// The built-in Google OAuth client baked into the binary at build time,
    /// if any. Returns `Some((client_id, client_secret))` ONLY when BOTH
    /// `DOCE_GOOGLE_CLIENT_ID` and `DOCE_GOOGLE_CLIENT_SECRET` were injected
    /// (via `build.rs`) and are non-empty; otherwise `None`. For a desktop app
    /// the `client_secret` is a non-confidential public identifier (PKCE is
    /// what protects the flow), so shipping it in the binary is expected — see
    /// `.env.example`. `option_env!` resolves at compile time, so a build
    /// without the vars simply has no built-in client.
    pub fn builtin_client() -> Option<(String, String)> {
        match (
            option_env!("DOCE_GOOGLE_CLIENT_ID"),
            option_env!("DOCE_GOOGLE_CLIENT_SECRET"),
        ) {
            (Some(id), Some(secret)) if !id.trim().is_empty() && !secret.trim().is_empty() => {
                Some((id.to_string(), secret.to_string()))
            }
            _ => None,
        }
    }

    /// Builds a Google [`OAuthProviderConfig`] from user-supplied credentials.
    /// `scopes` empty -> [`default_scopes`].
    pub fn config(
        client_id: String,
        client_secret: Option<String>,
        scopes: Vec<String>,
    ) -> OAuthProviderConfig {
        OAuthProviderConfig {
            client_id,
            client_secret,
            auth_uri: AUTH_URI.to_string(),
            token_uri: TOKEN_URI.to_string(),
            scopes: if scopes.is_empty() {
                default_scopes()
            } else {
                scopes
            },
        }
    }
}

/// Resolves a `provider` name + user-supplied credentials into an
/// [`OAuthProviderConfig`]. Only `google` is presently known; the engine
/// itself is provider-agnostic, so adding a preset is the only change needed
/// for another provider.
pub fn provider_config(
    provider: &str,
    client_id: String,
    client_secret: Option<String>,
    scopes: Vec<String>,
) -> Result<OAuthProviderConfig, OAuthError> {
    match provider {
        google::PROVIDER => Ok(google::config(client_id, client_secret, scopes)),
        other => Err(OAuthError::UnsupportedProvider(other.to_string())),
    }
}

/// Decides which client credentials a connect should use, given what the caller
/// passed and the (optionally present) built-in client. Pure so the fallback
/// policy is unit-testable without `option_env!`:
///   * a non-blank `passed_id` wins — BYO behavior, passing its own secret
///     through untouched;
///   * a blank/empty `passed_id` with a `builtin` present resolves to the
///     built-in id + secret;
///   * a blank/empty `passed_id` and NO built-in is an error.
pub fn resolve_client_credentials(
    passed_id: &str,
    passed_secret: Option<String>,
    builtin: Option<(String, String)>,
) -> Result<(String, Option<String>), OAuthError> {
    if !passed_id.trim().is_empty() {
        return Ok((passed_id.to_string(), passed_secret));
    }
    match builtin {
        Some((id, secret)) => Ok((id, Some(secret))),
        None => Err(OAuthError::NoClientConfigured),
    }
}

/// Builds the authorize URL for the auth-code + PKCE + loopback flow. Pure and
/// testable: given the config, the S256 `code_challenge`, the CSRF `state`, and
/// the loopback `redirect_uri`, it assembles every required query param
/// (`response_type=code`, `scope`, `code_challenge_method=S256`,
/// `access_type=offline`, `prompt=consent`, …).
fn build_authorize_url(
    config: &OAuthProviderConfig,
    code_challenge: &str,
    state: &str,
    redirect_uri: &str,
) -> String {
    let scope = config.scopes.join(" ");
    let mut url = reqwest::Url::parse(&config.auth_uri)
        // The auth_uri comes from a trusted preset; if it somehow doesn't
        // parse, degrade to the raw string rather than panicking.
        .unwrap_or_else(|_| reqwest::Url::parse("https://invalid.example/").unwrap());
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", &scope)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    url.to_string()
}

/// Begins an authorization: PKCE (S256) + CSRF, bind a loopback listener on an
/// OS-assigned port, build the authorize URL, and open the system browser
/// (best-effort — a failure to open is not fatal; the URL is also returned on
/// the [`PendingFlow`] for a manual fallback).
pub async fn begin_flow(config: OAuthProviderConfig) -> Result<PendingFlow, OAuthError> {
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let state = CsrfToken::new_random().secret().clone();

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");

    let authorize_url = build_authorize_url(&config, challenge.as_str(), &state, &redirect_uri);
    // Best-effort: don't fail the flow if no browser can be launched.
    let _ = open::that(&authorize_url);

    Ok(PendingFlow {
        config,
        verifier: verifier.secret().clone(),
        state,
        redirect_uri,
        listener,
        authorize_url,
    })
}

/// Parses the loopback redirect's HTTP request line and returns the
/// authorization `code`, having FIRST validated the CSRF `state` matches
/// `expected_state`. Pure over the raw request bytes so the CSRF-rejection
/// path is unit-testable without a socket.
fn extract_authorization_code(request: &str, expected_state: &str) -> Result<String, OAuthError> {
    // Request line: `GET /?code=...&state=... HTTP/1.1`
    let target = request
        .split_whitespace()
        .nth(1)
        .ok_or(OAuthError::MalformedCallback)?;
    // Parse against a dummy base so `?code&state` becomes query pairs.
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| OAuthError::MalformedCallback)?;

    let mut code = None;
    let mut state = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => return Err(OAuthError::Token(v.into_owned())),
            _ => {}
        }
    }

    let state = state.ok_or(OAuthError::MalformedCallback)?;
    if state != expected_state {
        return Err(OAuthError::StateMismatch);
    }
    code.ok_or(OAuthError::MalformedCallback)
}

/// Accepts ONE connection on the loopback listener, validates the CSRF state,
/// writes a friendly close-the-tab page, then exchanges the code for tokens.
pub async fn await_callback(flow: PendingFlow) -> Result<TokenSet, OAuthError> {
    let (mut stream, _) = flow.listener.accept().await?;

    // The request line + headers fit comfortably in one read for a GET.
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let result = extract_authorization_code(&request, &flow.state);

    // Always answer the browser so the tab doesn't hang, tailoring the body to
    // success/failure.
    let body = match &result {
        Ok(_) => "<!doctype html><meta charset=\"utf-8\"><title>doce</title><body style=\"font-family:system-ui;padding:2rem\"><h2>You're connected.</h2><p>You can close this tab and return to doce.</p></body>",
        Err(_) => "<!doctype html><meta charset=\"utf-8\"><title>doce</title><body style=\"font-family:system-ui;padding:2rem\"><h2>Authorization failed.</h2><p>You can close this tab and return to doce.</p></body>",
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;

    let code = result?;
    let client = reqwest::Client::new();
    exchange_code(
        &client,
        &flow.config,
        &code,
        &flow.verifier,
        &flow.redirect_uri,
        now_ms(),
    )
    .await
}

/// Builds a [`TokenSet`] from a decoded token-endpoint response, computing the
/// absolute `expires_at` (ms) from `now` + `expires_in`. Falls back to the
/// requested scopes when the response omits `scope`. Pure — the expiry math
/// and scope-derivation are unit-testable without the network.
fn token_set_from_response(resp: TokenResponse, requested_scopes: &[String], now: i64) -> TokenSet {
    let expires_in = resp.expires_in.unwrap_or(DEFAULT_EXPIRES_IN_SECS);
    let scopes = match resp.scope {
        Some(s) if !s.trim().is_empty() => s.split_whitespace().map(str::to_string).collect(),
        _ => requested_scopes.to_vec(),
    };
    TokenSet {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token,
        expires_at: now + expires_in * 1000,
        scopes,
    }
}

/// POSTs a form body to the token endpoint and decodes it into a [`TokenSet`].
/// A non-2xx response surfaces the endpoint's error body via [`OAuthError::Token`].
async fn post_token_request(
    client: &reqwest::Client,
    token_uri: &str,
    form: &[(&str, &str)],
    requested_scopes: &[String],
    now: i64,
) -> Result<TokenSet, OAuthError> {
    // Encode `application/x-www-form-urlencoded` explicitly (this reqwest build
    // doesn't expose `RequestBuilder::form`).
    let body: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form.iter().copied())
        .finish();
    let resp = client
        .post(token_uri)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        return Err(OAuthError::Token(format!("HTTP {status}: {text}")));
    }
    let decoded: TokenResponse =
        serde_json::from_str(&text).map_err(|e| OAuthError::Token(e.to_string()))?;
    Ok(token_set_from_response(decoded, requested_scopes, now))
}

/// Exchanges an authorization `code` + PKCE `code_verifier` for tokens.
async fn exchange_code(
    client: &reqwest::Client,
    config: &OAuthProviderConfig,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
    now: i64,
) -> Result<TokenSet, OAuthError> {
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", &config.client_id),
        ("code_verifier", code_verifier),
    ];
    if let Some(secret) = &config.client_secret {
        form.push(("client_secret", secret));
    }
    post_token_request(client, &config.token_uri, &form, &config.scopes, now).await
}

/// Trades a refresh token for a fresh access token. `now` is injected so the
/// derived `expires_at` is testable.
pub async fn refresh(
    client: &reqwest::Client,
    config: &OAuthProviderConfig,
    refresh_token: &str,
    now: i64,
) -> Result<TokenSet, OAuthError> {
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", &config.client_id),
    ];
    if let Some(secret) = &config.client_secret {
        form.push(("client_secret", secret));
    }
    post_token_request(client, &config.token_uri, &form, &config.scopes, now).await
}

/// Resolves an MCP transport config into a concrete, connectable one JUST
/// before connecting: an oauth-linked HTTP config
/// (`Http { oauth_account_id: Some(_), .. }`) has its bearer token resolved
/// (refreshing if needed) into `Http { auth_token: Some(fresh), .. }`. Every
/// other config (static HTTP, stdio) passes through unchanged. This is the one
/// bridge between the token store and the transport-agnostic `mcp` module —
/// `mcp::connect` never learns about the store.
pub async fn resolve_http_config(
    config: &McpTransportConfig,
    store: &OAuthTokenStore,
) -> Result<McpTransportConfig, OAuthError> {
    match config {
        McpTransportConfig::Http {
            url,
            oauth_account_id: Some(account_id),
            ..
        } => {
            let token = store.get_valid_access_token(account_id).await?;
            Ok(McpTransportConfig::Http {
                url: url.clone(),
                auth_token: Some(token),
                oauth_account_id: None,
            })
        }
        other => Ok(other.clone()),
    }
}

/// The OAuth account id an HTTP config is linked to, if any.
pub fn linked_account_id(config: &McpTransportConfig) -> Option<&str> {
    match config {
        McpTransportConfig::Http {
            oauth_account_id: Some(id),
            ..
        } => Some(id.as_str()),
        _ => None,
    }
}

/// Heuristic: does this MCP error look like an auth rejection (401/403)?
/// rmcp surfaces transport failures as an opaque `Client(String)`, so we
/// pattern-match the message.
///
/// TODO: once rmcp exposes a typed HTTP status on the transport error, switch
/// to that and emit a proper `McpAuthRequired` event to drive a re-consent UI
/// (a later phase). For now this only gates the best-effort refresh-and-retry.
pub fn is_auth_error(err: &crate::mcp::McpError) -> bool {
    match err {
        crate::mcp::McpError::Client(msg) => {
            let lower = msg.to_lowercase();
            msg.contains("401")
                || msg.contains("403")
                || lower.contains("unauthorized")
                || lower.contains("forbidden")
        }
        _ => false,
    }
}

/// Resolves `config`'s bearer (refreshing if near expiry), runs `op` against
/// the concrete config, and — if the call fails with what looks like an auth
/// rejection AND the config is OAuth-linked — forces ONE token refresh and
/// retries. This is the single place the three MCP call sites (`describe_service`,
/// `call_tool`, the "test connection" `list_tools`) get both refresh-per-connect
/// and live-401 recovery, without teaching `mcp::connect` about the store.
pub async fn resolve_with_retry<F, Fut, T>(
    config: &McpTransportConfig,
    store: &OAuthTokenStore,
    op: F,
) -> Result<T, crate::mcp::McpError>
where
    F: Fn(McpTransportConfig) -> Fut,
    Fut: std::future::Future<Output = Result<T, crate::mcp::McpError>>,
{
    let resolved = resolve_http_config(config, store)
        .await
        .map_err(|e| crate::mcp::McpError::Client(e.to_string()))?;
    let first = op(resolved).await;

    if let Err(ref e) = first {
        if is_auth_error(e) {
            if let Some(account_id) = linked_account_id(config) {
                if store.force_refresh(account_id).await.is_ok() {
                    let resolved = resolve_http_config(config, store)
                        .await
                        .map_err(|e| crate::mcp::McpError::Client(e.to_string()))?;
                    return op(resolved).await;
                }
            }
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use sha2::{Digest, Sha256};

    fn google_config() -> OAuthProviderConfig {
        google::config(
            "client-123.apps.googleusercontent.com".to_string(),
            None,
            vec![],
        )
    }

    // --- PKCE ---------------------------------------------------------

    #[test]
    fn pkce_challenge_is_s256_of_verifier() {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        // S256: challenge == base64url-nopad( SHA256(verifier) ).
        let digest = Sha256::digest(verifier.secret().as_bytes());
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
        assert_eq!(challenge.as_str(), expected);
    }

    #[test]
    fn authorize_url_contains_all_required_params() {
        let config = google_config();
        let url = build_authorize_url(
            &config,
            "the-challenge",
            "the-state",
            "http://127.0.0.1:54321",
        );
        let parsed = reqwest::Url::parse(&url).unwrap();
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(q.get("response_type").map(String::as_str), Some("code"));
        assert_eq!(
            q.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert_eq!(
            q.get("code_challenge").map(String::as_str),
            Some("the-challenge")
        );
        assert_eq!(q.get("state").map(String::as_str), Some("the-state"));
        assert_eq!(q.get("access_type").map(String::as_str), Some("offline"));
        assert_eq!(q.get("prompt").map(String::as_str), Some("consent"));
        assert_eq!(
            q.get("redirect_uri").map(String::as_str),
            Some("http://127.0.0.1:54321")
        );
        // Scopes are space-joined and include the least-privilege Gmail read.
        let scope = q.get("scope").cloned().unwrap_or_default();
        assert!(
            scope.contains(google::SCOPE_GMAIL_READONLY),
            "scope: {scope}"
        );
        // Points at Google's real authorize endpoint.
        assert!(url.starts_with(google::AUTH_URI));
    }

    // --- CSRF state validation ---------------------------------------

    #[test]
    fn callback_with_matching_state_yields_the_code() {
        let req =
            "GET /?code=auth-code-xyz&state=expected-state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let code = extract_authorization_code(req, "expected-state").unwrap();
        assert_eq!(code, "auth-code-xyz");
    }

    #[test]
    fn callback_with_mismatched_state_is_rejected() {
        let req = "GET /?code=auth-code-xyz&state=attacker-state HTTP/1.1\r\n\r\n";
        let err = extract_authorization_code(req, "expected-state").unwrap_err();
        assert!(matches!(err, OAuthError::StateMismatch));
    }

    #[test]
    fn callback_surfacing_provider_error_is_reported() {
        let req = "GET /?error=access_denied&state=expected-state HTTP/1.1\r\n\r\n";
        let err = extract_authorization_code(req, "expected-state").unwrap_err();
        assert!(matches!(err, OAuthError::Token(_)));
    }

    #[test]
    fn callback_missing_state_is_malformed() {
        let req = "GET /?code=abc HTTP/1.1\r\n\r\n";
        let err = extract_authorization_code(req, "expected-state").unwrap_err();
        assert!(matches!(err, OAuthError::MalformedCallback));
    }

    // --- Token response parsing / expiry ------------------------------

    #[test]
    fn token_response_computes_absolute_expiry_in_ms() {
        let resp = TokenResponse {
            access_token: "at".to_string(),
            refresh_token: Some("rt".to_string()),
            expires_in: Some(3600),
            scope: Some("scope.a scope.b".to_string()),
        };
        let now = 1_000_000_000_000;
        let ts = token_set_from_response(resp, &["fallback".to_string()], now);
        assert_eq!(ts.access_token, "at");
        assert_eq!(ts.refresh_token.as_deref(), Some("rt"));
        assert_eq!(ts.expires_at, now + 3600 * 1000);
        assert_eq!(
            ts.scopes,
            vec!["scope.a".to_string(), "scope.b".to_string()]
        );
    }

    #[test]
    fn token_response_falls_back_to_requested_scopes_and_default_lifetime() {
        let resp = TokenResponse {
            access_token: "at".to_string(),
            refresh_token: None,
            expires_in: None,
            scope: None,
        };
        let now = 500;
        let requested = vec!["req.scope".to_string()];
        let ts = token_set_from_response(resp, &requested, now);
        assert_eq!(ts.expires_at, now + DEFAULT_EXPIRES_IN_SECS * 1000);
        assert_eq!(ts.scopes, requested);
    }

    // --- Provider preset ----------------------------------------------

    #[test]
    fn google_preset_uses_known_endpoints_and_empty_credentials_by_default() {
        let cfg = google::config("cid".to_string(), None, vec![]);
        assert_eq!(cfg.auth_uri, google::AUTH_URI);
        assert_eq!(cfg.token_uri, google::TOKEN_URI);
        assert_eq!(cfg.client_id, "cid");
        assert!(cfg.client_secret.is_none(), "no secret is hardcoded");
        assert!(
            !cfg.scopes.is_empty(),
            "defaults to the least-privilege scope set"
        );
    }

    #[test]
    fn unknown_provider_is_rejected() {
        let err = provider_config("myspace", "cid".to_string(), None, vec![]).unwrap_err();
        assert!(matches!(err, OAuthError::UnsupportedProvider(_)));
    }

    // --- built-in client credential resolution ------------------------

    #[test]
    fn resolve_credentials_passed_id_wins_over_builtin() {
        let (id, secret) = resolve_client_credentials(
            "user-cid",
            Some("user-secret".to_string()),
            Some(("builtin-cid".to_string(), "builtin-secret".to_string())),
        )
        .unwrap();
        assert_eq!(id, "user-cid");
        assert_eq!(secret.as_deref(), Some("user-secret"));
    }

    #[test]
    fn resolve_credentials_passed_id_without_secret_is_preserved() {
        let (id, secret) = resolve_client_credentials("user-cid", None, None).unwrap();
        assert_eq!(id, "user-cid");
        assert!(secret.is_none());
    }

    #[test]
    fn resolve_credentials_blank_id_falls_back_to_builtin() {
        // A passed secret is ignored when falling back — the built-in's own
        // secret is used.
        let (id, secret) = resolve_client_credentials(
            "   ",
            Some("ignored".to_string()),
            Some(("builtin-cid".to_string(), "builtin-secret".to_string())),
        )
        .unwrap();
        assert_eq!(id, "builtin-cid");
        assert_eq!(secret.as_deref(), Some("builtin-secret"));
    }

    #[test]
    fn resolve_credentials_blank_id_and_no_builtin_errors() {
        let err = resolve_client_credentials("", None, None).unwrap_err();
        assert!(matches!(err, OAuthError::NoClientConfigured));
    }

    // --- resolve_http_config ------------------------------------------

    #[tokio::test]
    async fn resolve_http_config_injects_fresh_token_for_oauth_linked_config() {
        let store = OAuthTokenStore::with_clock(
            std::sync::Arc::new(InMemoryStore::new()),
            reqwest::Client::new(),
            || 1_000,
        );
        // A far-future expiry means get_valid_access_token returns the stored
        // token with NO network / refresh.
        store
            .put_credential(
                "acct-1",
                google_config(),
                TokenSet {
                    access_token: "stored-bearer".to_string(),
                    refresh_token: Some("rt".to_string()),
                    expires_at: i64::MAX,
                    scopes: vec![],
                },
            )
            .unwrap();

        let linked = McpTransportConfig::Http {
            url: "https://example.com/mcp".to_string(),
            auth_token: None,
            oauth_account_id: Some("acct-1".to_string()),
        };
        let resolved = resolve_http_config(&linked, &store).await.unwrap();
        assert_eq!(
            resolved,
            McpTransportConfig::Http {
                url: "https://example.com/mcp".to_string(),
                auth_token: Some("stored-bearer".to_string()),
                oauth_account_id: None,
            }
        );
    }

    #[tokio::test]
    async fn resolve_http_config_passes_static_and_stdio_through_unchanged() {
        let store = OAuthTokenStore::new(std::sync::Arc::new(InMemoryStore::new()));

        let static_http = McpTransportConfig::Http {
            url: "https://example.com/mcp".to_string(),
            auth_token: Some("static-token".to_string()),
            oauth_account_id: None,
        };
        assert_eq!(
            resolve_http_config(&static_http, &store).await.unwrap(),
            static_http
        );

        let stdio = McpTransportConfig::Stdio {
            command: "node".to_string(),
            args: vec!["s.js".to_string()],
        };
        assert_eq!(resolve_http_config(&stdio, &store).await.unwrap(), stdio);
    }
}
