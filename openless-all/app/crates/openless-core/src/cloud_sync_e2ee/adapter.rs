use std::{path::Path, sync::Arc};

use futures_util::future::BoxFuture;
use sha2::{Digest, Sha256};

use crate::cloud_sync_e2ee_documents::{
    CryptoJournalProtector, DocumentError, DocumentResult, ExportedDocuments, JournalProtector,
    RestoreContext, SecretJson, SyncScope, ValidatedSyncDocuments,
};
use crate::cloud_sync_e2ee_protocol::types::{DocumentSet, Revision, SourceDevice};
use crate::cloud_sync_e2ee_store::{
    extensions::{DeviceExtensionKey, ProtectedExtensionStore},
    gate::SyncChange,
    CoreSyncStore,
};

use super::{document_error, local::LocalStorage, service::SyncServiceData, SyncResult};

// Composition boundary: keep the existing injected services explicit rather than
// adding a second dependency bundle solely for this constructor.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build(
    config: super::EncryptedSyncConfig,
    data_dir: &Path,
    repositories: crate::BackendRepositories,
    credentials: Arc<dyn crate::CredentialStore>,
    marketplace: Arc<crate::marketplace::MarketplaceService>,
    github_client_id: String,
    events: crate::events::BackendEventPublisher,
    tasks: Arc<dyn crate::TaskSpawner>,
) -> SyncResult<(super::EncryptedSyncService, Arc<CoreSyncStore>)> {
    let origin =
        crate::cloud_sync_e2ee_protocol::transport::canonicalize_origin(&config.service_origin)
            .map_err(|_| super::error("unsupported_protocol"))?;
    if data_dir.as_os_str().is_empty() {
        return Err(super::error("unsupported_protocol"));
    }
    let root = data_dir.join("encrypted-sync");
    let configured_legacy_origin = repositories
        .preferences
        .get()
        .sync_custom_server_origin
        .and_then(|legacy| {
            crate::cloud_sync_e2ee_protocol::transport::canonicalize_origin(&legacy).ok()
        })
        .filter(|legacy| legacy != &origin);
    // Before the stable local identity was introduced, the local origin was fixed
    // at construction while the custom-server preference could change at runtime.
    // Probe both candidates during startup; the ciphertext, not the mutable
    // preference, identifies which origin protected the existing records.
    let legacy_origins = configured_legacy_origin
        .clone()
        .map(|legacy| {
            let mut origins = vec![legacy];
            if origins[0] != origin {
                origins.push(origin.clone());
            }
            origins
        })
        .unwrap_or_default();
    log::error!(
        "[e2ee-adapter] build start data_dir={} root={}",
        data_dir.display(),
        root.display()
    );
    std::fs::create_dir_all(&root).map_err(|error| {
        log::error!(
            "[e2ee-adapter] create encrypted-sync root failed path={} err={error}",
            root.display()
        );
        super::error("local_storage_unavailable")
    })?;
    log::error!("[e2ee-adapter] stage=gate_open");
    let gate =
        crate::cloud_sync_e2ee_store::gate::open_for_data_dir(data_dir).map_err(|error| {
            log::error!("[e2ee-adapter] reopen sync write gate failed: {error:#}");
            document_error(error)
        })?;
    log::error!("[e2ee-adapter] stage=device_id");
    let device_id = load_device_id(&root)?;
    log::error!(
        "[e2ee-adapter] stage=device_id_ok id_len={}",
        device_id.len()
    );
    let device = SourceDevice {
        id: device_id.clone(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        app_version: config.app_version,
    };
    let local_root = root.join("protected");
    let local = LocalStorage::new(
        local_root.clone(),
        origin.clone(),
        device_id.clone(),
        credentials.clone(),
    );
    let mut legacy_local = legacy_origins
        .iter()
        .cloned()
        .map(|legacy_origin| {
            LocalStorage::new(
                local_root.clone(),
                legacy_origin,
                device_id.clone(),
                credentials.clone(),
            )
        })
        .collect::<Vec<_>>();
    if has_legacy_root_ciphertext(&root)? {
        let mut legacy_root_origins = legacy_origins;
        if !legacy_root_origins
            .iter()
            .any(|candidate| candidate == &origin)
        {
            legacy_root_origins.push(origin.clone());
        }
        legacy_local.extend(legacy_root_origins.into_iter().map(|legacy_origin| {
            LocalStorage::new(
                root.clone(),
                legacy_origin,
                device_id.clone(),
                credentials.clone(),
            )
        }));
    }
    log::error!("[e2ee-adapter] stage=store_new");
    let credential_store_for_service = credentials.clone();
    let store = Arc::new(
        CoreSyncStore::new(
            repositories,
            credentials,
            data_dir.into(),
            device,
            gate,
            Arc::new(local.clone()),
            tasks,
        )
        .map_err(|error| {
            log::error!("[e2ee-adapter] CoreSyncStore::new failed: {error:#}");
            document_error(error)
        })?,
    );
    log::error!("[e2ee-adapter] stage=store_ok");
    let service = super::EncryptedSyncService::new_with_legacy(
        super::SyncServiceConfig {
            origin,
            github_client_id,
        },
        marketplace,
        local,
        legacy_local,
        store.clone(),
        credential_store_for_service,
        events,
    );
    Ok((service, store))
}

pub(crate) fn load_device_id(root: &Path) -> SyncResult<String> {
    let device_path = root.join("device-id");
    let device_id = match std::fs::read_to_string(&device_path) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Never invent another keyring/AAD binding for existing ciphertext.
            if has_legacy_root_ciphertext(root)? {
                log::error!(
                    "[e2ee-adapter] device-id missing but legacy root ciphertext is present under {}; recovery required",
                    root.display()
                );
                return Err(super::error("recovery_required"));
            }
            for entry in std::fs::read_dir(root).map_err(|error| {
                log::error!(
                    "[e2ee-adapter] read encrypted-sync root failed path={} err={error}",
                    root.display()
                );
                super::error("local_storage_unavailable")
            })? {
                let entry = entry.map_err(|error| {
                    log::error!(
                        "[e2ee-adapter] read encrypted-sync entry failed path={} err={error}",
                        root.display()
                    );
                    super::error("local_storage_unavailable")
                })?;
                let path = entry.path();
                if path.is_dir()
                    && std::fs::read_dir(&path)
                        .map_err(|error| {
                            log::error!(
                                "[e2ee-adapter] inspect protected dir failed path={} err={error}",
                                path.display()
                            );
                            super::error("local_storage_unavailable")
                        })?
                        .next()
                        .is_some()
                {
                    log::error!(
                        "[e2ee-adapter] device-id missing but ciphertext present under {}; recovery required",
                        path.display()
                    );
                    return Err(super::error("recovery_required"));
                }
            }
            let id = uuid::Uuid::new_v4().to_string();
            log::error!(
                "[e2ee-adapter] device-id missing; durable_create path={}",
                device_path.display()
            );
            if super::local::durable_create(&device_path, id.as_bytes()).map_err(|error| {
                log::error!(
                    "[e2ee-adapter] durable_create device-id failed path={} err={error:#}",
                    device_path.display()
                );
                error
            })? {
                id
            } else {
                std::fs::read_to_string(&device_path).map_err(|error| {
                    log::error!(
                        "[e2ee-adapter] reread raced device-id failed path={} err={error}",
                        device_path.display()
                    );
                    super::error("local_storage_unavailable")
                })?
            }
        }
        Err(error) => {
            log::error!(
                "[e2ee-adapter] read device-id failed path={} err={error}",
                device_path.display()
            );
            return Err(super::error("local_storage_unavailable"));
        }
    };
    crate::cloud_sync_e2ee_protocol::types::UuidV4::parse(&device_id).map_err(|error| {
        log::error!(
            "[e2ee-adapter] device-id is not a UuidV4 path={} err={error}",
            device_path.display()
        );
        super::error("recovery_required")
    })?;
    Ok(device_id)
}

fn has_legacy_root_ciphertext(root: &Path) -> SyncResult<bool> {
    let entries = std::fs::read_dir(root).map_err(|error| {
        log::error!(
            "[e2ee-adapter] inspect legacy encrypted-sync root failed path={} err={error}",
            root.display()
        );
        super::error("local_storage_unavailable")
    })?;
    for entry in entries {
        let path = entry
            .map_err(|_| super::error("local_storage_unavailable"))?
            .path();
        if path.is_file() && path.extension().is_some_and(|extension| extension == "enc") {
            return Ok(true);
        }
    }
    Ok(false)
}

impl ProtectedExtensionStore for LocalStorage {
    fn read_scope(&self, scope: SyncScope) -> BoxFuture<'_, DocumentResult<Option<SecretJson>>> {
        Box::pin(async move {
            self.read(&scope_bucket(&scope)?, "extensions")
                .await
                .map_err(|_| DocumentError::JournalUnavailable)
        })
    }

    fn write_scope(
        &self,
        scope: SyncScope,
        value: SecretJson,
    ) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move {
            self.write(&scope_bucket(&scope)?, "extensions", value)
                .await
                .map_err(|_| DocumentError::JournalUnavailable)
        })
    }

    fn read_device(
        &self,
        key: DeviceExtensionKey,
    ) -> BoxFuture<'_, DocumentResult<Option<SecretJson>>> {
        Box::pin(async move {
            if key == DeviceExtensionKey::UiPreferences {
                self.read_ui()
                    .await
                    .map(|value| value.map(|envelope| envelope.value))
                    .map_err(|_| DocumentError::JournalUnavailable)
            } else {
                self.read("device", key.storage_name())
                    .await
                    .map_err(|_| DocumentError::JournalUnavailable)
            }
        })
    }

    fn write_device(
        &self,
        key: DeviceExtensionKey,
        value: SecretJson,
    ) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move {
            if key == DeviceExtensionKey::UiPreferences {
                let envelope = super::local::UiEnvelope {
                    schema_version: 1,
                    revision: uuid::Uuid::new_v4().to_string(),
                    value,
                };
                self.write("device", key.storage_name(), envelope)
                    .await
                    .map_err(|_| DocumentError::JournalUnavailable)
            } else {
                self.write("device", key.storage_name(), value)
                    .await
                    .map_err(|_| DocumentError::JournalUnavailable)
            }
        })
    }

    fn read_ui_revision(&self) -> BoxFuture<'_, DocumentResult<Option<String>>> {
        Box::pin(async move {
            self.read_ui()
                .await
                .map(|value| value.map(|envelope| envelope.revision))
                .map_err(|_| DocumentError::JournalUnavailable)
        })
    }

    fn journal_protector(&self) -> BoxFuture<'_, DocumentResult<Arc<dyn JournalProtector>>> {
        Box::pin(async move {
            let key = self.key().await.map_err(|_| DocumentError::Locked)?;
            Ok(Arc::new(CryptoJournalProtector::new(key)) as Arc<dyn JournalProtector>)
        })
    }
}

fn scope_bucket(scope: &SyncScope) -> DocumentResult<String> {
    // Changing the cloud encryption password keeps the same locally protected
    // baseline/tombstone bucket; account or vault changes never share it.
    let bytes = serde_json::to_vec(&[
        &scope.service_origin,
        &scope.owner_github_id,
        &scope.vault_id,
        &scope.device_id,
    ])
    .map_err(|_| DocumentError::InvalidDocument)?;
    Ok(format!("scope:{:x}", Sha256::digest(bytes)))
}

impl SyncServiceData for CoreSyncStore {
    fn export(&self, scope: SyncScope) -> BoxFuture<'_, SyncResult<ExportedDocuments>> {
        Box::pin(async move { self.export_scope(scope).await.map_err(document_error) })
    }
    fn restore(
        &self,
        desired: ValidatedSyncDocuments,
        context: RestoreContext,
    ) -> BoxFuture<'_, SyncResult<ExportedDocuments>> {
        Box::pin(async move {
            self.restore_scope(desired, context)
                .await
                .map_err(document_error)
        })
    }
    fn recover(&self) -> BoxFuture<'_, SyncResult<()>> {
        Box::pin(async move { self.recover_registered().await.map_err(document_error) })
    }
    fn migrate_legacy_origin(
        &self,
        legacy: Arc<dyn crate::cloud_sync_e2ee_store::ProtectedExtensionStore>,
        legacy_origin: String,
        stable_origin: String,
        current_scope: Option<SyncScope>,
    ) -> BoxFuture<'_, SyncResult<()>> {
        Box::pin(async move {
            self.migrate_legacy_origin(legacy, &legacy_origin, &stable_origin, current_scope)
                .await
                .map_err(document_error)
        })
    }
    fn baseline(
        &self,
        scope: SyncScope,
        documents: DocumentSet,
        revision: Revision,
    ) -> BoxFuture<'_, SyncResult<()>> {
        Box::pin(async move {
            self.record_baseline(scope, documents, revision)
                .await
                .map_err(document_error)
        })
    }
    fn generation(&self) -> SyncResult<Revision> {
        CoreSyncStore::generation(self).map_err(document_error)
    }
    fn device(&self) -> SourceDevice {
        CoreSyncStore::device(self)
    }
    fn recovery_scopes(&self) -> SyncResult<Vec<SyncScope>> {
        CoreSyncStore::recovery_scopes(self).map_err(document_error)
    }
    fn custom_server_origin(&self) -> Option<String> {
        CoreSyncStore::custom_server_origin(self)
    }
    fn changes(&self) -> tokio::sync::watch::Receiver<SyncChange> {
        CoreSyncStore::changes(self)
    }
}
