//! Workshop-owned ChatGPT subscription OAuth.
//!
//! Tokens stay behind this native boundary. The renderer receives only the
//! redacted status DTO, while Codex receives a short-lived `auth.json` in its
//! isolated session home.
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StateMutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Mutex,
};

pub const PROVIDER_ID: &str = "openai-codex-oauth";
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const KEYCHAIN_SERVICE: &str = "synth-desktop";
const KEYCHAIN_ACCOUNT: &str = PROVIDER_ID;
const KEYCHAIN_SERVICE_ENV: &str = "SYNTH_DESKTOP_OAUTH_KEYCHAIN_SERVICE";
const DEV_CREDENTIAL_FILE_ENV: &str = "SYNTH_DESKTOP_DEV_OAUTH_FILE";

fn keychain_service() -> String {
    std::env::var(KEYCHAIN_SERVICE_ENV)
        .ok()
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
        .unwrap_or_else(|| KEYCHAIN_SERVICE.to_string())
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Credential {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub expires_ms: i64,
    pub account_id: String,
    pub account_hint: Option<String>,
    pub last_refresh_ms: i64,
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credential")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("id_token", &"<redacted>")
            .field("expires_ms", &self.expires_ms)
            .field("account_id", &self.account_id)
            .field("account_hint", &self.account_hint)
            .field("last_refresh_ms", &self.last_refresh_ms)
            .finish()
    }
}

pub trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Option<Credential>>;
    fn save(&self, credential: &Credential) -> Result<()>;
    fn delete(&self) -> Result<()>;
}

pub struct SystemKeychain {
    service: String,
}

/// Named local Workshop instances are rebuilt frequently. They must not read
/// the installed application's credential item: macOS binds that access to a
/// code-signing requirement and would otherwise prompt after local rebuilds.
struct LocalInstanceCredentialStore;

/// A named debug instance reads the canonical Codex file only as its initial
/// seed. Successful refresh/re-auth and disconnect are persisted in an
/// instance-local private overlay, so the source remains read-only and a
/// rebuilt app never invokes Keychain.
struct DevFileCredentialStore {
    seed: PathBuf,
    local: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum InstanceCredentialState {
    Connected { credential: Credential },
    Disconnected,
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<()> {
    Ok(())
}

impl CredentialStore for LocalInstanceCredentialStore {
    fn load(&self) -> Result<Option<Credential>> {
        Ok(None)
    }

    fn save(&self, _credential: &Credential) -> Result<()> {
        bail!("ChatGPT sign-in is unavailable in an isolated local Workshop instance")
    }

    fn delete(&self) -> Result<()> {
        Ok(())
    }
}

impl CredentialStore for DevFileCredentialStore {
    fn load(&self) -> Result<Option<Credential>> {
        match fs::read_to_string(&self.local) {
            Ok(value) => {
                return match serde_json::from_str::<InstanceCredentialState>(&value)
                    .context("invalid instance-local Codex OAuth state")?
                {
                    InstanceCredentialState::Connected { credential } => Ok(Some(credential)),
                    InstanceCredentialState::Disconnected => Ok(None),
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        match fs::read_to_string(&self.seed) {
            Ok(value) => Ok(Some(parse_dev_auth_file(&value)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn save(&self, credential: &Credential) -> Result<()> {
        self.write_local(&InstanceCredentialState::Connected {
            credential: credential.clone(),
        })
    }

    fn delete(&self) -> Result<()> {
        self.write_local(&InstanceCredentialState::Disconnected)
    }
}

impl DevFileCredentialStore {
    fn write_local(&self, state: &InstanceCredentialState) -> Result<()> {
        let parent = self
            .local
            .parent()
            .expect("instance-local OAuth path must have a parent directory");
        fs::create_dir_all(parent)?;
        let temporary = self.local.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(state)?)?;
        set_private_file(&temporary)?;
        fs::rename(&temporary, &self.local)?;
        set_private_file(&self.local)?;
        Ok(())
    }
}

fn parse_dev_auth_file(value: &str) -> Result<Credential> {
    if let Ok(credential) = serde_json::from_str::<Credential>(value) {
        return Ok(credential);
    }
    let auth: serde_json::Value =
        serde_json::from_str(value).context("invalid debug Codex OAuth file")?;
    if auth.get("auth_mode").and_then(|value| value.as_str()) != Some("chatgpt") {
        bail!("debug Codex OAuth file is not a ChatGPT credential");
    }
    let tokens = auth
        .get("tokens")
        .and_then(|value| value.as_object())
        .ok_or_else(|| anyhow!("debug Codex OAuth file omitted tokens"))?;
    let required = |name: &str| {
        tokens
            .get(name)
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("debug Codex OAuth file omitted {name}"))
    };
    let id_token = required("id_token")?;
    let claims = jwt_claims(&id_token)?;
    let account_id = tokens
        .get("account_id")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .or_else(|| account_id(&claims))
        .ok_or_else(|| anyhow!("debug Codex OAuth file omitted account_id"))?;
    let expires_ms = claims
        .get("exp")
        .and_then(|value| value.as_i64())
        .unwrap_or_else(|| Utc::now().timestamp() + 3600)
        * 1000;
    let last_refresh_ms = auth
        .get("last_refresh")
        .and_then(|value| value.as_str())
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis())
        .unwrap_or_else(|| Utc::now().timestamp_millis());
    Ok(Credential {
        access_token: required("access_token")?,
        refresh_token: required("refresh_token")?,
        id_token,
        expires_ms,
        account_id,
        account_hint: None,
        last_refresh_ms,
    })
}

impl SystemKeychain {
    fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, KEYCHAIN_ACCOUNT).map_err(Into::into)
    }
}

impl CredentialStore for SystemKeychain {
    fn load(&self) -> Result<Option<Credential>> {
        match self.entry()?.get_password() {
            Ok(value) => Ok(Some(
                serde_json::from_str(&value).context("invalid Codex OAuth keychain entry")?,
            )),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn save(&self, credential: &Credential) -> Result<()> {
        self.entry()?
            .set_password(&serde_json::to_string(credential)?)?;
        Ok(())
    }

    fn delete(&self) -> Result<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BeginResult {
    pub authorize_url: String,
    pub mode: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: AuthState,
    pub action: AuthAction,
    pub can_use_models: bool,
    pub guidance: String,
    pub configured: bool,
    pub account_hint: Option<String>,
    pub last_refresh: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    Disconnected,
    Authenticating,
    Ready,
    Expiring,
    Expired,
    RefreshFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AuthAction {
    Connect,
    Wait,
    None,
    Reauthenticate,
    Retry,
}

#[derive(Clone, Debug)]
struct AuthFailure(String);

#[derive(Clone)]
struct Pending {
    verifier: String,
    state: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<i64>,
}

pub struct Manager {
    store: Arc<dyn CredentialStore>,
    client: reqwest::Client,
    authorize_url: String,
    token_url: String,
    pending: Mutex<Option<Pending>>,
    failure: StateMutex<Option<AuthFailure>>,
    authenticating: AtomicBool,
}

impl Manager {
    pub fn production() -> Self {
        let named_instance = std::env::var_os("SYNTH_DESKTOP_INSTANCE").is_some();
        let dev_file = std::env::var_os(DEV_CREDENTIAL_FILE_ENV).map(PathBuf::from);
        let store: Arc<dyn CredentialStore> =
            match credential_store_mode(named_instance, cfg!(debug_assertions), dev_file.is_some())
            {
                CredentialStoreMode::Canonical => Arc::new(SystemKeychain::new(keychain_service())),
                CredentialStoreMode::Isolated => Arc::new(LocalInstanceCredentialStore),
                CredentialStoreMode::DevFile => Arc::new(DevFileCredentialStore {
                    seed: dev_file.expect("debug credential file mode requires a path"),
                    local: crate::storage::app_data_root().join("oauth/codex.json"),
                }),
            };
        Self::new(store, AUTHORIZE_URL, TOKEN_URL)
    }

    pub fn new(store: Arc<dyn CredentialStore>, authorize_url: &str, token_url: &str) -> Self {
        Self {
            store,
            client: reqwest::Client::new(),
            authorize_url: authorize_url.into(),
            token_url: token_url.into(),
            pending: Mutex::new(None),
            failure: StateMutex::new(None),
            authenticating: AtomicBool::new(false),
        }
    }

    pub async fn begin(self: &Arc<Self>) -> Result<BeginResult> {
        let mut guard = self.pending.lock().await;
        if guard.is_some() {
            bail!("A ChatGPT subscription sign-in is already in progress");
        }
        let verifier = random_urlsafe(32);
        let state = random_urlsafe(24);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url = reqwest::Url::parse(&self.authorize_url)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("scope", "openid profile email offline_access")
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            // Codex uses this to receive the organization-aware identity claims
            // that carry the ChatGPT account id used in auth.json.
            .append_pair("id_token_add_organizations", "true")
            .append_pair("codex_cli_simplified_flow", "true")
            .append_pair("originator", "codex_cli_rs");
        *guard = Some(Pending { verifier, state });
        self.authenticating.store(true, Ordering::Release);
        *self
            .failure
            .lock()
            .expect("OAuth failure state mutex poisoned") = None;
        drop(guard);

        let mode = match TcpListener::bind(("127.0.0.1", 1455)).await {
            Ok(listener) => {
                let manager = self.clone();
                tokio::spawn(async move { manager.accept_callback(listener).await });
                "auto"
            }
            Err(_) => "manual",
        };
        Ok(BeginResult {
            authorize_url: url.into(),
            mode: mode.into(),
        })
    }

    async fn accept_callback(self: Arc<Self>, listener: TcpListener) {
        let Ok(Ok((mut stream, _))) =
            tokio::time::timeout(Duration::from_secs(300), listener.accept()).await
        else {
            *self.pending.lock().await = None;
            self.authenticating.store(false, Ordering::Release);
            *self
                .failure
                .lock()
                .expect("OAuth failure state mutex poisoned") = Some(AuthFailure(
                "ChatGPT sign-in timed out. Choose Start over in Settings → Models to create a fresh authorization attempt."
                    .into(),
            ));
            return;
        };
        let mut bytes = vec![0; 16 * 1024];
        let read = stream.read(&mut bytes).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&bytes[..read]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1));
        let result = match target {
            Some(target) => self.complete_manual(target).await,
            None => Err(anyhow!("OAuth callback was malformed")),
        };
        if let Err(error) = &result {
            self.authenticating.store(false, Ordering::Release);
            *self
                .failure
                .lock()
                .expect("OAuth failure state mutex poisoned") = Some(AuthFailure(format!(
                "ChatGPT sign-in failed: {}. Re-sync from Settings → Models.",
                redact_text(&format!("{error:#}"))
            )));
            *self.pending.lock().await = None;
        }
        let (status, body) = if result.is_ok() {
            (
                "200 OK",
                "ChatGPT subscription connected. You can close this window.".into(),
            )
        } else {
            let detail = result
                .as_ref()
                .err()
                .map(|error| redact_text(&format!("{error:#}")))
                .unwrap_or_else(|| "Unknown callback error".into());
            (
                "400 Bad Request",
                format!("ChatGPT subscription sign-in failed: {detail}. Return to Workshop and try again."),
            )
        };
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let _ = stream.write_all(response.as_bytes()).await;
    }

    pub async fn complete_manual(&self, pasted: &str) -> Result<Status> {
        let (code, supplied_state) = parse_callback(pasted)?;
        let pending = self
            .pending
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow!("No ChatGPT subscription sign-in is in progress"))?;
        if let Some(state) = supplied_state {
            if state != pending.state {
                bail!("OAuth state did not match; no credentials were stored");
            }
        } else {
            bail!("OAuth callback is missing state; no credentials were stored");
        }
        let response = self
            .client
            .post(&self.token_url)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", CLIENT_ID),
                ("code", code.as_str()),
                ("redirect_uri", REDIRECT_URI),
                ("code_verifier", pending.verifier.as_str()),
            ])
            .send()
            .await
            .context("Codex OAuth token exchange failed")?;
        if !response.status().is_success() {
            bail!(
                "Codex OAuth token exchange was rejected ({})",
                response.status()
            );
        }
        let token: TokenResponse = response
            .json()
            .await
            .context("Codex OAuth token response was invalid")?;
        let id_token = token
            .id_token
            .ok_or_else(|| anyhow!("Codex OAuth response omitted the identity token"))?;
        let claims = jwt_claims(&id_token)?;
        let account_id = account_id(&claims)
            .ok_or_else(|| anyhow!("Codex OAuth identity did not include a ChatGPT account id"))?;
        let now = Utc::now().timestamp_millis();
        let credential = Credential {
            access_token: token.access_token,
            refresh_token: token
                .refresh_token
                .ok_or_else(|| anyhow!("Codex OAuth response omitted the refresh token"))?,
            id_token,
            expires_ms: now + token.expires_in.unwrap_or(3600) * 1000,
            account_id,
            account_hint: claims
                .get("email")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
            last_refresh_ms: now,
        };
        self.store.save(&credential)?;
        *self
            .failure
            .lock()
            .expect("OAuth failure state mutex poisoned") = None;
        *self.pending.lock().await = None;
        self.authenticating.store(false, Ordering::Release);
        self.status()
    }

    pub async fn fresh_credential(&self) -> Result<Option<Credential>> {
        let Some(current) = self.store.load()? else {
            return Ok(None);
        };
        if current.expires_ms > Utc::now().timestamp_millis() + 5 * 60 * 1000 {
            return Ok(Some(current));
        }
        let response = match self
            .client
            .post(&self.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", current.refresh_token.as_str()),
            ])
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                let message = format!("ChatGPT subscription refresh failed: {}. Retry from Settings → Models; Workshop will not fall back to another provider.", redact_text(&error.to_string()));
                *self
                    .failure
                    .lock()
                    .expect("OAuth failure state mutex poisoned") =
                    Some(AuthFailure(message.clone()));
                bail!(message);
            }
        };
        if !response.status().is_success() {
            if matches!(response.status().as_u16(), 400 | 401) {
                let message = "ChatGPT subscription authorization expired. Reauthenticate in Settings → Models; Workshop will not fall back to another provider.".to_owned();
                *self
                    .failure
                    .lock()
                    .expect("OAuth failure state mutex poisoned") =
                    Some(AuthFailure(message.clone()));
                bail!(message);
            }
            let message = format!("ChatGPT subscription refresh failed (HTTP {}). Retry from Settings → Models; Workshop will not fall back to another provider.", response.status());
            *self
                .failure
                .lock()
                .expect("OAuth failure state mutex poisoned") = Some(AuthFailure(message.clone()));
            bail!(message);
        }
        let token: TokenResponse = response
            .json()
            .await
            .context("Codex OAuth refresh response was invalid")?;
        let now = Utc::now().timestamp_millis();
        let mut refreshed = current;
        refreshed.access_token = token.access_token;
        if let Some(value) = token.refresh_token {
            refreshed.refresh_token = value;
        }
        if let Some(value) = token.id_token {
            let claims = jwt_claims(&value)?;
            if account_id(&claims).as_deref() != Some(refreshed.account_id.as_str()) {
                bail!("Codex OAuth refresh returned a different ChatGPT account");
            }
            refreshed.id_token = value;
        }
        refreshed.expires_ms = now + token.expires_in.unwrap_or(3600) * 1000;
        refreshed.last_refresh_ms = now;
        self.store.save(&refreshed)?;
        *self
            .failure
            .lock()
            .expect("OAuth failure state mutex poisoned") = None;
        Ok(Some(refreshed))
    }

    pub async fn ensure_ready(&self) -> Result<Status> {
        match self.fresh_credential().await {
            Ok(Some(_)) => self.status(),
            Ok(None) => Ok(self.status()?),
            Err(_) => Ok(self.status()?),
        }
    }

    pub fn status(&self) -> Result<Status> {
        let credential = self.store.load()?;
        let failure = self
            .failure
            .lock()
            .expect("OAuth failure state mutex poisoned")
            .clone();
        let now = Utc::now().timestamp_millis();
        let (state, action, can_use_models, guidance) = if let Some(AuthFailure(detail)) = failure {
            (AuthState::RefreshFailed, AuthAction::Retry, false, detail)
        } else if let Some(value) = credential.as_ref() {
            if value.expires_ms <= now {
                (AuthState::Expired, AuthAction::Reauthenticate, false,
                 "ChatGPT subscription authorization expired. Reauthenticate in Settings → Models; Workshop will not send or fall back.".into())
            } else if value.expires_ms <= now + 5 * 60 * 1000 {
                (
                    AuthState::Expiring,
                    AuthAction::Retry,
                    true,
                    "ChatGPT authorization is expiring; Workshop will refresh it before sending."
                        .into(),
                )
            } else {
                (
                    AuthState::Ready,
                    AuthAction::None,
                    true,
                    "ChatGPT subscription is ready.".into(),
                )
            }
        } else if self.authenticating.load(Ordering::Acquire) {
            (
                AuthState::Authenticating,
                AuthAction::Wait,
                false,
                "Finish ChatGPT sign-in in the browser, or cancel this attempt.".into(),
            )
        } else {
            (AuthState::Disconnected, AuthAction::Connect, false,
             "Connect a ChatGPT subscription in Settings → Models. Workshop will not fall back to another provider.".into())
        };
        Ok(Status {
            state,
            action,
            can_use_models,
            guidance,
            configured: credential.is_some(),
            account_hint: credential
                .as_ref()
                .and_then(|c| c.account_hint.clone())
                .or_else(|| credential.as_ref().map(|c| mask_account(&c.account_id))),
            last_refresh: credential
                .as_ref()
                .and_then(|c| millis_iso(c.last_refresh_ms)),
            expires_at: credential.as_ref().and_then(|c| millis_iso(c.expires_ms)),
        })
    }

    pub async fn cancel(&self) -> Result<()> {
        *self.pending.lock().await = None;
        self.authenticating.store(false, Ordering::Release);
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<Status> {
        self.cancel().await?;
        self.store.delete()?;
        *self
            .failure
            .lock()
            .expect("OAuth failure state mutex poisoned") = None;
        crate::session::codex::scrub_oauth_auth_files()?;
        self.status()
    }

    /// Adopt tokens refreshed by Codex, then remove the session copy.
    pub fn sync_from_session(&self, session_id: &str) -> Result<()> {
        let path = crate::session::codex::oauth_auth_path(session_id);
        let Ok(body) = std::fs::read_to_string(&path) else {
            return Ok(());
        };
        let value: serde_json::Value =
            serde_json::from_str(&body).context("Codex session auth file was invalid")?;
        if value.get("auth_mode").and_then(|v| v.as_str()) != Some("chatgpt") {
            return Ok(());
        }
        let tokens = value
            .get("tokens")
            .and_then(|v| v.as_object())
            .ok_or_else(|| anyhow!("Codex session auth file omitted tokens"))?;
        let mut credential = self
            .store
            .load()?
            .ok_or_else(|| anyhow!("ChatGPT subscription was disconnected"))?;
        let required = |name: &str| {
            tokens
                .get(name)
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| anyhow!("Codex session auth file omitted {name}"))
        };
        let account_id = required("account_id")?;
        if account_id != credential.account_id {
            bail!("Codex refreshed credentials for a different ChatGPT account");
        }
        credential.id_token = required("id_token")?;
        credential.access_token = required("access_token")?;
        credential.refresh_token = required("refresh_token")?;
        credential.last_refresh_ms = Utc::now().timestamp_millis();
        self.store.save(&credential)?;
        std::fs::remove_file(path)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CredentialStoreMode {
    Canonical,
    Isolated,
    DevFile,
}

fn credential_store_mode(
    named_instance: bool,
    debug_build: bool,
    has_dev_file: bool,
) -> CredentialStoreMode {
    if !named_instance {
        CredentialStoreMode::Canonical
    } else if debug_build && has_dev_file {
        CredentialStoreMode::DevFile
    } else {
        CredentialStoreMode::Isolated
    }
}

fn random_urlsafe(len: usize) -> String {
    let mut bytes = vec![0; len];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn parse_callback(input: &str) -> Result<(String, Option<String>)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("Paste the full redirect URL from the browser");
    }
    let normalized = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_owned()
    } else if trimmed.starts_with('/') {
        format!("http://localhost:1455{trimmed}")
    } else if trimmed.starts_with("code=") {
        format!("http://localhost:1455/auth/callback?{trimmed}")
    } else if let Some((code, state)) = trimmed.split_once('#') {
        format!("http://localhost:1455/auth/callback?code={code}&state={state}")
    } else {
        bail!("Paste a redirect URL containing code and state");
    };
    let url = reqwest::Url::parse(&normalized)?;
    if !matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
        || url.port_or_known_default() != Some(1455)
        || url.path() != "/auth/callback"
    {
        bail!("OAuth callback must use {REDIRECT_URI}");
    }
    let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    let code = pairs
        .get("code")
        .filter(|v| !v.is_empty())
        .cloned()
        .ok_or_else(|| anyhow!("OAuth callback is missing code"))?;
    Ok((code, pairs.get("state").cloned()))
}

fn jwt_claims(jwt: &str) -> Result<serde_json::Value> {
    let payload = jwt
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow!("identity token was not a JWT"))?;
    Ok(serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(payload)
            .context("identity token payload was invalid")?,
    )?)
}

fn account_id(claims: &serde_json::Value) -> Option<String> {
    let direct = [
        "https://api.openai.com/auth/chatgpt_account_id",
        "chatgpt_account_id",
        "account_id",
    ]
    .into_iter()
    .find_map(|key| claims.get(key).and_then(|v| v.as_str()).map(str::to_owned));
    direct.or_else(|| {
        claims
            .get("https://api.openai.com/auth")
            .and_then(|claim| claim.get("chatgpt_account_id"))
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    })
}

fn mask_account(value: &str) -> String {
    if value.len() <= 8 {
        return "Connected account".into();
    }
    format!("{}…{}", &value[..4], &value[value.len() - 4..])
}

fn millis_iso(value: i64) -> Option<String> {
    DateTime::from_timestamp_millis(value).map(|dt| dt.to_rfc3339_opts(SecondsFormat::Secs, true))
}

pub fn redact_event_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if matches!(
                    key.to_ascii_lowercase().as_str(),
                    "accesstoken"
                        | "access_token"
                        | "refreshtoken"
                        | "refresh_token"
                        | "idtoken"
                        | "id_token"
                        | "authorization"
                        | "codeverifier"
                        | "code_verifier"
                ) {
                    *value = serde_json::Value::String("<redacted>".into());
                } else {
                    redact_event_value(value);
                }
            }
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(redact_event_value),
        serde_json::Value::String(text) => *text = redact_text(text),
        _ => {}
    }
}

pub fn redact_text(input: &str) -> String {
    let mut output = input.to_owned();
    for marker in ["code=", "access_token=", "refresh_token=", "id_token="] {
        let mut offset = 0;
        while let Some(found) = output[offset..].find(marker) {
            let start = offset + found + marker.len();
            let end = output[start..]
                .find(|character: char| matches!(character, '&' | '#' | ' ' | '\"' | '\''))
                .map(|value| start + value)
                .unwrap_or(output.len());
            output.replace_range(start..end, "<redacted>");
            offset = start + "<redacted>".len();
        }
    }
    if let Some(start) = output.find("Bearer ") {
        let value_start = start + "Bearer ".len();
        let end = output[value_start..]
            .find(|character: char| character.is_whitespace() || matches!(character, '\"' | '\''))
            .map(|value| value_start + value)
            .unwrap_or(output.len());
        output.replace_range(value_start..end, "<redacted>");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::RwLock;

    #[derive(Default)]
    struct MemoryKeychain(RwLock<Option<Credential>>);

    impl CredentialStore for MemoryKeychain {
        fn load(&self) -> Result<Option<Credential>> {
            Ok(self.0.read().unwrap().clone())
        }

        fn save(&self, value: &Credential) -> Result<()> {
            *self.0.write().unwrap() = Some(value.clone());
            Ok(())
        }

        fn delete(&self) -> Result<()> {
            *self.0.write().unwrap() = None;
            Ok(())
        }
    }

    #[test]
    fn local_instance_store_never_touches_or_accepts_credentials() {
        let store = LocalInstanceCredentialStore;
        assert!(store.load().unwrap().is_none());
        assert!(store.delete().is_ok());
        assert!(store
            .save(&Credential {
                access_token: "access".into(),
                refresh_token: "refresh".into(),
                id_token: "id".into(),
                expires_ms: 0,
                account_id: "account".into(),
                account_hint: None,
                last_refresh_ms: 0,
            })
            .unwrap_err()
            .to_string()
            .contains("unavailable in an isolated local Workshop instance"));
    }

    #[test]
    fn debug_oauth_file_is_explicit_named_debug_only() {
        assert_eq!(
            credential_store_mode(false, true, true),
            CredentialStoreMode::Canonical
        );
        assert_eq!(
            credential_store_mode(true, true, false),
            CredentialStoreMode::Isolated
        );
        assert_eq!(
            credential_store_mode(true, false, true),
            CredentialStoreMode::Isolated
        );
        assert_eq!(
            credential_store_mode(true, true, true),
            CredentialStoreMode::DevFile
        );
    }

    #[test]
    fn debug_file_store_seeds_then_persists_only_instance_local_state() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("oauth.json");
        let id_token = format!(
            "x.{}.y",
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&serde_json::json!({
                    "exp": 2_000_000_000_i64,
                    "https://api.openai.com/auth": {"chatgpt_account_id": "file-account"}
                }))
                .unwrap()
            )
        );
        let body = serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "access_token": "file-access",
                "refresh_token": "file-refresh",
                "id_token": id_token,
                "account_id": "file-account"
            },
            "last_refresh": "2026-08-12T18:00:00Z"
        });
        fs::write(&path, serde_json::to_vec_pretty(&body).unwrap()).unwrap();
        let before = fs::read(&path).unwrap();
        let local = temp.path().join("instance/oauth/codex.json");
        let store = DevFileCredentialStore {
            seed: path.clone(),
            local: local.clone(),
        };
        let mut loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.access_token, "file-access");
        assert_eq!(loaded.account_id, "file-account");
        loaded.access_token = "instance-access".into();
        store.save(&loaded).unwrap();
        assert_eq!(
            store.load().unwrap().unwrap().access_token,
            "instance-access"
        );
        assert!(local.is_file());
        store.delete().unwrap();
        assert!(store.load().unwrap().is_none());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    async fn token_server(body: serde_json::Value) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 8192];
            let _ = stream.read(&mut request).await.unwrap();
            let body = serde_json::to_string(&body).unwrap();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{address}/token")
    }

    fn test_jwt(account: &str) -> String {
        let claims = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "https://api.openai.com/auth": {
                    "chatgpt_account_id": account
                },
                "email": "person@example.com"
            }))
            .unwrap(),
        );
        format!("e30.{claims}.signature")
    }

    #[test]
    fn parses_supported_manual_callback_shapes() {
        for value in [
            "http://localhost:1455/auth/callback?code=abc&state=xyz",
            "/auth/callback?code=abc&state=xyz",
            "code=abc&state=xyz",
            "abc#xyz",
        ] {
            assert_eq!(
                parse_callback(value).unwrap(),
                ("abc".into(), Some("xyz".into()))
            );
        }
        assert!(parse_callback("garbage").is_err());
        assert!(parse_callback("http://localhost:1456/auth/callback?code=x&state=y").is_err());
    }

    #[test]
    fn status_never_serializes_tokens() {
        let store = Arc::new(MemoryKeychain::default());
        store
            .save(&Credential {
                access_token: "access-secret".into(),
                refresh_token: "refresh-secret".into(),
                id_token: "id-secret".into(),
                expires_ms: 2_000_000_000_000,
                account_id: "acct_123456789".into(),
                account_hint: Some("person@example.com".into()),
                last_refresh_ms: 1_700_000_000_000,
            })
            .unwrap();
        let manager = Manager::new(store, AUTHORIZE_URL, TOKEN_URL);
        let json = serde_json::to_string(&manager.status().unwrap()).unwrap();
        assert!(!json.contains("secret"));
        assert!(json.contains("person@example.com"));
    }

    #[test]
    fn expired_credentials_are_not_reported_as_usable() {
        let store = Arc::new(MemoryKeychain::default());
        store
            .save(&Credential {
                access_token: "expired".into(),
                refresh_token: "refresh".into(),
                id_token: "id".into(),
                expires_ms: Utc::now().timestamp_millis() - 1,
                account_id: "acct_expired".into(),
                account_hint: None,
                last_refresh_ms: 0,
            })
            .unwrap();
        let status = Manager::new(store, AUTHORIZE_URL, TOKEN_URL)
            .status()
            .unwrap();
        assert_eq!(status.state, AuthState::Expired);
        assert_eq!(status.action, AuthAction::Reauthenticate);
        assert!(!status.can_use_models);
        assert!(
            status.configured,
            "stored and usable are intentionally distinct states"
        );
    }

    #[test]
    fn callback_bearer_and_structured_tokens_are_redacted() {
        let redacted = redact_text(
            "GET /auth/callback?code=secret&state=ok Authorization: Bearer eyJ.secret.sig",
        );
        assert!(!redacted.contains("code=secret"));
        assert!(!redacted.contains("eyJ.secret.sig"));
        let mut value = serde_json::json!({"error":{"access_token":"secret", "message":"refresh_token=also-secret"}});
        redact_event_value(&mut value);
        let body = value.to_string();
        assert!(!body.contains("also-secret"));
        assert!(!body.contains("\"secret\""));
    }

    #[tokio::test]
    async fn begin_generates_pkce_and_cancel_is_safe() {
        let manager = Arc::new(Manager::new(
            Arc::new(MemoryKeychain::default()),
            AUTHORIZE_URL,
            TOKEN_URL,
        ));
        let begin = manager.begin().await.unwrap();
        let url = reqwest::Url::parse(&begin.authorize_url).unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(
            query.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert_eq!(query.get("client_id").map(String::as_str), Some(CLIENT_ID));
        assert_eq!(
            query.get("codex_cli_simplified_flow").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            query.get("id_token_add_organizations").map(String::as_str),
            Some("true")
        );
        assert!(manager.begin().await.is_err());
        manager.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn manual_exchange_persists_only_in_keychain_and_disconnect_clears_it() {
        let store = Arc::new(MemoryKeychain::default());
        let token_url = token_server(serde_json::json!({
            "access_token": "access-secret",
            "refresh_token": "refresh-secret",
            "id_token": test_jwt("acct_123456789"),
            "expires_in": 3600
        }))
        .await;
        let manager = Arc::new(Manager::new(store.clone(), AUTHORIZE_URL, &token_url));
        manager.begin().await.unwrap();
        let state = manager.pending.lock().await.as_ref().unwrap().state.clone();
        let status = manager
            .complete_manual(&format!(
                "http://localhost:1455/auth/callback?code=fixture&state={state}"
            ))
            .await
            .unwrap();
        assert!(status.configured);
        assert_eq!(status.account_hint.as_deref(), Some("person@example.com"));
        assert_eq!(store.load().unwrap().unwrap().access_token, "access-secret");
        manager.disconnect().await.unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[tokio::test]
    async fn near_expiry_refreshes_and_rotates_the_keychain_entry() {
        let store = Arc::new(MemoryKeychain::default());
        store
            .save(&Credential {
                access_token: "old-access".into(),
                refresh_token: "old-refresh".into(),
                id_token: test_jwt("acct_123456789"),
                expires_ms: Utc::now().timestamp_millis() + 1_000,
                account_id: "acct_123456789".into(),
                account_hint: None,
                last_refresh_ms: 0,
            })
            .unwrap();
        let token_url = token_server(serde_json::json!({
            "access_token": "new-access",
            "refresh_token": "new-refresh",
            "expires_in": 7200
        }))
        .await;
        let manager = Manager::new(store.clone(), AUTHORIZE_URL, &token_url);
        let refreshed = manager.fresh_credential().await.unwrap().unwrap();
        assert_eq!(refreshed.access_token, "new-access");
        assert_eq!(refreshed.refresh_token, "new-refresh");
        assert_eq!(store.load().unwrap().unwrap(), refreshed);
    }
}
