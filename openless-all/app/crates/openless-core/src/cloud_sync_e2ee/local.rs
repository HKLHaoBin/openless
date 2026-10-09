//! Encrypted local baselines and unresolved operations. The wrapping key lives
//! only in the host's system vault, never beside these files.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;
use zeroize::Zeroizing;

use crate::cloud_sync_e2ee_protocol::crypto::{open_local, seal_local, DerivedKey};
use crate::credentials::SyncSecretAccount;
use crate::{CredentialStore, SecretValue};

use super::{error, SyncResult};

#[derive(Clone)]
pub(crate) struct LocalStorage {
    root: PathBuf,
    origin: String,
    device_id: String,
    credentials: Arc<dyn CredentialStore>,
    cipher: Arc<OnceCell<Arc<DerivedKey>>>,
    io: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct KnownRemoteAccount {
    pub owner_id: String,
    pub vault_id: Option<String>,
    pub key_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ClientSettings {
    pub version: u32,
    pub enabled: bool,
    pub consent_version: Option<String>,
    pub owner_id: Option<String>,
    #[serde(default)]
    pub known_owner_ids: Vec<String>,
    #[serde(default)]
    pub known_accounts: Vec<KnownRemoteAccount>,
    #[serde(default)]
    pub remote_origin: Option<String>,
    pub remember_key: bool,
    #[serde(default)]
    pub key_preference_epoch: u64,
    pub last_success: Option<String>,
    pub last_generation: Option<String>,
    pub vault_id: Option<String>,
    pub key_id: Option<String>,
    pub prompted: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UiEnvelope {
    pub schema_version: u32,
    pub revision: String,
    pub value: crate::cloud_sync_e2ee_documents::SecretJson,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OriginMigrationManifest {
    version: u32,
    #[serde(default)]
    legacy_origin: Option<String>,
    entries: Vec<OriginMigrationEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OriginMigrationCompletion {
    version: u32,
    legacy_origin: String,
    manifest_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OriginMigrationEntry {
    owner: String,
    name: String,
    existed: bool,
}

pub(crate) struct LocalMigrationRecord {
    pub owner: String,
    pub name: String,
    pub value: serde_json::Value,
}

const ORIGIN_MIGRATION_VERSION: u32 = 1;
const MAX_ORIGIN_MIGRATION_ENTRIES: usize = 4096;

fn migration_manifest_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: false,
            consent_version: None,
            owner_id: None,
            known_owner_ids: Vec::new(),
            known_accounts: Vec::new(),
            remote_origin: None,
            remember_key: false,
            key_preference_epoch: 0,
            last_success: None,
            last_generation: None,
            vault_id: None,
            key_id: None,
            prompted: false,
        }
    }
}

impl LocalStorage {
    pub(crate) fn new(
        root: PathBuf,
        origin: String,
        device_id: String,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            root,
            origin,
            device_id,
            credentials,
            cipher: Arc::new(OnceCell::new()),
            io: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub(crate) fn origin(&self) -> &str {
        &self.origin
    }

    pub(crate) fn shares_root_with(&self, other: &Self) -> bool {
        self.root == other.root
    }

    pub(crate) fn with_origin(&self, origin: String) -> Self {
        Self::new(
            self.root.clone(),
            origin,
            self.device_id.clone(),
            self.credentials.clone(),
        )
    }

    pub(crate) fn origin_migration_complete(&self, legacy_origin: &str) -> SyncResult<bool> {
        let path = self.root.join(".origin-migration-complete-v1");
        match fs::read_to_string(path) {
            Ok(value) => {
                let value = value.trim();
                if value == legacy_origin {
                    return Ok(true);
                }
                let Ok(completion) = serde_json::from_str::<OriginMigrationCompletion>(value)
                else {
                    return Ok(false);
                };
                Ok(completion.version == ORIGIN_MIGRATION_VERSION
                    && completion.legacy_origin == legacy_origin)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(error("local_storage_unavailable")),
        }
    }

    pub(crate) fn recover_origin_migration(&self) -> SyncResult<()> {
        recover_origin_migration_sync(&self.root)
    }

    pub(crate) async fn prepare_migration_key(&self) -> SyncResult<()> {
        if self.cipher.get().is_some() {
            return Ok(());
        }
        let account = self.storage_key_account()?;
        let bytes = match self
            .credentials
            .read_sync_secret(account.clone())
            .await
            .map_err(|_| error("secure_storage_denied"))?
        {
            Some(value) => decode_key(value.expose_secret())?,
            None => {
                let mut bytes = Zeroizing::new([0_u8; 32]);
                getrandom::fill(&mut *bytes).map_err(|_| error("secure_random_unavailable"))?;
                self.credentials
                    .write_sync_secret(
                        account.clone(),
                        SecretValue::new(URL_SAFE_NO_PAD.encode(&bytes[..])),
                    )
                    .await
                    .map_err(|_| error("secure_storage_denied"))?;
                let verified = self
                    .credentials
                    .read_sync_secret(account)
                    .await
                    .map_err(|_| error("secure_storage_denied"))?
                    .ok_or_else(|| error("secure_storage_denied"))?;
                if decode_key(verified.expose_secret())?.as_ref() != bytes.as_ref() {
                    return Err(error("secure_storage_denied"));
                }
                bytes
            }
        };
        let _ = self
            .cipher
            .set(Arc::new(DerivedKey::from_secret_bytes(bytes)));
        Ok(())
    }

    pub(crate) async fn migrate_records(
        &self,
        legacy_origin: &str,
        records: Vec<LocalMigrationRecord>,
    ) -> SyncResult<()> {
        self.recover_origin_migration()?;
        self.prepare_migration_key().await?;
        let key = self.key().await?;
        let mut staged = Vec::with_capacity(records.len());
        for record in records {
            let aad = self.aad(&record.owner, &record.name)?;
            let bytes = serde_json::to_vec(&record.value)
                .map_err(|_| error("local_storage_unavailable"))?;
            let sealed =
                seal_local(&key, &aad, &bytes).map_err(|_| error("local_storage_unavailable"))?;
            staged.push((record.owner, record.name, sealed));
        }
        let root = self.root.clone();
        let legacy_origin = legacy_origin.to_owned();
        let io = self.io.clone().lock_owned().await;
        tokio::task::spawn_blocking(move || {
            let _io = io;
            commit_origin_migration(&root, &legacy_origin, staged)
        })
        .await
        .map_err(|_| error("local_storage_unavailable"))?
    }

    fn storage_key_account(&self) -> SyncResult<SyncSecretAccount> {
        let aad = self.aad("device", "storage-key")?;
        SyncSecretAccount::new(format!("cloud-sync.e2ee.local.{:x}", Sha256::digest(aad)))
            .map_err(|_| error("local_storage_unavailable"))
    }

    pub(crate) fn initialized(&self) -> SyncResult<bool> {
        self.path("device", "client")
            .try_exists()
            .map_err(|_| error("local_storage_unavailable"))
    }

    pub(crate) async fn read_ui(&self) -> SyncResult<Option<UiEnvelope>> {
        let value: Option<UiEnvelope> = self
            .read("device", "sync-ui-preferences")
            .await
            .inspect_err(|error| {
                log_local_failure("ui_mirror_read", error);
            })?;
        if let Some(value) = &value {
            if value.schema_version != 1
                || crate::cloud_sync_e2ee_protocol::types::UuidV4::parse(&value.revision).is_err()
            {
                return Err(error("recovery_required"));
            }
        }
        Ok(value)
    }

    pub(crate) fn binding(
        &self,
        owner: &str,
        vault: &str,
        key: &str,
    ) -> SyncResult<SyncSecretAccount> {
        let binding = serde_json::to_vec(&[&self.origin, owner, vault, key, &self.device_id])
            .map_err(|_| error("local_storage_unavailable"))?;
        SyncSecretAccount::new(format!("cloud-sync.e2ee.key.{:x}", Sha256::digest(binding)))
            .map_err(|_| error("local_storage_unavailable"))
    }

    pub(crate) async fn key(&self) -> SyncResult<Arc<DerivedKey>> {
        self.cipher
            .get_or_try_init(|| async {
                let aad = self.aad("device", "storage-key")?;
                let account = SyncSecretAccount::new(format!(
                    "cloud-sync.e2ee.local.{:x}",
                    Sha256::digest(aad)
                ))
                .map_err(|_| error("local_storage_unavailable"))?;
                let value = self
                    .credentials
                    .read_sync_secret(account.clone())
                    .await
                    .map_err(|_| {
                        log::warn!("[e2ee-local] stage=wrapping_key_read code=secure_storage_denied");
                        error("secure_storage_denied")
                    })?;
                let bytes = match value {
                    Some(value) => decode_key(value.expose_secret())?,
                    None => {
                        // Losing the system-vault key never silently resets encrypted recovery data.
                        let root = self.root.clone();
                        let has_state = tokio::task::spawn_blocking(move || {
                            if !root.exists() {
                                return Ok(false);
                            }
                            let files = fs::read_dir(root)
                                .map_err(|_| error("local_storage_unavailable"))?;
                            for file in files {
                                let file = file.map_err(|_| error("local_storage_unavailable"))?;
                                if file
                                    .path()
                                    .extension()
                                    .is_some_and(|extension| extension == "enc")
                                {
                                    return Ok(true);
                                }
                            }
                            Ok(false)
                        })
                        .await
                        .map_err(|_| error("local_storage_unavailable"))??;
                        if has_state {
                            return Err(error("recovery_required"));
                        }
                        let mut bytes = Zeroizing::new([0_u8; 32]);
                        getrandom::fill(&mut *bytes)
                            .map_err(|_| error("secure_random_unavailable"))?;
                        self.credentials
                            .write_sync_secret(
                                account.clone(),
                                SecretValue::new(URL_SAFE_NO_PAD.encode(&bytes[..])),
                            )
                            .await
                            .map_err(|_| {
                                log::warn!("[e2ee-local] stage=wrapping_key_write code=secure_storage_denied");
                                error("secure_storage_denied")
                            })?;
                        let verified = self
                            .credentials
                            .read_sync_secret(account)
                            .await
                            .map_err(|_| error("secure_storage_denied"))?
                            .ok_or_else(|| error("secure_storage_denied"))?;
                        if decode_key(verified.expose_secret())?.as_ref() != bytes.as_ref() {
                            return Err(error("secure_storage_denied"));
                        }
                        bytes
                    }
                };
                Ok(Arc::new(DerivedKey::from_secret_bytes(bytes)))
            })
            .await
            .cloned()
    }

    fn aad(&self, owner: &str, name: &str) -> SyncResult<Vec<u8>> {
        serde_json::to_vec(&[
            "openless.local-sync.v1",
            &self.origin,
            &self.device_id,
            owner,
            name,
        ])
        .map_err(|_| error("local_storage_unavailable"))
    }

    fn path(&self, owner: &str, name: &str) -> PathBuf {
        let id = format!("{owner}\0{name}");
        self.root
            .join(format!("{:x}.enc", Sha256::digest(id.as_bytes())))
    }

    pub(crate) async fn read<T: DeserializeOwned + Send + 'static>(
        &self,
        owner: &str,
        name: &str,
    ) -> SyncResult<Option<T>> {
        let io = self.io.clone().lock_owned().await;
        let path = self.path(owner, name);
        if !path
            .try_exists()
            .map_err(|_| error("local_storage_unavailable"))?
        {
            return Ok(None);
        }
        let key = self.key().await?;
        let aad = self.aad(owner, name)?;
        tokio::task::spawn_blocking(move || {
            let _io = io;
            let file = fs::File::open(path).map_err(|_| error("local_storage_unavailable"))?;
            if file
                .metadata()
                .map_err(|_| error("local_storage_unavailable"))?
                .len()
                > 41 * 1024 * 1024
            {
                return Err(error("recovery_required"));
            }
            use std::io::Read;
            let mut bytes = Vec::new();
            file.take(41 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| error("local_storage_unavailable"))?;
            let plain = open_local(&key, &aad, &bytes).map_err(|_| error("recovery_required"))?;
            serde_json::from_slice(&plain)
                .map(Some)
                .map_err(|_| error("recovery_required"))
        })
        .await
        .map_err(|_| error("local_storage_unavailable"))?
    }

    pub(crate) async fn write<T: Serialize + Send + 'static>(
        &self,
        owner: &str,
        name: &str,
        value: T,
    ) -> SyncResult<()> {
        let key = self.key().await?;
        let io = self.io.clone().lock_owned().await;
        let aad = self.aad(owner, name)?;
        let path = self.path(owner, name);
        tokio::task::spawn_blocking(move || {
            let _io = io;
            let plain = Zeroizing::new(
                serde_json::to_vec(&value).map_err(|_| error("local_storage_unavailable"))?,
            );
            let sealed =
                seal_local(&key, &aad, &plain).map_err(|_| error("local_storage_unavailable"))?;
            durable_replace(&path, &sealed)
        })
        .await
        .map_err(|_| error("local_storage_unavailable"))?
    }

    pub(crate) async fn remove(&self, owner: &str, name: &str) -> SyncResult<()> {
        let io = self.io.clone().lock_owned().await;
        let path = self.path(owner, name);
        tokio::task::spawn_blocking(move || {
            let _io = io;
            match fs::remove_file(&path) {
                Ok(()) => sync_directory(
                    path.parent()
                        .ok_or_else(|| error("local_storage_unavailable"))?,
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(error("local_storage_unavailable")),
            }
        })
        .await
        .map_err(|_| error("local_storage_unavailable"))?
    }

    pub(crate) async fn remember(
        &self,
        owner: &str,
        vault: &str,
        key_id: &str,
        key: &DerivedKey,
        remember: bool,
    ) -> SyncResult<()> {
        let account = self.binding(owner, vault, key_id)?;
        if remember {
            let bytes = key.copy_secret_bytes();
            self.credentials
                .write_sync_secret(
                    account,
                    SecretValue::new(URL_SAFE_NO_PAD.encode(&bytes[..])),
                )
                .await
                .map_err(|_| error("secure_storage_denied"))
        } else {
            self.credentials
                .remove_sync_secret(account)
                .await
                .map_err(|_| error("secure_storage_denied"))
        }
    }

    pub(crate) async fn remembered(
        &self,
        owner: &str,
        vault: &str,
        key_id: &str,
    ) -> SyncResult<Option<DerivedKey>> {
        self.credentials
            .read_sync_secret(self.binding(owner, vault, key_id)?)
            .await
            .map_err(|_| error("secure_storage_denied"))?
            .map(|value| decode_key(value.expose_secret()).map(DerivedKey::from_secret_bytes))
            .transpose()
    }

    pub(crate) async fn forget(&self, owner: &str, vault: &str, key_id: &str) -> SyncResult<()> {
        self.credentials
            .remove_sync_secret(self.binding(owner, vault, key_id)?)
            .await
            .map_err(|_| error("secure_storage_denied"))
    }

    /// A non-secret, durable refusal to auto-unlock survives even when encrypted
    /// recovery metadata or the operating-system vault cannot currently be read.
    pub(crate) async fn set_lockout(&self, locked: bool) -> SyncResult<()> {
        let io = self.io.clone().lock_owned().await;
        let path = self.root.join("unlock-disabled");
        tokio::task::spawn_blocking(move || {
            let _io = io;
            if locked {
                durable_replace(&path, b"locked-v1\n")
            } else {
                match fs::remove_file(&path) {
                    Ok(()) => sync_directory(
                        path.parent()
                            .ok_or_else(|| error("local_storage_unavailable"))?,
                    ),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(_) => Err(error("local_storage_unavailable")),
                }
            }
        })
        .await
        .map_err(|_| error("local_storage_unavailable"))?
    }

    pub(crate) async fn locked_out(&self) -> SyncResult<bool> {
        let io = self.io.clone().lock_owned().await;
        let path = self.root.join("unlock-disabled");
        tokio::task::spawn_blocking(move || {
            let _io = io;
            path.try_exists()
                .map_err(|_| error("local_storage_unavailable"))
        })
        .await
        .map_err(|_| error("local_storage_unavailable"))?
    }
}

fn commit_origin_migration(
    root: &Path,
    legacy_origin: &str,
    staged: Vec<(String, String, Vec<u8>)>,
) -> SyncResult<()> {
    let marker = root.join(".origin-migration-v1.json");
    let backup_dir = root.join(".origin-migration-v1");
    if marker.exists() || backup_dir.exists() {
        recover_origin_migration_sync(root)?;
    }
    if staged.len() > MAX_ORIGIN_MIGRATION_ENTRIES {
        return Err(error("recovery_required"));
    }
    fs::create_dir_all(&backup_dir).map_err(|_| error("local_storage_unavailable"))?;
    let mut entries = Vec::with_capacity(staged.len());
    for (index, (owner, name, _)) in staged.iter().enumerate() {
        let path = root.join(format!(
            "{:x}.enc",
            Sha256::digest(format!("{owner}\0{name}").as_bytes())
        ));
        let existed = path
            .try_exists()
            .map_err(|_| error("local_storage_unavailable"))?;
        if existed {
            let bytes = fs::read(&path).map_err(|_| error("local_storage_unavailable"))?;
            durable_replace(&backup_dir.join(format!("{index}.bak")), &bytes)?;
        }
        entries.push(OriginMigrationEntry {
            owner: owner.clone(),
            name: name.clone(),
            existed,
        });
    }
    let manifest = OriginMigrationManifest {
        version: ORIGIN_MIGRATION_VERSION,
        legacy_origin: Some(legacy_origin.to_owned()),
        entries,
    };
    let manifest_bytes =
        serde_json::to_vec(&manifest).map_err(|_| error("local_storage_unavailable"))?;
    durable_replace(&marker, &manifest_bytes)?;
    let completion_bytes = serde_json::to_vec(&OriginMigrationCompletion {
        version: ORIGIN_MIGRATION_VERSION,
        legacy_origin: legacy_origin.to_owned(),
        manifest_hash: migration_manifest_hash(&manifest_bytes),
    })
    .map_err(|_| error("local_storage_unavailable"))?;
    let result = (|| {
        for (owner, name, sealed) in staged {
            let path = root.join(format!(
                "{:x}.enc",
                Sha256::digest(format!("{owner}\0{name}").as_bytes())
            ));
            durable_replace(&path, &sealed)?;
        }
        durable_replace(
            &root.join(".origin-migration-complete-v1"),
            &completion_bytes,
        )?;
        Ok(())
    })();
    if result.is_err() {
        let _ = recover_origin_migration_sync(root);
        return result;
    }
    fs::remove_file(&marker).map_err(|_| error("local_storage_unavailable"))?;
    fs::remove_dir_all(&backup_dir).map_err(|_| error("local_storage_unavailable"))?;
    sync_directory(root)
}

fn recover_origin_migration_sync(root: &Path) -> SyncResult<()> {
    let marker = root.join(".origin-migration-v1.json");
    let backup_dir = root.join(".origin-migration-v1");
    if !marker
        .try_exists()
        .map_err(|_| error("local_storage_unavailable"))?
    {
        if backup_dir.exists() {
            fs::remove_dir_all(&backup_dir).map_err(|_| error("local_storage_unavailable"))?;
        }
        return Ok(());
    }
    let bytes = fs::read(&marker).map_err(|_| error("recovery_required"))?;
    let manifest: OriginMigrationManifest =
        serde_json::from_slice(&bytes).map_err(|_| error("recovery_required"))?;
    if manifest.version != ORIGIN_MIGRATION_VERSION
        || manifest.entries.len() > MAX_ORIGIN_MIGRATION_ENTRIES
    {
        return Err(error("recovery_required"));
    }
    let completion = match fs::read_to_string(root.join(".origin-migration-complete-v1")) {
        Ok(value) => serde_json::from_str::<OriginMigrationCompletion>(value.trim()).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(error("local_storage_unavailable")),
    };
    if completion.as_ref().is_some_and(|completion| {
        completion.version == ORIGIN_MIGRATION_VERSION
            && manifest
                .legacy_origin
                .as_deref()
                .is_some_and(|legacy_origin| completion.legacy_origin == legacy_origin)
            && completion.manifest_hash == migration_manifest_hash(&bytes)
    }) {
        fs::remove_file(&marker).map_err(|_| error("recovery_required"))?;
        sync_directory(root)?;
        match fs::remove_dir_all(&backup_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(error("recovery_required")),
        }
        return sync_directory(root);
    }
    if !backup_dir.is_dir() {
        return Err(error("recovery_required"));
    }
    for (index, entry) in manifest.entries.iter().enumerate() {
        let path = root.join(format!(
            "{:x}.enc",
            Sha256::digest(format!("{}\0{}", entry.owner, entry.name).as_bytes())
        ));
        if entry.existed {
            let backup = backup_dir.join(format!("{index}.bak"));
            if backup.is_file() {
                let bytes = fs::read(&backup).map_err(|_| error("recovery_required"))?;
                durable_replace(&path, &bytes)?;
            } else if !path.is_file() {
                return Err(error("recovery_required"));
            }
        } else {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(error("recovery_required")),
            }
        }
    }
    fs::remove_file(&marker).map_err(|_| error("recovery_required"))?;
    sync_directory(root)?;
    match fs::remove_dir_all(&backup_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(error("recovery_required")),
    }
    sync_directory(root)
}

fn decode_key(value: &str) -> SyncResult<Zeroizing<[u8; 32]>> {
    let bytes = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| error("secure_storage_denied"))?,
    );
    if URL_SAFE_NO_PAD.encode(&*bytes) != value {
        return Err(error("secure_storage_denied"));
    }
    let bytes =
        <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| error("secure_storage_denied"))?;
    Ok(Zeroizing::new(bytes))
}

pub(crate) fn durable_replace(path: &Path, bytes: &[u8]) -> SyncResult<()> {
    let parent = path.parent().ok_or_else(|| {
        log::error!(
            "[e2ee-local] durable_replace missing parent path={}",
            path.display()
        );
        error("local_storage_unavailable")
    })?;
    fs::create_dir_all(parent).map_err(|err| {
        log::error!(
            "[e2ee-local] durable_replace create_dir_all path={} err={err}",
            parent.display()
        );
        error("local_storage_unavailable")
    })?;
    let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|err| {
            log::error!(
                "[e2ee-local] durable_replace open tmp={} err={err}",
                temporary.display()
            );
            error("local_storage_unavailable")
        })?;
        file.write_all(bytes).map_err(|err| {
            log::error!(
                "[e2ee-local] durable_replace write tmp={} err={err}",
                temporary.display()
            );
            error("local_storage_unavailable")
        })?;
        file.sync_all().map_err(|err| {
            log::error!(
                "[e2ee-local] durable_replace sync tmp={} err={err}",
                temporary.display()
            );
            error("local_storage_unavailable")
        })?;
        replace_temporary(&temporary, path).map_err(|err| {
            log::error!(
                "[e2ee-local] durable_replace rename tmp={} dest={} err={err}",
                temporary.display(),
                path.display()
            );
            error("local_storage_unavailable")
        })?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(target_os = "windows")]
fn replace_temporary(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| std::io::Error::other(error.to_string()))
}

#[cfg(not(target_os = "windows"))]
fn replace_temporary(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

fn sync_directory(path: &Path) -> SyncResult<()> {
    #[cfg(unix)]
    {
        fs::File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|err| {
                log::error!(
                    "[e2ee-local] sync_directory path={} err={err}",
                    path.display()
                );
                error("local_storage_unavailable")
            })?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Publish a fully written identity without replacing an existing install's ID.
pub(crate) fn durable_create(path: &Path, bytes: &[u8]) -> SyncResult<bool> {
    let parent = path.parent().ok_or_else(|| {
        log::error!(
            "[e2ee-local] durable_create missing parent path={}",
            path.display()
        );
        error("local_storage_unavailable")
    })?;
    fs::create_dir_all(parent).map_err(|err| {
        log::error!(
            "[e2ee-local] durable_create create_dir_all path={} err={err}",
            parent.display()
        );
        error("local_storage_unavailable")
    })?;
    let temporary = parent.join(format!(".identity-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|err| {
            log::error!(
                "[e2ee-local] durable_create open tmp={} err={err}",
                temporary.display()
            );
            error("local_storage_unavailable")
        })?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|err| {
                log::error!(
                    "[e2ee-local] durable_create write/sync tmp={} err={err}",
                    temporary.display()
                );
                error("local_storage_unavailable")
            })?;
        match fs::hard_link(&temporary, path) {
            Ok(()) => {
                sync_directory(parent)?;
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(err) if hard_link_unsupported(&err) => {
                // Android app-private storage often denies link(2) (EPERM). Fall back to
                // O_EXCL create of the final path so we still never replace an existing ID.
                log::warn!(
                    "[e2ee-local] durable_create hard_link unsupported tmp={} dest={} kind={:?} err={err}; falling back to exclusive create",
                    temporary.display(),
                    path.display(),
                    err.kind()
                );
                if exclusive_create(path, bytes)? {
                    sync_directory(parent)?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Err(err) => {
                log::error!(
                    "[e2ee-local] durable_create hard_link tmp={} dest={} kind={:?} err={err}",
                    temporary.display(),
                    path.display(),
                    err.kind()
                );
                Err(error("local_storage_unavailable"))
            }
        }
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn hard_link_unsupported(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::Unsupported | std::io::ErrorKind::PermissionDenied
    ) || matches!(
        err.raw_os_error(),
        Some(1 /* EPERM */) | Some(95 /* EOPNOTSUPP/ENOTSUP */)
    )
}

fn exclusive_create(path: &Path, bytes: &[u8]) -> SyncResult<bool> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|err| {
                log::error!(
                    "[e2ee-local] exclusive_create write path={} err={err}",
                    path.display()
                );
                let _ = fs::remove_file(path);
                error("local_storage_unavailable")
            })
            .map(|()| true),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => {
            log::error!(
                "[e2ee-local] exclusive_create open path={} err={err}",
                path.display()
            );
            Err(error("local_storage_unavailable"))
        }
    }
}

fn log_local_failure(stage: &'static str, failure: &crate::BackendError) {
    let code = match failure
        .details
        .as_ref()
        .and_then(|details| details.get("reason"))
        .and_then(serde_json::Value::as_str)
    {
        Some("secure_storage_denied") => "secure_storage_denied",
        Some("recovery_required") => "recovery_required",
        Some("secure_random_unavailable") => "secure_random_unavailable",
        _ => "local_storage_unavailable",
    };
    log::warn!("[e2ee-local] stage={stage} code={code}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_replace_updates_existing_file() {
        let root =
            std::env::temp_dir().join(format!("openless-durable-replace-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("record.enc");

        durable_replace(&path, b"first").unwrap();
        durable_replace(&path, b"second").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"second");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn origin_only_completion_marker_does_not_acknowledge_a_new_manifest() {
        let root =
            std::env::temp_dir().join(format!("openless-origin-recovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let owner = "owner";
        let name = "baseline";
        let path = root.join(format!(
            "{:x}.enc",
            Sha256::digest(format!("{owner}\0{name}").as_bytes())
        ));
        let backup_dir = root.join(".origin-migration-v1");
        fs::create_dir_all(&backup_dir).unwrap();
        fs::write(&path, b"partially replaced").unwrap();
        fs::write(backup_dir.join("0.bak"), b"original").unwrap();
        let manifest = OriginMigrationManifest {
            version: ORIGIN_MIGRATION_VERSION,
            legacy_origin: Some("https://legacy.example".into()),
            entries: vec![OriginMigrationEntry {
                owner: owner.into(),
                name: name.into(),
                existed: true,
            }],
        };
        fs::write(
            root.join(".origin-migration-v1.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join(".origin-migration-complete-v1"),
            b"https://legacy.example\n",
        )
        .unwrap();

        recover_origin_migration_sync(&root).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert!(!root.join(".origin-migration-v1.json").exists());
        assert!(!root.join(".origin-migration-v1").exists());
        let _ = fs::remove_dir_all(root);
    }
}
