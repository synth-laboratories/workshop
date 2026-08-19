//! Private secret storage. Values never cross the agent RPC boundary.

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Bytes of a provider credential. `Debug` never prints the value.
#[derive(Clone)]
pub struct SecretBytes(pub Vec<u8>);

impl SecretBytes {
    pub fn from_utf8(value: &str) -> Self {
        Self(value.as_bytes().to_vec())
    }

    pub fn from_owned(value: String) -> Self {
        Self(value.into_bytes())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_utf8(&self) -> Result<&str> {
        std::str::from_utf8(&self.0).context("secret is not valid UTF-8")
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretBytes(<redacted>)")
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            *byte = 0;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendStatus {
    Present,
    Missing,
    Locked,
}

/// Trusted-host vault adapter. `resolve` is the only read path and must not be
/// exposed over the agent RPC router.
pub trait SecretBackend: Send + Sync {
    fn create(&self, secret_id: &str, value: &SecretBytes) -> Result<String>;
    fn replace(&self, backend_ref: &str, value: &SecretBytes) -> Result<()>;
    fn delete(&self, backend_ref: &str) -> Result<()>;
    fn resolve(&self, backend_ref: &str) -> Result<SecretBytes>;
    fn status(&self, backend_ref: &str) -> Result<BackendStatus>;
    fn backend_name(&self) -> &'static str;
}

/// Process-local map used by tests and isolated local Workshop instances.
#[derive(Default)]
pub struct MemoryBackend {
    items: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretBackend for MemoryBackend {
    fn create(&self, secret_id: &str, value: &SecretBytes) -> Result<String> {
        let backend_ref = format!("mem:{secret_id}");
        self.items
            .lock()
            .expect("memory secret backend")
            .insert(backend_ref.clone(), value.0.clone());
        Ok(backend_ref)
    }

    fn replace(&self, backend_ref: &str, value: &SecretBytes) -> Result<()> {
        let mut items = self.items.lock().expect("memory secret backend");
        if !items.contains_key(backend_ref) {
            anyhow::bail!("secret is not stored in the local vault");
        }
        items.insert(backend_ref.to_owned(), value.0.clone());
        Ok(())
    }

    fn delete(&self, backend_ref: &str) -> Result<()> {
        self.items
            .lock()
            .expect("memory secret backend")
            .remove(backend_ref);
        Ok(())
    }

    fn resolve(&self, backend_ref: &str) -> Result<SecretBytes> {
        self.items
            .lock()
            .expect("memory secret backend")
            .get(backend_ref)
            .cloned()
            .map(SecretBytes)
            .ok_or_else(|| anyhow!("secret is not stored in the local vault"))
    }

    fn status(&self, backend_ref: &str) -> Result<BackendStatus> {
        Ok(
            if self
                .items
                .lock()
                .expect("memory secret backend")
                .contains_key(backend_ref)
            {
                BackendStatus::Present
            } else {
                BackendStatus::Missing
            },
        )
    }

    fn backend_name(&self) -> &'static str {
        "memory"
    }
}

/// OS credential store via the `keyring` crate (Keychain / Credential Manager /
/// Secret Service). The service name is instance-scoped so a named local
/// Workshop rebuild does not collide with the installed app.
pub struct OsKeychainBackend {
    service: String,
}

impl OsKeychainBackend {
    pub fn new() -> Self {
        let instance = std::env::var(crate::instance::INSTANCE_ENV)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "canonical".into());
        let service = format!("synth-desktop.secrets.{instance}");
        Self { service }
    }

    fn entry(&self, backend_ref: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, backend_ref).map_err(Into::into)
    }
}

impl SecretBackend for OsKeychainBackend {
    fn create(&self, secret_id: &str, value: &SecretBytes) -> Result<String> {
        let backend_ref = format!("kc:{secret_id}");
        self.entry(&backend_ref)?
            .set_password(value.as_utf8()?)
            .context("store secret in the OS credential store")?;
        Ok(backend_ref)
    }

    fn replace(&self, backend_ref: &str, value: &SecretBytes) -> Result<()> {
        self.entry(backend_ref)?
            .set_password(value.as_utf8()?)
            .context("replace secret in the OS credential store")
    }

    fn delete(&self, backend_ref: &str) -> Result<()> {
        match self.entry(backend_ref)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error).context("delete secret from the OS credential store"),
        }
    }

    fn resolve(&self, backend_ref: &str) -> Result<SecretBytes> {
        match self.entry(backend_ref)?.get_password() {
            Ok(value) => Ok(SecretBytes::from_owned(value)),
            Err(keyring::Error::NoEntry) => {
                Err(anyhow!("secret is not stored in the OS credential store"))
            }
            Err(error) => {
                let message = error.to_string().to_ascii_lowercase();
                if message.contains("lock") || message.contains("denied") {
                    Err(anyhow!("the OS credential store is locked"))
                } else {
                    Err(error).context("read secret from the OS credential store")
                }
            }
        }
    }

    fn status(&self, backend_ref: &str) -> Result<BackendStatus> {
        match self.entry(backend_ref)?.get_password() {
            Ok(_) => Ok(BackendStatus::Present),
            Err(keyring::Error::NoEntry) => Ok(BackendStatus::Missing),
            Err(error) => {
                let message = error.to_string().to_ascii_lowercase();
                if message.contains("lock") || message.contains("denied") {
                    Ok(BackendStatus::Locked)
                } else {
                    Err(error).context("probe OS credential store")
                }
            }
        }
    }

    fn backend_name(&self) -> &'static str {
        "os-keychain"
    }
}

pub fn default_backend() -> Arc<dyn SecretBackend> {
    if cfg!(test) || std::env::var("SYNTH_DESKTOP_SECRETS_MEMORY").as_deref() == Ok("1") {
        return Arc::new(MemoryBackend::new());
    }
    Arc::new(OsKeychainBackend::new())
}
