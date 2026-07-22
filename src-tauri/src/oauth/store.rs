//! Secure token storage + the refresh-on-read trigger.
//!
//! Tokens (access + refresh) live in the OS secret store (macOS Keychain
//! via [`KeyringStore`]) — NEVER in SQLite or in `mcp_server_connections.config`.
//! Only non-secret metadata (provider, client_id, scopes, expires_at) is kept
//! in SQLite for listing accounts.
//!
//! The secret store is abstracted behind [`SecretStore`] so unit tests use the
//! in-memory [`InMemoryStore`] and NEVER touch the real Keychain — critical on
//! headless CI runners where the login Keychain is unavailable and a real
//! Keychain probe would hang.

use super::{now_ms, refresh, OAuthError, OAuthProviderConfig, TokenSet};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// How close to expiry (in ms) [`OAuthTokenStore::get_valid_access_token`]
/// proactively refreshes, so a token isn't handed out only to expire in
/// flight. 60s of clock-skew slack.
const REFRESH_SKEW_MS: i64 = 60_000;

/// A pluggable secret backend. Keyed by an opaque account id; the stored
/// value is the JSON-serialized [`StoredCredential`] blob.
pub trait SecretStore: Send + Sync {
    fn get(&self, account_id: &str) -> Result<Option<String>, OAuthError>;
    fn set(&self, account_id: &str, secret: &str) -> Result<(), OAuthError>;
    fn delete(&self, account_id: &str) -> Result<(), OAuthError>;
}

/// Real macOS Keychain backing, via the `keyring` crate (`apple-native`).
/// Constructed only at runtime (see `lib.rs`) — no test may build one, or CI
/// hangs on the login Keychain. Entry construction is lazy: `Entry::new` does
/// NOT touch the Keychain, so merely holding a `KeyringStore` is inert.
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account_id: &str) -> Result<Option<String>, OAuthError> {
        let entry = keyring::Entry::new(&self.service, account_id)
            .map_err(|e| OAuthError::Keyring(e.to_string()))?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(OAuthError::Keyring(e.to_string())),
        }
    }

    fn set(&self, account_id: &str, secret: &str) -> Result<(), OAuthError> {
        let entry = keyring::Entry::new(&self.service, account_id)
            .map_err(|e| OAuthError::Keyring(e.to_string()))?;
        entry
            .set_password(secret)
            .map_err(|e| OAuthError::Keyring(e.to_string()))
    }

    fn delete(&self, account_id: &str) -> Result<(), OAuthError> {
        let entry = keyring::Entry::new(&self.service, account_id)
            .map_err(|e| OAuthError::Keyring(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(OAuthError::Keyring(e.to_string())),
        }
    }
}

/// In-memory secret backing for tests. Round-trips like the real store but
/// touches nothing outside the process.
#[derive(Default)]
pub struct InMemoryStore {
    map: Mutex<HashMap<String, String>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for InMemoryStore {
    fn get(&self, account_id: &str) -> Result<Option<String>, OAuthError> {
        Ok(self.map.lock().unwrap().get(account_id).cloned())
    }

    fn set(&self, account_id: &str, secret: &str) -> Result<(), OAuthError> {
        self.map
            .lock()
            .unwrap()
            .insert(account_id.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, account_id: &str) -> Result<(), OAuthError> {
        self.map.lock().unwrap().remove(account_id);
        Ok(())
    }
}

/// The full secret blob persisted per account: the provider config (incl. the
/// desktop `client_secret`, which Google states "is not treated as
/// confidential" for installed apps) plus the current token set. Kept
/// self-contained so [`OAuthTokenStore::get_valid_access_token`] can refresh
/// WITHOUT reaching back into SQLite for endpoint/credential metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCredential {
    pub config: OAuthProviderConfig,
    pub tokens: TokenSet,
}

/// Owns a [`SecretStore`] and the refresh machinery (an HTTP client + a clock).
/// The clock is injectable so the refresh-decision boundary is testable
/// without sleeping or hitting the network.
pub struct OAuthTokenStore {
    secrets: Arc<dyn SecretStore>,
    http: reqwest::Client,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
}

impl OAuthTokenStore {
    /// Runtime constructor: real wall clock, a fresh reqwest client.
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            secrets,
            http: reqwest::Client::new(),
            clock: Box::new(now_ms),
        }
    }

    /// Test constructor: inject a clock (and reuse a client). Lets a test pin
    /// "now" to drive the expiry boundary deterministically.
    pub fn with_clock(
        secrets: Arc<dyn SecretStore>,
        http: reqwest::Client,
        clock: impl Fn() -> i64 + Send + Sync + 'static,
    ) -> Self {
        Self {
            secrets,
            http,
            clock: Box::new(clock),
        }
    }

    /// Persist (or overwrite) the credential for `account_id`.
    pub fn put_credential(
        &self,
        account_id: &str,
        config: OAuthProviderConfig,
        tokens: TokenSet,
    ) -> Result<(), OAuthError> {
        let blob = serde_json::to_string(&StoredCredential { config, tokens })?;
        self.secrets.set(account_id, &blob)
    }

    /// Read the stored credential, if any.
    pub fn get_credential(&self, account_id: &str) -> Result<Option<StoredCredential>, OAuthError> {
        match self.secrets.get(account_id)? {
            Some(blob) => Ok(Some(serde_json::from_str(&blob)?)),
            None => Ok(None),
        }
    }

    /// Forget an account's tokens (Keychain entry). Idempotent.
    pub fn delete_credential(&self, account_id: &str) -> Result<(), OAuthError> {
        self.secrets.delete(account_id)
    }

    /// The refresh trigger. Reads the stored token set and, if it is within
    /// [`REFRESH_SKEW_MS`] of expiry, refreshes at the token endpoint,
    /// persists the rotated tokens (preserving the prior refresh token when
    /// the response omits a new one — Google often does), and returns a valid
    /// access token. Otherwise returns the stored access token untouched (no
    /// network).
    pub async fn get_valid_access_token(&self, account_id: &str) -> Result<String, OAuthError> {
        let cred = self
            .get_credential(account_id)?
            .ok_or(OAuthError::AccountNotFound)?;
        let now = (self.clock)();

        if now < cred.tokens.expires_at - REFRESH_SKEW_MS {
            return Ok(cred.tokens.access_token);
        }

        let refresh_token = cred
            .tokens
            .refresh_token
            .clone()
            .ok_or(OAuthError::NoRefreshToken)?;
        let refreshed = refresh(&self.http, &cred.config, &refresh_token, now).await?;

        // A refresh response may omit `refresh_token` — keep the existing one.
        let merged = TokenSet {
            refresh_token: refreshed.refresh_token.or(cred.tokens.refresh_token),
            ..refreshed
        };
        self.put_credential(account_id, cred.config, merged.clone())?;
        Ok(merged.access_token)
    }

    /// Force a refresh regardless of the expiry window — used by the live-401
    /// retry path at the MCP call sites (a server may reject a token the store
    /// still believes is valid). Persists and returns the fresh access token.
    pub async fn force_refresh(&self, account_id: &str) -> Result<String, OAuthError> {
        let cred = self
            .get_credential(account_id)?
            .ok_or(OAuthError::AccountNotFound)?;
        let refresh_token = cred
            .tokens
            .refresh_token
            .clone()
            .ok_or(OAuthError::NoRefreshToken)?;
        let now = (self.clock)();
        let refreshed = refresh(&self.http, &cred.config, &refresh_token, now).await?;
        let merged = TokenSet {
            refresh_token: refreshed.refresh_token.or(cred.tokens.refresh_token),
            ..refreshed
        };
        self.put_credential(account_id, cred.config, merged.clone())?;
        Ok(merged.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::google;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(token_uri: String) -> OAuthProviderConfig {
        OAuthProviderConfig {
            client_id: "cid".to_string(),
            client_secret: Some("csecret".to_string()),
            auth_uri: google::AUTH_URI.to_string(),
            token_uri,
            scopes: vec!["scope.a".to_string()],
        }
    }

    fn token_set(access: &str, refresh: Option<&str>, expires_at: i64) -> TokenSet {
        TokenSet {
            access_token: access.to_string(),
            refresh_token: refresh.map(str::to_string),
            expires_at,
            scopes: vec!["scope.a".to_string()],
        }
    }

    #[test]
    fn in_memory_secret_store_round_trips() {
        let store = InMemoryStore::new();
        assert_eq!(store.get("acct").unwrap(), None);
        store.set("acct", "blob-value").unwrap();
        assert_eq!(store.get("acct").unwrap().as_deref(), Some("blob-value"));
        store.delete("acct").unwrap();
        assert_eq!(store.get("acct").unwrap(), None);
        // delete is idempotent
        store.delete("acct").unwrap();
    }

    #[tokio::test]
    async fn does_not_refresh_when_token_is_still_fresh() {
        // Token expires 2 minutes out; clock is "now". Well outside the 60s
        // skew window -> no refresh, no network. Point token_uri at an
        // unroutable address to PROVE no request is made.
        let now = 1_000_000_000_000;
        let store = OAuthTokenStore::with_clock(
            Arc::new(InMemoryStore::new()),
            reqwest::Client::new(),
            move || now,
        );
        let mut cfg = test_config("http://127.0.0.1:1/never-called".to_string());
        cfg.token_uri = "http://127.0.0.1:1/never-called".to_string();
        store
            .put_credential(
                "acct",
                cfg,
                token_set("stored-access", Some("rt"), now + 120_000),
            )
            .unwrap();

        let token = store.get_valid_access_token("acct").await.unwrap();
        assert_eq!(token, "stored-access", "fresh token returned as-is");
    }

    #[tokio::test]
    async fn refreshes_when_within_skew_window() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fresh-access",
                "expires_in": 3600,
                "scope": "scope.a"
            })))
            .mount(&server)
            .await;

        let now = 1_000_000_000_000;
        let store = OAuthTokenStore::with_clock(
            Arc::new(InMemoryStore::new()),
            reqwest::Client::new(),
            move || now,
        );
        // Expires in 30s -> inside the 60s skew window -> must refresh.
        store
            .put_credential(
                "acct",
                test_config(format!("{}/token", server.uri())),
                token_set("stale-access", Some("the-refresh-token"), now + 30_000),
            )
            .unwrap();

        let token = store.get_valid_access_token("acct").await.unwrap();
        assert_eq!(token, "fresh-access", "refreshed access token returned");

        // The rotated token set was persisted with the preserved refresh token
        // and a recomputed expiry ~1h out.
        let stored = store.get_credential("acct").unwrap().unwrap();
        assert_eq!(stored.tokens.access_token, "fresh-access");
        assert_eq!(
            stored.tokens.refresh_token.as_deref(),
            Some("the-refresh-token"),
            "refresh token preserved when the response omits a new one"
        );
        assert_eq!(stored.tokens.expires_at, now + 3600 * 1000);
    }

    #[tokio::test]
    async fn missing_account_is_an_error_not_a_panic() {
        let store = OAuthTokenStore::new(Arc::new(InMemoryStore::new()));
        let err = store.get_valid_access_token("nope").await.unwrap_err();
        assert!(matches!(err, OAuthError::AccountNotFound));
    }
}
