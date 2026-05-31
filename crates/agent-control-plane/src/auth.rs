use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedDevice {
    pub device_id: String,
    pub platform: String,
    pub public_key_fingerprint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkedIdentityMethod {
    Passkey {
        credential_id: String,
    },
    Apple {
        subject: String,
        email: Option<String>,
    },
    Google {
        subject: String,
        email: Option<String>,
    },
    Email {
        email: String,
    },
    SecurityKey {
        credential_id: String,
        transport: String,
    },
}

impl LinkedIdentityMethod {
    pub fn passkey(credential_id: impl Into<String>) -> Self {
        Self::Passkey {
            credential_id: credential_id.into(),
        }
    }

    pub fn apple(subject: impl Into<String>, email: Option<&str>) -> Self {
        Self::Apple {
            subject: subject.into(),
            email: email.map(str::to_owned),
        }
    }

    pub fn google(subject: impl Into<String>, email: Option<&str>) -> Self {
        Self::Google {
            subject: subject.into(),
            email: email.map(str::to_owned),
        }
    }

    pub fn email(email: impl Into<String>) -> Self {
        Self::Email {
            email: email.into(),
        }
    }

    pub fn security_key(credential_id: impl Into<String>, transport: impl Into<String>) -> Self {
        Self::SecurityKey {
            credential_id: credential_id.into(),
            transport: transport.into(),
        }
    }

    fn storage_key(&self) -> String {
        match self {
            Self::Passkey { credential_id } => format!("passkey:{credential_id}"),
            Self::Apple { subject, .. } => format!("apple:{subject}"),
            Self::Google { subject, .. } => format!("google:{subject}"),
            Self::Email { email } => format!("email:{email}"),
            Self::SecurityKey { credential_id, .. } => format!("security-key:{credential_id}"),
        }
    }
}

pub trait SecretStore {
    fn put_secret(&mut self, key: &str, value: &str) -> Result<()>;
}

#[derive(Default)]
pub struct MemorySecretStore {
    secrets: HashMap<String, String>,
}

impl SecretStore for MemorySecretStore {
    fn put_secret(&mut self, key: &str, value: &str) -> Result<()> {
        self.secrets.insert(key.to_owned(), value.to_owned());
        Ok(())
    }
}

pub struct MacosKeychainStore {
    service_name: String,
}

impl MacosKeychainStore {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
        }
    }
}

impl SecretStore for MacosKeychainStore {
    fn put_secret(&mut self, key: &str, value: &str) -> Result<()> {
        let delete_status = std::process::Command::new("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-s",
                &self.service_name,
                "-a",
                key,
            ])
            .status();
        let _ = delete_status;

        let status = std::process::Command::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-U",
                "-s",
                &self.service_name,
                "-a",
                key,
                "-w",
                value,
            ])
            .status()
            .with_context(|| "write secret to macOS Keychain")?;
        if status.success() {
            Ok(())
        } else {
            anyhow::bail!("macOS Keychain rejected secret write for {key}");
        }
    }
}

pub struct AuthManager<S: SecretStore> {
    secret_store: S,
    device: Option<TrustedDevice>,
    linked_methods: Vec<LinkedIdentityMethod>,
}

impl<S: SecretStore> AuthManager<S> {
    pub fn new(secret_store: S) -> Self {
        Self {
            secret_store,
            device: None,
            linked_methods: Vec::new(),
        }
    }

    pub fn register_device(
        &mut self,
        device_id: impl Into<String>,
        platform: impl Into<String>,
        public_key_material: &str,
    ) -> Result<()> {
        self.device = Some(TrustedDevice {
            device_id: device_id.into(),
            platform: platform.into(),
            public_key_fingerprint: fingerprint(public_key_material),
        });
        Ok(())
    }

    pub fn link_method(
        &mut self,
        method: LinkedIdentityMethod,
        provider_secret: Option<&str>,
    ) -> Result<()> {
        if let Some(secret) = provider_secret {
            self.secret_store
                .put_secret(&method.storage_key(), secret)?;
        }
        self.linked_methods.push(method);
        Ok(())
    }

    pub fn device(&self) -> Option<&TrustedDevice> {
        self.device.as_ref()
    }

    pub fn linked_methods(&self) -> &[LinkedIdentityMethod] {
        &self.linked_methods
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudAuthContract {
    pub account_id: String,
    pub trusted_devices: Vec<TrustedDevice>,
    pub linked_methods: Vec<LinkedIdentityMethod>,
}

impl CloudAuthContract {
    pub fn from_manager<S: SecretStore>(manager: &AuthManager<S>, account_id: &str) -> Self {
        Self {
            account_id: account_id.to_owned(),
            trusted_devices: manager.device().cloned().into_iter().collect(),
            linked_methods: manager.linked_methods().to_vec(),
        }
    }
}

fn fingerprint(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
