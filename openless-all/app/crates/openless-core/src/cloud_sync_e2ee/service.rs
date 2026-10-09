#[path = "setup_prompt.rs"]
mod setup_prompt;

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::cloud_sync_e2ee_documents::{
    self as documents, ExportedDocuments, RestoreContext, SyncScope, ValidatedSyncDocuments,
};
use crate::cloud_sync_e2ee_protocol::{
    crypto::{self, DerivedKey, EncryptContext, NormalizedPassword},
    transport::{Metadata, MetadataResult, OperationStatus, ProxyPolicy, SyncSession, Transport},
    types::*,
};
use crate::credentials::{CredentialKey, CredentialNamespace, CLOUD_SYNC_CUSTOM_TOKEN_ACCOUNT};
use crate::events::{BackendEventKind, BackendEventPublisher};
use crate::marketplace::MarketplaceService;
use crate::{BackendError, CredentialStore, SecretValue};

use super::{
    document_error,
    dto::*,
    error,
    local::{ClientSettings, KnownRemoteAccount, LocalMigrationRecord, LocalStorage},
    protocol_error, PersistedCustomServerConfig, SyncResult, CUSTOM_SYNC_CONFIG_VERSION,
};

/// The repository adapter performs the protected, exclusive restore transaction
/// and verifies actual read-back before returning. It never runs UI callbacks.
pub(crate) trait SyncServiceData: Send + Sync {
    fn export(&self, scope: SyncScope) -> BoxFuture<'_, SyncResult<ExportedDocuments>>;
    fn restore(
        &self,
        desired: ValidatedSyncDocuments,
        context: RestoreContext,
    ) -> BoxFuture<'_, SyncResult<ExportedDocuments>>;
    fn recover(&self) -> BoxFuture<'_, SyncResult<()>>;
    fn migrate_legacy_origin(
        &self,
        _legacy: Arc<dyn crate::cloud_sync_e2ee_store::ProtectedExtensionStore>,
        _legacy_origin: String,
        _stable_origin: String,
        _current_scope: Option<SyncScope>,
    ) -> BoxFuture<'_, SyncResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn baseline(
        &self,
        scope: SyncScope,
        documents: DocumentSet,
        revision: Revision,
    ) -> BoxFuture<'_, SyncResult<()>>;
    fn generation(&self) -> SyncResult<Revision>;
    fn device(&self) -> SourceDevice;
    fn recovery_scopes(&self) -> SyncResult<Vec<SyncScope>> {
        Ok(Vec::new())
    }
    /// Live read of the user's self-hosted server preference. A Settings-page
    /// save must take effect on the next connection attempt, never only after
    /// a full process restart — this is the one thing `SyncServiceConfig.origin`
    /// (a fixed snapshot from backend construction) cannot provide by itself.
    fn custom_server_origin(&self) -> Option<String>;
    fn changes(
        &self,
    ) -> tokio::sync::watch::Receiver<crate::cloud_sync_e2ee_store::gate::SyncChange>;
}

pub(crate) struct SyncServiceConfig {
    pub origin: String,
    pub github_client_id: String,
}

/// How the current `Connection` authenticated. Re-validated on every use so a
/// credential change (GitHub sign-out, or the custom token being edited) is
/// detected the same way `current_account` already detects a GitHub account
/// switch — never trust a cached `Connection` past a credential mismatch.
enum ConnectionCredential {
    Github(SecretValue),
    CustomToken(SecretValue),
}

struct CustomSyncTarget {
    origin: String,
    token: SecretValue,
}

struct Connection {
    transport: Transport,
    session: SyncSession,
    credential: ConnectionCredential,
    expires_at: Instant,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Baseline {
    revision: Revision,
    vault_id: UuidV4,
    documents: DocumentSet,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pending {
    record: String,
    operation_id: String,
    vault_id: String,
    key_id: Option<String>,
    local_generation: Revision,
    deleted: bool,
    remember_key: bool,
    key_preference_epoch: u64,
    base_revision: Revision,
    old_key_id: Option<String>,
}

const DEVICE_RECORD_NAMES: [&str; 4] = [
    "client",
    "sync-ui-preferences",
    "sync-window-positions",
    "sync-recovery-pointers",
];

async fn remember_migrated_key(
    legacy: &LocalStorage,
    remembered: &mut Vec<(String, String, String, DerivedKey)>,
    owner: &str,
    vault: &str,
    key_id: Option<&str>,
) -> SyncResult<()> {
    let Some(key_id) = key_id else {
        return Ok(());
    };
    if remembered
        .iter()
        .any(|(stored_owner, stored_vault, stored_key_id, _)| {
            stored_owner == owner && stored_vault == vault && stored_key_id == key_id
        })
    {
        return Ok(());
    }
    if let Some(key) = legacy.remembered(owner, vault, key_id).await? {
        remembered.push((owner.to_owned(), vault.to_owned(), key_id.to_owned(), key));
    }
    Ok(())
}

struct PreviewState {
    public: RestorePreview,
    remote: ValidatedSyncDocuments,
    local: ExportedDocuments,
    baseline: Option<ValidatedSyncDocuments>,
    vault_id: String,
    key_id: String,
}

struct Runtime {
    initialized: bool,
    settings: ClientSettings,
    connection: Option<Connection>,
    metadata: Option<Metadata>,
    snapshot: Option<SnapshotUpload>,
    key: Option<Arc<DerivedKey>>,
    baseline: Option<Baseline>,
    preview: Option<PreviewState>,
    retry_at: Option<Instant>,
}

struct Shared {
    config: SyncServiceConfig,
    marketplace: Arc<MarketplaceService>,
    data: Arc<dyn SyncServiceData>,
    local: LocalStorage,
    legacy_local: Vec<LocalStorage>,
    credential_store: Arc<dyn CredentialStore>,
    events: BackendEventPublisher,
    runtime: tokio::sync::Mutex<Runtime>,
    status: Mutex<EncryptedSyncStatus>,
    cancelled: Arc<AtomicBool>,
    auto_started: AtomicBool,
    shutdown: AtomicBool,
    auto_suspended: AtomicBool,
    signing_out: AtomicBool,
    sequence: AtomicU64,
    wake: tokio::sync::Notify,
    #[cfg(test)]
    runtime_release_count: AtomicU64,
}

struct SignOutGuard<'a>(&'a AtomicBool);

impl Drop for SignOutGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Clone)]
pub(crate) struct EncryptedSyncService(Arc<Shared>);

impl EncryptedSyncService {
    #[cfg(test)]
    pub(crate) fn new(
        config: SyncServiceConfig,
        marketplace: Arc<MarketplaceService>,
        local: LocalStorage,
        data: Arc<dyn SyncServiceData>,
        credential_store: Arc<dyn CredentialStore>,
        events: BackendEventPublisher,
    ) -> Self {
        Self::new_with_legacy(
            config,
            marketplace,
            local,
            Vec::new(),
            data,
            credential_store,
            events,
        )
    }

    pub(crate) fn new_with_legacy(
        config: SyncServiceConfig,
        marketplace: Arc<MarketplaceService>,
        local: LocalStorage,
        legacy_local: Vec<LocalStorage>,
        data: Arc<dyn SyncServiceData>,
        credential_store: Arc<dyn CredentialStore>,
        events: BackendEventPublisher,
    ) -> Self {
        let status = EncryptedSyncStatus::initial(config.origin.clone());
        Self(Arc::new(Shared {
            config,
            marketplace,
            data,
            local,
            legacy_local,
            credential_store,
            events,
            runtime: tokio::sync::Mutex::new(Runtime {
                initialized: false,
                settings: ClientSettings::default(),
                connection: None,
                metadata: None,
                snapshot: None,
                key: None,
                baseline: None,
                preview: None,
                retry_at: None,
            }),
            status: Mutex::new(status),
            cancelled: Arc::new(AtomicBool::new(false)),
            auto_started: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            auto_suspended: AtomicBool::new(false),
            signing_out: AtomicBool::new(false),
            sequence: AtomicU64::new(0),
            wake: tokio::sync::Notify::new(),
            #[cfg(test)]
            runtime_release_count: AtomicU64::new(0),
        }))
    }

    /// The local encrypted-storage identity is deliberately stable. Custom
    /// servers only affect the transport target, never local AAD or key scope.
    fn origin(&self) -> String {
        self.0.config.origin.clone()
    }

    #[cfg(test)]
    pub(crate) fn runtime_release_count_for_test(&self) -> u64 {
        self.0.runtime_release_count.load(Ordering::Acquire)
    }

    pub(crate) fn status(&self) -> EncryptedSyncStatus {
        let mut value = self
            .0
            .status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        value.sequence = self.0.sequence.load(Ordering::Acquire).to_string();
        match self.0.data.generation() {
            Ok(generation) => value.local_generation = generation.as_str().into(),
            Err(_) => {
                value.recovery_required = true;
                value.sync_state = SyncState::RecoveryRequired;
            }
        }
        value
    }

    pub(crate) async fn custom_server_config(
        &self,
    ) -> SyncResult<Option<super::EncryptedSyncCustomServerConfig>> {
        self.read_custom_target().await.map(|target| {
            target.map(|target| super::EncryptedSyncCustomServerConfig {
                origin: target.origin,
                has_token: true,
            })
        })
    }

    pub(crate) async fn set_custom_server_config(
        &self,
        origin: String,
        token: Option<String>,
    ) -> SyncResult<Option<super::EncryptedSyncCustomServerConfig>> {
        let mut runtime = self.0.runtime.lock().await;
        let origin = origin.trim();
        let token = token.as_deref().map(str::trim).unwrap_or_default();
        let key = Self::custom_token_key()?;
        if origin.is_empty() && token.is_empty() {
            self.0
                .credential_store
                .remove(key)
                .await
                .map_err(|_| error("secure_storage_denied"))?;
            Self::invalidate_connection(&mut runtime);
            self.update(|status| status.service_origin = self.origin());
            return Ok(None);
        }
        if origin.is_empty() {
            return Err(error("unsupported_protocol"));
        }
        let origin = crate::cloud_sync_e2ee_protocol::transport::canonicalize_origin(origin)
            .map_err(|_| error("unsupported_protocol"))?;
        let token = if token.is_empty() {
            let target = self
                .read_custom_target()
                .await?
                .filter(|target| !target.token.expose_secret().trim().is_empty())
                .ok_or_else(|| error("unsupported_protocol"))?;
            if target.origin != origin {
                return Err(error("unsupported_protocol"));
            }
            target.token.expose_secret().to_owned()
        } else {
            token.to_owned()
        };
        let record = PersistedCustomServerConfig {
            version: CUSTOM_SYNC_CONFIG_VERSION,
            origin: origin.clone(),
            token,
        };
        let value = serde_json::to_string(&record).map_err(|_| error("service_unavailable"))?;
        self.0
            .credential_store
            .write(key, SecretValue::new(value))
            .await
            .map_err(|_| error("secure_storage_denied"))?;
        Self::invalidate_connection(&mut runtime);
        self.update(|status| status.service_origin = origin.clone());
        Ok(Some(super::EncryptedSyncCustomServerConfig {
            origin,
            has_token: true,
        }))
    }

    pub(crate) async fn clear_custom_server_config(&self) -> SyncResult<()> {
        self.0
            .credential_store
            .remove(Self::custom_token_key()?)
            .await
            .map_err(|_| error("secure_storage_denied"))?;
        self.update(|status| status.service_origin = self.origin());
        Ok(())
    }

    fn custom_token_key() -> SyncResult<CredentialKey> {
        CredentialKey::new(
            CredentialNamespace::Application,
            None,
            CLOUD_SYNC_CUSTOM_TOKEN_ACCOUNT,
        )
        .map_err(|_| error("service_unavailable"))
    }

    fn invalidate_connection(runtime: &mut Runtime) {
        runtime.connection = None;
        runtime.metadata = None;
        runtime.snapshot = None;
        runtime.key = None;
        runtime.baseline = None;
        runtime.preview = None;
    }

    async fn read_custom_target(&self) -> SyncResult<Option<CustomSyncTarget>> {
        let key = Self::custom_token_key()?;
        let value = self
            .0
            .credential_store
            .read(key)
            .await
            .map_err(|_| error("secure_storage_denied"))?;
        let Some(value) = value else {
            return Ok(None);
        };
        let raw = value.expose_secret();
        if let Ok(record) = serde_json::from_str::<PersistedCustomServerConfig>(raw) {
            if record.version != CUSTOM_SYNC_CONFIG_VERSION
                || record.origin.trim().is_empty()
                || record.token.trim().is_empty()
            {
                return Err(error("unsupported_protocol"));
            }
            let origin =
                crate::cloud_sync_e2ee_protocol::transport::canonicalize_origin(&record.origin)
                    .map_err(|_| error("unsupported_protocol"))?;
            return Ok(Some(CustomSyncTarget {
                origin,
                token: SecretValue::new(record.token),
            }));
        }

        // PR #1133 stored a raw token separately from the preference. Read it
        // only as a compatibility input; never combine it with an invalid
        // origin or fall back to GitHub at that origin.
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            return Err(error("unsupported_protocol"));
        }
        let origin = self
            .0
            .data
            .custom_server_origin()
            .filter(|origin| !origin.trim().is_empty())
            .ok_or_else(|| error("unsupported_protocol"))?;
        let origin = crate::cloud_sync_e2ee_protocol::transport::canonicalize_origin(&origin)
            .map_err(|_| error("unsupported_protocol"))?;
        Ok(Some(CustomSyncTarget {
            origin,
            token: SecretValue::new(raw.to_owned()),
        }))
    }

    async fn configured_origin_has_owner_records(
        &self,
        candidates: &[LocalStorage],
        origin: &str,
        settings: Option<&ClientSettings>,
    ) -> bool {
        let mut owners = settings
            .into_iter()
            .flat_map(|settings| {
                settings
                    .known_owner_ids
                    .iter()
                    .cloned()
                    .chain(
                        settings
                            .known_accounts
                            .iter()
                            .map(|account| account.owner_id.clone()),
                    )
                    .chain(settings.owner_id.iter().cloned())
            })
            .collect::<Vec<_>>();
        owners.sort();
        owners.dedup();
        for candidate in candidates
            .iter()
            .filter(|candidate| candidate.origin() == origin)
        {
            for owner in &owners {
                for name in ["baseline", "pending", "last-seen-revision"] {
                    if candidate
                        .read::<serde_json::Value>(owner, name)
                        .await
                        .is_ok_and(|value| value.is_some())
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
    async fn migration_candidates(&self) -> SyncResult<Vec<LocalStorage>> {
        let mut candidates = vec![self.0.local.clone()];
        candidates.extend(self.0.legacy_local.clone());
        if let Some(target) = self.read_custom_target().await? {
            if !candidates
                .iter()
                .any(|candidate| candidate.origin() == target.origin)
            {
                candidates.push(self.0.local.with_origin(target.origin));
            }
        }
        Ok(candidates)
    }

    async fn read_custom_token(&self) -> SyncResult<Option<SecretValue>> {
        Ok(self.read_custom_target().await?.map(|target| target.token))
    }

    fn update(&self, update: impl FnOnce(&mut EncryptedSyncStatus)) {
        let status = {
            let mut status = self.0.status.lock().unwrap_or_else(|e| e.into_inner());
            update(&mut status);
            status.sequence = self
                .0
                .sequence
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1)
                .to_string();
            status.clone()
        };
        self.0.events.publish(
            None,
            BackendEventKind::CloudSyncStateChanged(EncryptedSyncEvent {
                sequence: status.sequence.clone(),
                account_id: status.account.as_ref().map(|v| v.github_id.clone()),
                vault_id: status.vault_id.clone(),
                task_id: status.task_id.clone(),
                status,
            }),
        );
    }

    fn begin(&self) {
        if self.0.signing_out.load(Ordering::Acquire) {
            // Sign-out owns the cancellation window across secure-store awaits.
            // A concurrent manual trigger may acquire the runtime mutex, but it
            // must not clear cancellation and start remote work.
            self.0.cancelled.store(true, Ordering::Release);
            return;
        }
        self.update(|s| {
            self.0.cancelled.store(false, Ordering::Release);
            s.task_id = Some(uuid::Uuid::new_v4().to_string());
            s.sync_state = SyncState::Syncing;
            s.last_error = None;
        });
    }

    fn check_cancelled(&self) -> SyncResult<()> {
        if self.0.cancelled.load(Ordering::Acquire) || self.0.shutdown.load(Ordering::Acquire) {
            Err(error("cancelled"))
        } else {
            Ok(())
        }
    }

    async fn current_account(&self, runtime: &Runtime) -> SyncResult<()> {
        self.check_cancelled()?;
        let connection = runtime
            .connection
            .as_ref()
            .ok_or_else(|| error("sign_in_required"))?;
        let unchanged = match &connection.credential {
            ConnectionCredential::Github(token) => {
                self.0
                    .marketplace
                    .read_access_token()
                    .await
                    .map_err(|_| error("sign_in_required"))?
                    == *token
            }
            ConnectionCredential::CustomToken(token) => {
                self.read_custom_token().await?.as_ref() == Some(token)
            }
        };
        if !unchanged {
            return Err(error("account_changed"));
        }
        self.check_cancelled()
    }

    fn finish<T>(&self, runtime: &mut Runtime, result: &SyncResult<T>) {
        let failure = result.as_ref().err().map(|e| {
            let code = e
                .details
                .as_ref()
                .and_then(|v| v.get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("sync_failed")
                .to_owned();
            let retry_after_seconds = e
                .details
                .as_ref()
                .and_then(|v| v.get("retryAfterSeconds"))
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok());
            SyncFailure {
                code,
                retry_after_seconds,
            }
        });
        if let Some(seconds) = failure.as_ref().and_then(|v| v.retry_after_seconds) {
            runtime.retry_at = Some(Instant::now() + Duration::from_secs(u64::from(seconds)));
        }
        self.update(|s| {
            if let Ok(generation) = self.0.data.generation() {
                s.local_generation = generation.as_str().into();
            }
            s.task_id = None;
            s.enabled = runtime.settings.enabled;
            s.key_state = if runtime.key.is_some() {
                KeyState::Unlocked
            } else {
                KeyState::Locked
            };
            s.consent_version = runtime.settings.consent_version.clone();
            s.last_successful_sync_at = runtime.settings.last_success.clone();
            s.last_synced_local_generation = runtime.settings.last_generation.clone();
            if let Some(failure) = failure {
                s.sync_state = match failure.code.as_str() {
                    "sign_in_required" | "account_changed" => {
                        s.auth_state = AuthState::Expired;
                        SyncState::SignInRequired
                    }
                    "unlock_required" | "invalid_password_or_ciphertext" => {
                        SyncState::UnlockRequired
                    }
                    "outcome_unknown" => SyncState::OutcomeUnknown,
                    "recovery_required" => {
                        s.recovery_required = true;
                        SyncState::RecoveryRequired
                    }
                    "conflict" | "restore_review_required" => SyncState::Conflict,
                    "cancelled" => {
                        if s.enabled {
                            SyncState::Pending
                        } else {
                            SyncState::Disabled
                        }
                    }
                    _ => SyncState::Failed,
                };
                s.last_error = Some(failure);
            } else {
                s.last_error = None;
                s.sync_state = if runtime.preview.is_some() {
                    SyncState::Conflict
                } else if !s.enabled {
                    SyncState::Disabled
                } else if s.auth_state != AuthState::SignedIn {
                    SyncState::SignInRequired
                } else if runtime.key.is_none() {
                    SyncState::UnlockRequired
                } else if s.pending_operation_id.is_some() {
                    SyncState::OutcomeUnknown
                } else if s.last_synced_local_generation.as_deref()
                    != Some(s.local_generation.as_str())
                {
                    SyncState::Pending
                } else {
                    SyncState::Ready
                };
            }
        });
    }

    async fn migrate_legacy_state_from_stable_candidates(
        &self,
        candidates: &[LocalStorage],
        settings: Option<&ClientSettings>,
        scope_origin: &str,
        stable_origin: &str,
    ) -> SyncResult<()> {
        let recovery_scopes = self.0.data.recovery_scopes()?;
        let mut scopes = recovery_scopes.clone();
        let mut owners = recovery_scopes
            .iter()
            .map(|scope| scope.owner_github_id.clone())
            .collect::<Vec<_>>();
        if let Some(settings) = settings {
            owners.extend(settings.known_owner_ids.iter().cloned());
            owners.extend(
                settings
                    .known_accounts
                    .iter()
                    .map(|account| account.owner_id.clone()),
            );
            if let Some(owner) = settings.owner_id.clone() {
                owners.push(owner.clone());
                if let (Some(vault_id), Some(key_id)) =
                    (settings.vault_id.clone(), settings.key_id.clone())
                {
                    scopes.push(SyncScope {
                        service_origin: scope_origin.to_owned(),
                        owner_github_id: owner,
                        vault_id,
                        key_id,
                        device_id: self.0.data.device().id.clone(),
                    });
                }
            }
            scopes.extend(settings.known_accounts.iter().filter_map(|account| {
                Some(SyncScope {
                    service_origin: scope_origin.to_owned(),
                    owner_github_id: account.owner_id.clone(),
                    vault_id: account.vault_id.clone()?,
                    key_id: account.key_id.clone()?,
                    device_id: self.0.data.device().id.clone(),
                })
            }));
        }
        owners.retain(|owner| !owner.trim().is_empty());
        owners.sort();
        owners.dedup();
        scopes.sort_by(|left, right| {
            (
                &left.owner_github_id,
                &left.vault_id,
                &left.key_id,
                &left.device_id,
            )
                .cmp(&(
                    &right.owner_github_id,
                    &right.vault_id,
                    &right.key_id,
                    &right.device_id,
                ))
        });
        scopes.dedup();

        self.0.local.prepare_migration_key().await?;
        for candidate in candidates {
            if candidate.origin() == stable_origin && candidate.shares_root_with(&self.0.local) {
                continue;
            }
            let legacy_origin = candidate.origin().to_owned();
            let legacy_store: Arc<dyn crate::cloud_sync_e2ee_store::ProtectedExtensionStore> =
                Arc::new(candidate.clone());
            for scope in &scopes {
                let mut scope = scope.clone();
                scope.service_origin = legacy_origin.clone();
                self.0
                    .data
                    .migrate_legacy_origin(
                        legacy_store.clone(),
                        legacy_origin.clone(),
                        stable_origin.to_owned(),
                        Some(scope),
                    )
                    .await?;
            }
        }
        for owner in owners {
            let known_account = settings.and_then(|settings| {
                settings
                    .known_accounts
                    .iter()
                    .find(|account| account.owner_id == owner)
                    .cloned()
                    .or_else(|| {
                        (settings.owner_id.as_deref() == Some(owner.as_str())).then(|| {
                            KnownRemoteAccount {
                                owner_id: owner.clone(),
                                vault_id: settings.vault_id.clone(),
                                key_id: settings.key_id.clone(),
                            }
                        })
                    })
            });
            self.migrate_legacy_owner(&owner, known_account.as_ref())
                .await?;
        }
        Ok(())
    }

    async fn migrate_legacy_storage(&self) -> SyncResult<()> {
        let candidates = self.migration_candidates().await?;
        let mut selected = None;
        let mut empty = None;
        let mut first_error = None;
        for candidate in &candidates {
            match candidate
                .read::<serde_json::Value>("device", "client")
                .await
            {
                Ok(Some(_)) => {
                    selected = Some(candidate);
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if selected.is_none() {
            'candidates: for candidate in &candidates {
                let mut candidate_had_error = false;
                for name in DEVICE_RECORD_NAMES {
                    match candidate.read::<serde_json::Value>("device", name).await {
                        Ok(Some(_)) => {
                            selected = Some(candidate);
                            break 'candidates;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            candidate_had_error = true;
                            first_error.get_or_insert(error);
                        }
                    }
                }
                if !candidate_had_error {
                    empty.get_or_insert(candidate);
                }
            }
        }
        let legacy = if let Some(legacy) = selected {
            legacy
        } else if let Some(error) = first_error {
            return Err(error);
        } else if let Some(legacy) = empty {
            legacy
        } else {
            return Ok(());
        };
        let legacy_origin = legacy.origin().to_owned();
        let stable_origin = self.origin();
        let layout_changed = !legacy.shares_root_with(&self.0.local);
        let client_value = legacy.read::<serde_json::Value>("device", "client").await?;
        let legacy_locked_out = legacy.locked_out().await?;
        let mut settings = client_value
            .as_ref()
            .map(|value| {
                serde_json::from_value::<ClientSettings>(value.clone())
                    .map_err(|_| error("recovery_required"))
            })
            .transpose()?;
        let persisted_remote_origin = settings
            .as_ref()
            .and_then(|settings| settings.remote_origin.as_ref())
            .map(|origin| {
                crate::cloud_sync_e2ee_protocol::transport::canonicalize_origin(origin)
                    .map_err(|_| error("recovery_required"))
            })
            .transpose()?;
        let configured_remote_origin = self.read_custom_target().await?.map(|target| target.origin);
        let configured_origin_has_owner_records = match configured_remote_origin.as_deref() {
            Some(origin) => {
                self.configured_origin_has_owner_records(&candidates, origin, settings.as_ref())
                    .await
            }
            None => false,
        };
        let scope_origin = persisted_remote_origin
            .or_else(|| (legacy_origin != stable_origin).then(|| legacy_origin.clone()))
            .or_else(|| {
                configured_origin_has_owner_records
                    .then(|| configured_remote_origin.clone())
                    .flatten()
            })
            .unwrap_or_else(|| legacy_origin.clone());
        if legacy_origin == stable_origin && scope_origin != stable_origin {
            if legacy_locked_out && !self.0.local.locked_out().await? {
                self.0.local.set_lockout(true).await?;
            }
            self.migrate_legacy_state_from_stable_candidates(
                &candidates,
                settings.as_ref(),
                &scope_origin,
                &stable_origin,
            )
            .await?;
            if let Some(settings) = settings.as_mut() {
                settings.remote_origin = Some(scope_origin.clone());
                self.0
                    .local
                    .write("device", "client", settings.clone())
                    .await?;
            }
            return Ok(());
        }
        if self.0.local.origin_migration_complete(&legacy_origin)?
            || (legacy_origin == stable_origin && !layout_changed)
        {
            return Ok(());
        }
        self.0.local.recover_origin_migration()?;

        let mut records = Vec::new();
        for name in DEVICE_RECORD_NAMES {
            let value = if name == "client" {
                client_value.clone()
            } else {
                legacy.read::<serde_json::Value>("device", name).await?
            };
            if let Some(value) = value {
                records.push(LocalMigrationRecord {
                    owner: "device".into(),
                    name: name.into(),
                    value,
                });
            }
        }

        let recovery_scopes = self.0.data.recovery_scopes()?;
        let mut owners = recovery_scopes
            .iter()
            .map(|scope| scope.owner_github_id.clone())
            .collect::<Vec<_>>();
        if let Some(settings) = settings.as_ref() {
            owners.extend(settings.known_owner_ids.iter().cloned());
            owners.extend(
                settings
                    .known_accounts
                    .iter()
                    .map(|account| account.owner_id.clone()),
            );
            if let Some(owner) = settings.owner_id.clone() {
                owners.push(owner);
            }
        }
        owners.retain(|owner| !owner.trim().is_empty());
        owners.sort();
        owners.dedup();
        let mut pending = Vec::new();
        for owner in owners {
            let mut owner_pending = None;
            for name in ["baseline", "pending", "last-seen-revision"] {
                if let Some(value) = legacy.read::<serde_json::Value>(&owner, name).await? {
                    if name == "pending" {
                        owner_pending = Some(
                            serde_json::from_value::<Pending>(value.clone())
                                .map_err(|_| error("recovery_required"))?,
                        );
                    }
                    records.push(LocalMigrationRecord {
                        owner: owner.clone(),
                        name: name.into(),
                        value,
                    });
                }
            }
            if let Some(owner_pending) = owner_pending {
                if !owner_pending.deleted {
                    let name = format!("proposal:{}", owner_pending.operation_id);
                    if let Some(value) = legacy.read::<serde_json::Value>(&owner, &name).await? {
                        records.push(LocalMigrationRecord {
                            owner: owner.clone(),
                            name,
                            value,
                        });
                    }
                }
                pending.push((owner.clone(), owner_pending));
            }
            records.push(LocalMigrationRecord {
                owner: owner.clone(),
                name: "state-origin".into(),
                value: serde_json::json!(scope_origin.clone()),
            });
        }

        let known_scopes = settings
            .as_ref()
            .into_iter()
            .flat_map(|settings| settings.known_accounts.iter())
            .filter_map(|account| {
                Some(SyncScope {
                    service_origin: legacy_origin.clone(),
                    owner_github_id: account.owner_id.clone(),
                    vault_id: account.vault_id.clone()?,
                    key_id: account.key_id.clone()?,
                    device_id: self.0.data.device().id.clone(),
                })
            })
            .collect::<Vec<_>>();
        let current_scope = settings.as_ref().and_then(|settings| {
            Some(SyncScope {
                service_origin: scope_origin.clone(),
                owner_github_id: settings.owner_id.clone()?,
                vault_id: settings.vault_id.clone()?,
                key_id: settings.key_id.clone()?,
                device_id: self.0.data.device().id.clone(),
            })
        });
        self.0.local.prepare_migration_key().await?;
        if legacy_locked_out && !self.0.local.locked_out().await? {
            self.0.local.set_lockout(true).await?;
        }
        let legacy_store: Arc<dyn crate::cloud_sync_e2ee_store::ProtectedExtensionStore> =
            Arc::new(legacy.clone());
        self.0
            .data
            .migrate_legacy_origin(
                legacy_store.clone(),
                legacy_origin.clone(),
                stable_origin.clone(),
                current_scope,
            )
            .await?;
        for scope in &known_scopes {
            self.0
                .data
                .migrate_legacy_origin(
                    legacy_store.clone(),
                    legacy_origin.clone(),
                    stable_origin.clone(),
                    Some(scope.clone()),
                )
                .await?;
        }

        if let Some(settings) = settings.as_mut() {
            settings.remote_origin = Some(scope_origin.clone());
            let value =
                serde_json::to_value(settings).map_err(|_| error("local_storage_unavailable"))?;
            if let Some(record) = records
                .iter_mut()
                .find(|record| record.owner == "device" && record.name == "client")
            {
                record.value = value;
            }
        }

        let mut remembered = Vec::new();
        if let Some(settings) = settings.as_ref() {
            remember_migrated_key(
                legacy,
                &mut remembered,
                settings.owner_id.as_deref().unwrap_or_default(),
                settings.vault_id.as_deref().unwrap_or_default(),
                settings.key_id.as_deref(),
            )
            .await?;
            for account in &settings.known_accounts {
                remember_migrated_key(
                    legacy,
                    &mut remembered,
                    &account.owner_id,
                    account.vault_id.as_deref().unwrap_or_default(),
                    account.key_id.as_deref(),
                )
                .await?;
            }
        }
        for scope in &recovery_scopes {
            remember_migrated_key(
                legacy,
                &mut remembered,
                &scope.owner_github_id,
                &scope.vault_id,
                Some(&scope.key_id),
            )
            .await?;
        }
        for (owner, pending) in &pending {
            remember_migrated_key(
                legacy,
                &mut remembered,
                owner,
                &pending.vault_id,
                pending.key_id.as_deref(),
            )
            .await?;
            remember_migrated_key(
                legacy,
                &mut remembered,
                owner,
                &pending.vault_id,
                pending.old_key_id.as_deref(),
            )
            .await?;
        }
        for (owner, vault, key_id, key) in remembered {
            if legacy.origin() == self.0.local.origin() {
                continue;
            }
            self.0
                .local
                .remember(&owner, &vault, &key_id, &key, true)
                .await?;
            // The stable binding is durable before removing the legacy binding. If
            // migration is interrupted, the next attempt can safely retry either
            // side without leaving two unlock credentials behind.
            legacy.forget(&owner, &vault, &key_id).await?;
        }
        self.0
            .local
            .migrate_records(&legacy_origin, records)
            .await?;
        Ok(())
    }

    /// Migrate a previously used account lazily once its identity is known. Older
    /// installs did not persist an owner index, so startup can only migrate the
    /// account in `device/client`; this closes that gap without guessing owner IDs.
    async fn migrate_legacy_owner(
        &self,
        owner: &str,
        known_account: Option<&KnownRemoteAccount>,
    ) -> SyncResult<()> {
        let candidates = self.migration_candidates().await?;

        for legacy in candidates {
            if legacy.origin() == self.0.local.origin() && legacy.shares_root_with(&self.0.local) {
                continue;
            }
            let migrated_origin = self
                .0
                .local
                .read::<String>(owner, "state-origin")
                .await
                .ok()
                .flatten();
            if migrated_origin.as_deref() == Some(legacy.origin()) {
                continue;
            }
            let mut records = Vec::new();
            let mut pending = None;
            let mut found = false;
            for name in ["baseline", "pending", "last-seen-revision"] {
                let value = match legacy.read::<serde_json::Value>(owner, name).await {
                    Ok(value) => value,
                    Err(error) => {
                        let stable_value =
                            self.0.local.read::<serde_json::Value>(owner, name).await;
                        if stable_value.as_ref().is_ok_and(|value| value.is_some()) {
                            break;
                        }
                        if found {
                            return Err(error);
                        }
                        break;
                    }
                };
                let Some(value) = value else {
                    continue;
                };
                let stable_value = self.0.local.read::<serde_json::Value>(owner, name).await;
                if stable_value.as_ref().is_ok_and(|value| value.is_some()) {
                    continue;
                }
                found = true;
                if name == "pending" {
                    pending = Some(
                        serde_json::from_value::<Pending>(value.clone())
                            .map_err(|_| error("recovery_required"))?,
                    );
                }
                records.push(LocalMigrationRecord {
                    owner: owner.to_owned(),
                    name: name.to_owned(),
                    value,
                });
            }
            if !found {
                continue;
            }

            if let Some(owner_pending) = &pending {
                if !owner_pending.deleted {
                    let name = format!("proposal:{}", owner_pending.operation_id);
                    if let Some(value) = legacy.read::<serde_json::Value>(owner, &name).await? {
                        records.push(LocalMigrationRecord {
                            owner: owner.to_owned(),
                            name,
                            value,
                        });
                    }
                }
            }

            let current_scope = pending
                .as_ref()
                .and_then(|pending| {
                    Some(SyncScope {
                        service_origin: legacy.origin().to_owned(),
                        owner_github_id: owner.to_owned(),
                        vault_id: pending.vault_id.clone(),
                        key_id: pending.key_id.clone()?,
                        device_id: self.0.data.device().id.clone(),
                    })
                })
                .or_else(|| {
                    known_account.and_then(|account| {
                        Some(SyncScope {
                            service_origin: legacy.origin().to_owned(),
                            owner_github_id: owner.to_owned(),
                            vault_id: account.vault_id.clone()?,
                            key_id: account.key_id.clone()?,
                            device_id: self.0.data.device().id.clone(),
                        })
                    })
                });
            self.0.local.prepare_migration_key().await?;
            self.0
                .data
                .migrate_legacy_origin(
                    Arc::new(legacy.clone()),
                    legacy.origin().to_owned(),
                    self.0.local.origin().to_owned(),
                    current_scope,
                )
                .await?;

            let mut remembered = Vec::new();
            if let Some(account) = known_account {
                remember_migrated_key(
                    &legacy,
                    &mut remembered,
                    owner,
                    account.vault_id.as_deref().unwrap_or_default(),
                    account.key_id.as_deref(),
                )
                .await?;
            }
            if let Some(owner_pending) = &pending {
                remember_migrated_key(
                    &legacy,
                    &mut remembered,
                    owner,
                    &owner_pending.vault_id,
                    owner_pending.key_id.as_deref(),
                )
                .await?;
                remember_migrated_key(
                    &legacy,
                    &mut remembered,
                    owner,
                    &owner_pending.vault_id,
                    owner_pending.old_key_id.as_deref(),
                )
                .await?;
            }
            for (owner, vault, key_id, key) in remembered {
                if legacy.origin() == self.0.local.origin() {
                    continue;
                }
                self.0
                    .local
                    .remember(&owner, &vault, &key_id, &key, true)
                    .await?;
                legacy.forget(&owner, &vault, &key_id).await?;
            }
            if self
                .0
                .local
                .read::<String>(owner, "state-origin")
                .await
                .ok()
                .flatten()
                .as_deref()
                != Some(legacy.origin())
            {
                records.push(LocalMigrationRecord {
                    owner: owner.to_owned(),
                    name: "state-origin".into(),
                    value: serde_json::json!(legacy.origin()),
                });
            }
            self.0
                .local
                .migrate_records(legacy.origin(), records)
                .await?;
            return Ok(());
        }
        Ok(())
    }

    async fn ensure_remote_state_origin(
        &self,
        owner: &str,
        origin: &str,
        origin_changed: bool,
    ) -> SyncResult<()> {
        let marker = match self.0.local.read::<String>(owner, "state-origin").await {
            Ok(value) => value,
            Err(_) if origin_changed => None,
            Err(error) => return Err(error),
        };
        let stale = marker.as_deref().is_some_and(|stored| stored != origin)
            || (marker.is_none() && origin_changed);
        if stale {
            for record in ["baseline", "pending", "last-seen-revision"] {
                self.0.local.remove(owner, record).await?;
            }
        }
        self.0
            .local
            .write(owner, "state-origin", origin.to_owned())
            .await
    }

    #[cfg(test)]
    pub(super) async fn ensure_remote_state_origin_for_test(
        &self,
        owner: &str,
        origin: &str,
        origin_changed: bool,
    ) -> SyncResult<()> {
        self.ensure_remote_state_origin(owner, origin, origin_changed)
            .await
    }

    async fn migrate_legacy_scope(
        &self,
        owner: &str,
        vault_id: &str,
        key_id: &str,
    ) -> SyncResult<()> {
        let candidates = self.migration_candidates().await?;
        for legacy in candidates {
            if legacy.origin() == self.0.local.origin() && legacy.shares_root_with(&self.0.local) {
                continue;
            }
            self.0
                .data
                .migrate_legacy_origin(
                    Arc::new(legacy.clone()),
                    legacy.origin().to_owned(),
                    self.0.local.origin().to_owned(),
                    Some(SyncScope {
                        service_origin: legacy.origin().to_owned(),
                        owner_github_id: owner.to_owned(),
                        vault_id: vault_id.to_owned(),
                        key_id: key_id.to_owned(),
                        device_id: self.0.data.device().id.clone(),
                    }),
                )
                .await?;
        }
        Ok(())
    }

    async fn initialize(&self, runtime: &mut Runtime) -> SyncResult<()> {
        // Recovery precedes all ordinary mutations and backend-ready reporting.
        // It also handles a dropped IPC future without requiring a process restart.
        if !runtime.initialized {
            self.0.local.recover_origin_migration()?;
            self.migrate_legacy_storage().await?;
        }
        self.0.data.recover().await?;
        if runtime.initialized {
            return Ok(());
        }
        if self.0.local.initialized()? {
            runtime.settings = self
                .0
                .local
                .read("device", "client")
                .await?
                .ok_or_else(|| error("recovery_required"))?;
            if runtime.settings.version != 1 {
                return Err(error("recovery_required"));
            }
            let mut owners = runtime.settings.known_owner_ids.clone();
            if let Some(owner) = runtime.settings.owner_id.clone() {
                owners.push(owner);
            }
            owners.extend(
                runtime
                    .settings
                    .known_accounts
                    .iter()
                    .map(|account| account.owner_id.clone()),
            );
            owners.sort();
            owners.dedup();
            for owner in owners {
                let known_account = runtime
                    .settings
                    .known_accounts
                    .iter()
                    .find(|account| account.owner_id == owner)
                    .cloned();
                self.migrate_legacy_owner(&owner, known_account.as_ref())
                    .await?;
            }
        }
        if self.0.local.locked_out().await? {
            runtime.settings.remember_key = false;
            runtime.key = None;
            runtime.preview = None;
        }
        runtime.initialized = true;
        Ok(())
    }

    async fn save_settings(&self, runtime: &Runtime) -> SyncResult<()> {
        self.0
            .local
            .write("device", "client", runtime.settings.clone())
            .await
    }

    fn record_known_account(runtime: &mut Runtime) {
        let Some(owner_id) = runtime.settings.owner_id.clone() else {
            return;
        };
        if let Some(account) = runtime
            .settings
            .known_accounts
            .iter_mut()
            .find(|account| account.owner_id == owner_id)
        {
            if runtime.settings.vault_id.is_some() {
                account.vault_id = runtime.settings.vault_id.clone();
            }
            if runtime.settings.key_id.is_some() {
                account.key_id = runtime.settings.key_id.clone();
            }
        } else {
            runtime.settings.known_accounts.push(KnownRemoteAccount {
                owner_id,
                vault_id: runtime.settings.vault_id.clone(),
                key_id: runtime.settings.key_id.clone(),
            });
            runtime
                .settings
                .known_accounts
                .sort_by(|left, right| left.owner_id.cmp(&right.owner_id));
        }
    }

    async fn connect(&self, runtime: &mut Runtime) -> SyncResult<()> {
        self.initialize(runtime).await?;
        self.check_cancelled()?;
        if runtime.retry_at.is_some_and(|at| at > Instant::now()) {
            return Err(error("rate_limited"));
        }
        let custom_target = self.read_custom_target().await?;
        let target_origin = custom_target
            .as_ref()
            .map(|target| target.origin.as_str())
            .unwrap_or_else(|| self.0.config.origin.as_str());
        let origin_changed = self
            .0
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .service_origin
            != target_origin;
        if origin_changed {
            self.update(|status| status.service_origin = target_origin.to_owned());
        }
        let use_system_proxy = custom_target.is_none() && crate::net::use_system_proxy();
        let proxy_policy = ProxyPolicy::for_origin(target_origin, use_system_proxy);
        log::warn!(
            "[e2ee-token] connect: use_system_proxy={use_system_proxy} proxy_policy={proxy_policy:?} custom_mode={}",
            custom_target.is_some(),
        );
        self.connect_with_proxy_policy(runtime, proxy_policy, custom_target)
            .await
    }

    async fn connect_with_proxy_policy(
        &self,
        runtime: &mut Runtime,
        proxy_policy: ProxyPolicy,
        custom_target: Option<CustomSyncTarget>,
    ) -> SyncResult<()> {
        let origin = custom_target
            .as_ref()
            .map(|target| target.origin.clone())
            .unwrap_or_else(|| self.origin());
        log::warn!(
            "[e2ee-token] connect_with_proxy_policy: origin={} custom_token_present={} has_cached_connection={}",
            origin,
            custom_target.is_some(),
            runtime.connection.is_some()
        );
        let valid = if let Some(connection) = &runtime.connection {
            let credential_unchanged = match (&connection.credential, custom_target.as_ref()) {
                (ConnectionCredential::CustomToken(existing), Some(target)) => {
                    existing == &target.token
                }
                (ConnectionCredential::Github(existing), None) => {
                    self.0.marketplace.read_access_token().await.ok().as_ref() == Some(existing)
                }
                // Switching between GitHub and custom-token mode (token just
                // added, or just cleared) always forces a fresh connect.
                _ => false,
            };
            connection.expires_at > Instant::now() + Duration::from_secs(30)
                && connection.transport.service_origin() == origin
                && connection.transport.proxy_policy() == proxy_policy
                && credential_unchanged
        } else {
            false
        };
        if valid {
            return Ok(());
        }
        let custom_mode = custom_target.is_some();
        #[cfg(not(test))]
        let transport = {
            let result = if custom_mode {
                Transport::new_for_custom_token(&origin, proxy_policy).await
            } else {
                Transport::new(&origin, &self.0.config.github_client_id, proxy_policy).await
            };
            match result {
                Ok(transport) => transport,
                Err(e) => {
                    log::warn!("[e2ee-token] Transport::new failed: {e:?}");
                    return Err(protocol_error(e));
                }
            }
        };
        #[cfg(test)]
        let transport = if origin.starts_with("http://127.0.0.1:") {
            if custom_mode {
                Transport::for_test_with_proxy_policy_for_custom_token(&origin, proxy_policy)
                    .await
                    .map_err(protocol_error)?
            } else {
                Transport::for_test_with_proxy_policy(
                    &origin,
                    &self.0.config.github_client_id,
                    proxy_policy,
                )
                .await
                .map_err(protocol_error)?
            }
        } else if custom_mode {
            Transport::new_for_custom_token(&origin, proxy_policy)
                .await
                .map_err(protocol_error)?
        } else {
            Transport::new(&origin, &self.0.config.github_client_id, proxy_policy)
                .await
                .map_err(protocol_error)?
        };
        let (session, credential, account) = if let Some(target) = custom_target {
            self.check_cancelled()?;
            log::warn!(
                "[e2ee-token] calling exchange_with_token against {}",
                origin
            );
            let session = match transport
                .exchange_with_token(target.token.expose_secret())
                .await
            {
                Ok(session) => {
                    log::warn!("[e2ee-token] exchange_with_token succeeded");
                    session
                }
                Err(e) => {
                    log::warn!("[e2ee-token] exchange_with_token failed: {e:?}");
                    return Err(protocol_error(e));
                }
            };
            if transport.service_origin() != origin {
                return Err(error("account_changed"));
            }
            let account = session.account().clone();
            (
                session,
                ConnectionCredential::CustomToken(target.token),
                account,
            )
        } else {
            let (token, account) = self
                .0
                .marketplace
                .sync_identity()
                .await
                .map_err(|_| error("sign_in_required"))?;
            self.check_cancelled()?;
            let session = transport
                .exchange(token.expose_secret(), &account.github_id)
                .await
                .map_err(protocol_error)?;
            if session.account().github_id != account.github_id
                || transport.service_origin() != origin
            {
                return Err(error("account_changed"));
            }
            if self
                .0
                .marketplace
                .read_access_token()
                .await
                .map_err(|_| error("sign_in_required"))?
                != token
            {
                return Err(error("account_changed"));
            }
            (session, ConnectionCredential::Github(token), account)
        };
        let backup_retention_days = transport.capabilities().max_backup_retention_days;
        let target_changed = runtime
            .settings
            .remote_origin
            .as_deref()
            .is_some_and(|previous| previous != origin)
            || (custom_mode
                && runtime.settings.remote_origin.is_none()
                && runtime.settings.owner_id.is_some());
        if target_changed {
            self.reset_remote_state(runtime, Some(account.github_id.as_str()))
                .await?;
        }
        runtime.settings.remote_origin = Some(origin.clone());
        let owner = account.github_id.as_str().to_string();
        if runtime.settings.owner_id.as_deref() != Some(&owner) {
            if runtime.settings.owner_id.is_some() {
                runtime.settings.enabled = false;
                runtime.settings.remember_key = false;
                self.save_settings(runtime).await?;
            }
            self.forget_unlock_material(runtime).await?;
            runtime.settings.enabled = false;
            runtime.settings.consent_version = None;
            runtime.settings.remember_key = false;
            runtime.settings.last_success = None;
            runtime.settings.last_generation = None;
            runtime.settings.vault_id = None;
            runtime.settings.key_id = None;
            runtime.key = None;
            runtime.baseline = None;
            runtime.preview = None;
            runtime.metadata = None;
            runtime.snapshot = None;
        }
        runtime.settings.owner_id = Some(owner.clone());
        if !runtime.settings.known_owner_ids.contains(&owner) {
            runtime.settings.known_owner_ids.push(owner.clone());
            runtime.settings.known_owner_ids.sort();
        }
        Self::record_known_account(runtime);
        self.save_settings(runtime).await?;
        let known_account = runtime
            .settings
            .known_accounts
            .iter()
            .find(|account| account.owner_id == owner)
            .cloned();
        self.migrate_legacy_owner(&owner, known_account.as_ref())
            .await?;
        self.ensure_remote_state_origin(&owner, &origin, target_changed)
            .await?;
        let expires_at = Instant::now() + Duration::from_secs(u64::from(session.expires_in()));
        runtime.connection = Some(Connection {
            transport,
            session,
            credential,
            expires_at,
        });
        runtime.baseline = self.0.local.read(&owner, "baseline").await?;
        let pending: Option<Pending> = self.0.local.read(&owner, "pending").await?;
        self.update(|s| {
            s.account = Some(SyncAccount {
                github_id: owner,
                login: account.login,
            });
            s.auth_state = AuthState::SignedIn;
            s.backup_retention_days = Some(backup_retention_days);
            s.pending_operation_id = pending.map(|p| p.operation_id);
            s.has_cloud_snapshot = None;
        });
        Ok(())
    }

    #[cfg(test)]
    pub(super) async fn connect_with_proxy_setting_for_test(
        &self,
        use_system_proxy: bool,
    ) -> SyncResult<()> {
        let mut runtime = self.0.runtime.lock().await;
        self.initialize(&mut runtime).await?;
        self.check_cancelled()?;
        let custom_target = self.read_custom_target().await?;
        let target_origin = custom_target
            .as_ref()
            .map(|target| target.origin.clone())
            .unwrap_or_else(|| self.origin());
        let policy =
            ProxyPolicy::for_origin(&target_origin, custom_target.is_none() && use_system_proxy);
        self.connect_with_proxy_policy(&mut runtime, policy, custom_target)
            .await
    }

    async fn refresh_metadata(&self, runtime: &mut Runtime) -> SyncResult<()> {
        self.current_account(runtime).await?;
        let connection = runtime
            .connection
            .as_ref()
            .ok_or_else(|| error("sign_in_required"))?;
        let metadata = match connection
            .transport
            .metadata(&connection.session, None)
            .await
            .map_err(protocol_error)?
        {
            MetadataResult::Modified(value) => *value,
            MetadataResult::NotModified => return Err(error("invalid_response")),
        };
        self.current_account(runtime).await?;
        let value = metadata.value();
        let owner = self.owner(runtime)?.to_owned();
        let seen: Option<Revision> = self.0.local.read(&owner, "last-seen-revision").await?;
        if seen.is_some_and(|seen| value.revision < seen)
            || runtime
                .baseline
                .as_ref()
                .is_some_and(|base| value.revision < base.revision)
        {
            return Err(error("revision_rollback"));
        }
        self.0
            .local
            .write(&owner, "last-seen-revision", value.revision)
            .await?;
        let key_binding_changed = runtime.key.is_some()
            && (runtime.settings.vault_id.as_deref()
                != value.vault_id.as_ref().map(UuidV4::as_str)
                || runtime.settings.key_id.as_deref() != value.key_id.as_ref().map(UuidV4::as_str));
        if value.state != VaultState::Active
            || key_binding_changed
            || runtime.snapshot.as_ref().is_some_and(|s| {
                Some(&s.key_id) != value.key_id.as_ref()
                    || Some(&s.vault_id) != value.vault_id.as_ref()
            })
        {
            runtime.key = None;
            runtime.snapshot = None;
            runtime.preview = None;
        }
        if value.state == VaultState::Deleted {
            runtime.settings.enabled = false;
        }
        self.update(|s| {
            s.remote_revision = Some(value.revision.as_str().into());
            s.vault_id = value.vault_id.as_ref().map(|v| v.as_str().into());
            s.key_id = value.key_id.as_ref().map(|v| v.as_str().into());
            s.has_cloud_snapshot = Some(value.state == VaultState::Active);
        });
        if let (Some(vault_id), Some(key_id)) = (&value.vault_id, &value.key_id) {
            self.migrate_legacy_scope(&owner, vault_id.as_str(), key_id.as_str())
                .await?;
        }
        runtime.metadata = Some(metadata);
        Ok(())
    }

    fn metadata<'a>(&self, runtime: &'a Runtime) -> SyncResult<&'a Metadata> {
        runtime
            .metadata
            .as_ref()
            .ok_or_else(|| error("metadata_required"))
    }
    fn owner<'a>(&self, runtime: &'a Runtime) -> SyncResult<&'a str> {
        runtime
            .settings
            .owner_id
            .as_deref()
            .ok_or_else(|| error("sign_in_required"))
    }
    fn unlocked(&self, runtime: &Runtime) -> SyncResult<Arc<DerivedKey>> {
        runtime.key.clone().ok_or_else(|| error("unlock_required"))
    }

    async fn download(&self, runtime: &mut Runtime) -> SyncResult<SnapshotUpload> {
        self.current_account(runtime).await?;
        let connection = runtime
            .connection
            .as_ref()
            .ok_or_else(|| error("sign_in_required"))?;
        let snapshot = connection
            .transport
            .snapshot(&connection.session, self.metadata(runtime)?)
            .await
            .map_err(protocol_error)?;
        self.current_account(runtime).await?;
        Ok(snapshot)
    }

    async fn decrypt(
        &self,
        runtime: &Runtime,
        snapshot: SnapshotUpload,
        key: Arc<DerivedKey>,
    ) -> SyncResult<ValidatedSyncDocuments> {
        let metadata = self.metadata(runtime)?.value().clone();
        let owner = GithubId::parse(self.owner(runtime)?).map_err(protocol_error)?;
        tokio::task::spawn_blocking(move || {
            let set = crypto::decrypt_snapshot(&snapshot, &metadata, &owner, &key)
                .map_err(protocol_error)?;
            documents::validate_sync_documents(set, metadata.revision).map_err(document_error)
        })
        .await
        .map_err(|_| error("crypto_worker_failed"))?
    }

    async fn remember(
        &self,
        runtime: &mut Runtime,
        snapshot: &SnapshotUpload,
        key: Arc<DerivedKey>,
        remember: bool,
    ) -> SyncResult<()> {
        self.0
            .local
            .remember(
                self.owner(runtime)?,
                snapshot.vault_id.as_str(),
                snapshot.key_id.as_str(),
                &key,
                remember,
            )
            .await?;
        runtime.key = Some(key);
        runtime.settings.remember_key = remember;
        runtime.settings.key_preference_epoch = runtime
            .settings
            .key_preference_epoch
            .checked_add(1)
            .ok_or_else(|| error("recovery_required"))?;
        runtime.settings.vault_id = Some(snapshot.vault_id.as_str().into());
        runtime.settings.key_id = Some(snapshot.key_id.as_str().into());
        Self::record_known_account(runtime);
        self.save_settings(runtime).await?;
        self.0.local.set_lockout(false).await
    }

    async fn try_remembered(&self, runtime: &mut Runtime) -> SyncResult<()> {
        if runtime.key.is_some()
            || !runtime.settings.remember_key
            || self.0.local.locked_out().await?
        {
            return Ok(());
        }
        let metadata = self.metadata(runtime)?.value();
        if metadata.state != VaultState::Active {
            return Ok(());
        }
        let vault = metadata
            .vault_id
            .as_ref()
            .ok_or_else(|| error("invalid_response"))?
            .as_str();
        let key_id = metadata
            .key_id
            .as_ref()
            .ok_or_else(|| error("invalid_response"))?
            .as_str();
        if runtime.settings.vault_id.as_deref() != Some(vault)
            || runtime.settings.key_id.as_deref() != Some(key_id)
        {
            return Ok(());
        }
        if let Some(key) = self
            .0
            .local
            .remembered(self.owner(runtime)?, vault, key_id)
            .await?
        {
            let snapshot = self.download(runtime).await?;
            let key = Arc::new(key);
            // Merely retrieving/deriving a key cannot set keyState=unlocked.
            self.decrypt(runtime, snapshot.clone(), key.clone()).await?;
            self.current_account(runtime).await?;
            runtime.snapshot = Some(snapshot);
            runtime.key = Some(key);
        }
        Ok(())
    }

    pub(crate) async fn prepare_enable(
        &self,
        consent_version: String,
    ) -> SyncResult<EnablePreparation> {
        if consent_version != CONSENT_VERSION {
            return Err(error("consent_required"));
        }
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result: SyncResult<EnableStep> = async {
            self.connect(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            runtime.settings.consent_version = Some(consent_version);
            runtime.settings.prompted = true;
            self.save_settings(&runtime).await?;
            self.try_remembered(&mut runtime).await?;
            let step = if self.metadata(&runtime)?.value().state != VaultState::Active {
                EnableStep::Create
            } else if runtime.key.is_none() {
                EnableStep::Unlock
            } else if runtime.baseline.as_ref().is_some_and(|b| {
                Some(&b.vault_id)
                    == self
                        .metadata(&runtime)
                        .ok()
                        .and_then(|m| m.value().vault_id.as_ref())
            }) {
                EnableStep::Ready
            } else {
                EnableStep::RestoreReview
            };
            Ok(step)
        }
        .await;
        match &result {
            Ok(step) => log::warn!("[e2ee-token] prepare_enable: succeeded, next_step={step:?}"),
            Err(e) => log::warn!(
                "[e2ee-token] prepare_enable: failed code={:?} message={}",
                e.code,
                e.message
            ),
        }
        self.finish(&mut runtime, &result);
        result.map(|next_step| EnablePreparation {
            next_step,
            status: self.status(),
        })
    }

    pub(crate) async fn unlock(
        &self,
        password: String,
        remember_key: bool,
    ) -> SyncResult<EncryptedSyncStatus> {
        let password = SecretInput::new(password);
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.0.auto_suspended.store(false, Ordering::Release);
        self.begin();
        let result = async {
            self.connect(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            if runtime.settings.consent_version.as_deref() != Some(CONSENT_VERSION) {
                return Err(error("consent_required"));
            }
            let snapshot = self.download(&mut runtime).await?;
            let encoded = snapshot.clone();
            let metadata = self.metadata(&runtime)?.value().clone();
            let owner = GithubId::parse(self.owner(&runtime)?).map_err(protocol_error)?;
            let (key, documents) = tokio::task::spawn_blocking(move || {
                let (key, set) =
                    crypto::decrypt_snapshot_with_password(&encoded, &metadata, &owner, password)
                        .map_err(protocol_error)?;
                let documents = documents::validate_sync_documents(set, metadata.revision)
                    .map_err(document_error)?;
                Ok::<_, BackendError>((Arc::new(key), documents))
            })
            .await
            .map_err(|_| error("crypto_worker_failed"))??;
            self.current_account(&runtime).await?;
            if let (Some(vault), Some(key_id)) =
                (&runtime.settings.vault_id, &runtime.settings.key_id)
            {
                if vault != snapshot.vault_id.as_str() || key_id != snapshot.key_id.as_str() {
                    self.0
                        .local
                        .forget(self.owner(&runtime)?, vault, key_id)
                        .await?;
                }
            }
            self.remember(&mut runtime, &snapshot, key, remember_key)
                .await?;
            runtime.snapshot = Some(snapshot);
            if !runtime.baseline.as_ref().is_some_and(|b| {
                Some(&b.vault_id)
                    == self
                        .metadata(&runtime)
                        .ok()
                        .and_then(|m| m.value().vault_id.as_ref())
            }) {
                self.make_preview(&mut runtime, documents).await?;
            }
            Ok(())
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn create(
        &self,
        password: String,
        confirmation: String,
        remember_key: bool,
        consent_version: String,
        observed_revision: String,
    ) -> SyncResult<EncryptedSyncStatus> {
        let password = SecretInput::new(password);
        let confirmation = SecretInput::new(confirmation);
        if consent_version != CONSENT_VERSION {
            return Err(error("consent_required"));
        }
        log::warn!("[e2ee-token] create: entered, observed_revision={observed_revision}");
        let observed = Revision::parse(&observed_revision).map_err(protocol_error)?;
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.0.auto_suspended.store(false, Ordering::Release);
        self.begin();
        let result = async {
            self.connect(&mut runtime).await?;
            self.reconcile_for_review(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            let metadata = self.metadata(&runtime)?.value();
            log::warn!(
                "[e2ee-token] create: server state={:?} server_revision={} observed={}",
                metadata.state,
                metadata.revision.as_str(),
                observed.as_str()
            );
            if metadata.state == VaultState::Active || metadata.revision != observed {
                log::warn!("[e2ee-token] create: revision_conflict (state or revision mismatch)");
                return Err(error("revision_conflict"));
            }
            if runtime.settings.consent_version.as_deref() != Some(CONSENT_VERSION) {
                return Err(error("consent_required"));
            }
            self.retire_reviewed_pending(&mut runtime, None).await?;
            let context = EncryptContext {
                owner_github_id: GithubId::parse(self.owner(&runtime)?).map_err(protocol_error)?,
                vault_id: UuidV4::random().map_err(protocol_error)?,
                key_id: UuidV4::random().map_err(protocol_error)?,
                base_revision: observed,
                operation_id: UuidV4::random().map_err(protocol_error)?,
                kind: UploadKind::Create,
                kdf: Kdf::fixed(Salt::random().map_err(protocol_error)?),
            };
            let captured = self
                .0
                .data
                .export(self.scope(&runtime, context.vault_id.as_str(), context.key_id.as_str())?)
                .await?;
            let documents = captured.documents.documents().clone();
            let (snapshot, key) = tokio::task::spawn_blocking(move || {
                let password = NormalizedPassword::confirmed_new(password, confirmation)
                    .map_err(protocol_error)?;
                let key =
                    Arc::new(crypto::derive_key(&password, &context.kdf).map_err(protocol_error)?);
                let snapshot =
                    crypto::encrypt_snapshot(&documents, &context, &key).map_err(protocol_error)?;
                Ok::<_, BackendError>((snapshot, key))
            })
            .await
            .map_err(|_| error("crypto_worker_failed"))??;
            self.current_account(&runtime).await?;
            self.remember(&mut runtime, &snapshot, key, remember_key)
                .await?;
            self.upload(&mut runtime, snapshot, captured, None).await?;
            self.check_cancelled()?;
            runtime.settings.enabled = true;
            self.save_settings(&runtime).await?;
            Ok(())
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    async fn upload(
        &self,
        runtime: &mut Runtime,
        snapshot: SnapshotUpload,
        captured: ExportedDocuments,
        old_key_id: Option<String>,
    ) -> SyncResult<()> {
        self.current_account(runtime).await?;
        let connection = runtime
            .connection
            .as_ref()
            .ok_or_else(|| error("sign_in_required"))?;
        let operation = connection
            .transport
            .prepare_upload(
                &connection.session,
                self.metadata(runtime)?,
                &snapshot,
                runtime.snapshot.as_ref(),
            )
            .map_err(protocol_error)?;
        let pending = Pending {
            record: String::from_utf8(operation.to_pending_record().map_err(protocol_error)?)
                .map_err(|_| error("invalid_response"))?,
            operation_id: snapshot.operation_id.as_str().into(),
            vault_id: snapshot.vault_id.as_str().into(),
            key_id: Some(snapshot.key_id.as_str().into()),
            local_generation: captured.generation,
            deleted: false,
            remember_key: runtime.settings.remember_key,
            key_preference_epoch: runtime.settings.key_preference_epoch,
            base_revision: snapshot.base_revision,
            old_key_id,
        };
        let owner = self.owner(runtime)?.to_string();
        self.0
            .local
            .write(
                &owner,
                &format!("proposal:{}", pending.operation_id),
                captured.documents.documents().clone(),
            )
            .await?;
        self.0
            .local
            .write(&owner, "pending", pending.clone())
            .await?;
        self.update(|s| s.pending_operation_id = Some(pending.operation_id.clone()));
        self.current_account(runtime).await?;
        // Once submitted, preserve the exact operation until a validated receipt.
        let receipt = connection
            .transport
            .submit(&connection.session, &operation)
            .await;
        match receipt {
            Ok(receipt) => {
                self.accept_receipt(runtime, &pending, &receipt.receipt)
                    .await?;
                runtime.snapshot = Some(snapshot);
                if receipt.replayed {
                    // A replay proves that operation committed, not that its
                    // revision is still the server's current head.
                    self.update(|s| s.has_cloud_snapshot = None);
                }
                Ok(())
            }
            Err(e) => {
                if definite_rejection(&e) {
                    self.discard_rejected(runtime, &pending).await?;
                    if snapshot.kind != UploadKind::Snapshot {
                        self.0
                            .local
                            .forget(&owner, snapshot.vault_id.as_str(), snapshot.key_id.as_str())
                            .await?;
                    }
                    if snapshot.kind == UploadKind::Create {
                        runtime.key = None;
                        runtime.settings.remember_key = false;
                        runtime.settings.key_id = None;
                        runtime.settings.vault_id = None;
                        self.save_settings(runtime).await?;
                    }
                    return Err(protocol_error(e));
                }
                Err(error("outcome_unknown"))
            }
        }
    }

    async fn reconcile(&self, runtime: &mut Runtime) -> SyncResult<()> {
        let owner = self.owner(runtime)?.to_owned();
        let Some(pending): Option<Pending> = self.0.local.read(&owner, "pending").await? else {
            return Ok(());
        };
        let connection = runtime
            .connection
            .as_ref()
            .ok_or_else(|| error("sign_in_required"))?;
        let operation = connection
            .transport
            .restore_pending(&connection.session, pending.record.as_bytes())
            .map_err(protocol_error)?;
        if operation.operation_id().as_str() != pending.operation_id {
            return Err(error("recovery_required"));
        }
        self.current_account(runtime).await?;
        let receipt = match connection
            .transport
            .operation(&connection.session, &operation)
            .await
            .map_err(protocol_error)?
        {
            OperationStatus::Committed(receipt) => receipt,
            OperationStatus::Pending(pending) => {
                let mut result = error("outcome_unknown");
                result.details = Some(
                    serde_json::json!({"reason":"outcome_unknown","retryAfterSeconds":pending.retry_after_seconds}),
                );
                return Err(result);
            }
            OperationStatus::NotFound => {
                // 404 does not prove failure. Only an identical body/op ID can be retried.
                self.current_account(runtime).await?;
                match connection
                    .transport
                    .submit(&connection.session, &operation)
                    .await
                {
                    Ok(receipt) => receipt.receipt,
                    Err(e) if definite_rejection(&e) => {
                        // This is an older unknown operation: rejection of a
                        // retry cannot prove that the original never committed.
                        let _ = self.refresh_metadata(runtime).await;
                        return Err(error("outcome_unknown"));
                    }
                    Err(_) => return Err(error("outcome_unknown")),
                }
            }
        };
        self.accept_receipt(runtime, &pending, &receipt).await
    }

    async fn accept_receipt(
        &self,
        runtime: &mut Runtime,
        pending: &Pending,
        receipt: &OperationReceipt,
    ) -> SyncResult<()> {
        let owner = self.owner(runtime)?.to_string();
        if receipt.operation_id.as_str() != pending.operation_id
            || receipt.vault_id.as_str() != pending.vault_id
        {
            return Err(error("invalid_response"));
        }
        if pending.deleted {
            runtime.settings.enabled = false;
            runtime.key = None;
            runtime.preview = None;
            runtime.baseline = None;
            self.0.local.remove(&owner, "baseline").await?;
            if let Some(key_id) = &runtime.settings.key_id {
                self.0
                    .local
                    .forget(&owner, &pending.vault_id, key_id)
                    .await?;
            }
            runtime.settings.remember_key = false;
            runtime.settings.vault_id = None;
            runtime.settings.key_id = None;
        } else {
            if runtime.settings.vault_id.as_deref() != Some(pending.vault_id.as_str())
                || runtime.settings.key_id.as_ref() != pending.key_id.as_ref()
            {
                // Receipt reconciliation can complete a password rotation while
                // memory still holds the old key from the failed upload turn.
                runtime.key = None;
                runtime.snapshot = None;
                runtime.preview = None;
            }
            let documents: DocumentSet = self
                .0
                .local
                .read(&owner, &format!("proposal:{}", pending.operation_id))
                .await?
                .ok_or_else(|| error("recovery_required"))?;
            documents::validate_sync_documents(documents.clone(), receipt.committed_revision)
                .map_err(document_error)?;
            let baseline = Baseline {
                revision: receipt.committed_revision,
                vault_id: receipt.vault_id.clone(),
                documents,
            };
            self.0
                .local
                .write(&owner, "baseline", baseline.clone())
                .await?;
            self.0
                .data
                .baseline(
                    self.scope(
                        runtime,
                        &pending.vault_id,
                        pending
                            .key_id
                            .as_deref()
                            .ok_or_else(|| error("recovery_required"))?,
                    )?,
                    baseline.documents.clone(),
                    baseline.revision,
                )
                .await?;
            runtime.baseline = Some(baseline);
            runtime.settings.vault_id = Some(pending.vault_id.clone());
            runtime.settings.key_id = pending.key_id.clone();
            if runtime.settings.key_preference_epoch == pending.key_preference_epoch
                && !self.0.local.locked_out().await?
            {
                runtime.settings.remember_key = pending.remember_key;
            }
            if let Some(old) = &pending.old_key_id {
                if Some(old) != pending.key_id.as_ref() {
                    self.0.local.forget(&owner, &pending.vault_id, old).await?;
                }
            }
        }
        runtime.settings.last_generation = Some(pending.local_generation.as_str().into());
        runtime.settings.last_success = Some(receipt.committed_at.clone());
        let seen: Option<Revision> = self.0.local.read(&owner, "last-seen-revision").await?;
        self.0
            .local
            .write(
                &owner,
                "last-seen-revision",
                seen.map_or(receipt.committed_revision, |seen| {
                    seen.max(receipt.committed_revision)
                }),
            )
            .await?;
        self.save_settings(runtime).await?;
        // Deleting the pending record is last; a crash at any earlier step replays safely.
        self.0.local.remove(&owner, "pending").await?;
        // This is an unreferenced encrypted scratch copy after pending removal.
        // Its cleanup failure cannot undo a confirmed commit or restore old keys.
        let _ = self
            .0
            .local
            .remove(&owner, &format!("proposal:{}", pending.operation_id))
            .await;
        self.update(|s| {
            s.pending_operation_id = None;
            s.remote_revision = Some(receipt.committed_revision.as_str().into());
            s.has_cloud_snapshot = Some(!pending.deleted);
            s.vault_id = (!pending.deleted).then(|| pending.vault_id.clone());
            s.key_id = if pending.deleted {
                None
            } else {
                pending.key_id.clone()
            };
        });
        Ok(())
    }

    pub(crate) fn cancel(&self, task_id: &str) -> SyncResult<EncryptedSyncStatus> {
        {
            // The identity check and flag update share begin/finish's status
            // lock. A delayed cancel can never poison the next task's flag.
            let status = self.0.status.lock().unwrap_or_else(|e| e.into_inner());
            if status.task_id.as_deref() != Some(task_id) {
                return Err(error("stale_task"));
            }
            self.0.cancelled.store(true, Ordering::Release);
        }
        Ok(self.status())
    }

    pub(crate) async fn lock(&self) -> SyncResult<EncryptedSyncStatus> {
        self.0.auto_suspended.store(true, Ordering::Release);
        self.0.cancelled.store(true, Ordering::Release);
        let mut runtime = self.0.runtime.lock().await;
        runtime.key = None;
        runtime.preview = None;
        runtime.settings.remember_key = false;
        let result = async {
            self.0.local.set_lockout(true).await?;
            self.initialize(&mut runtime).await?;
            runtime.settings.remember_key = false;
            runtime.settings.key_preference_epoch = runtime
                .settings
                .key_preference_epoch
                .checked_add(1)
                .ok_or_else(|| error("recovery_required"))?;
            // Persist the refusal to auto-unlock before asking the OS to delete
            // a key; a denied deletion must not unlock again on restart.
            self.save_settings(&runtime).await?;
            self.forget_unlock_material(&runtime).await
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn set_enabled(&self, enabled: bool) -> SyncResult<EncryptedSyncStatus> {
        if !enabled {
            self.0.auto_suspended.store(true, Ordering::Release);
            self.0.cancelled.store(true, Ordering::Release);
        } else {
            self.0.auto_suspended.store(false, Ordering::Release);
        }
        let mut runtime = self.0.runtime.lock().await;
        self.initialize(&mut runtime).await?;
        let result = async {
            if enabled {
                if runtime.settings.consent_version.as_deref() != Some(CONSENT_VERSION) {
                    return Err(error("consent_required"));
                }
                if runtime.preview.is_some() || runtime.baseline.is_none() {
                    return Err(error("restore_review_required"));
                }
                self.unlocked(&runtime)?;
                self.0.cancelled.store(false, Ordering::Release);
                self.current_account(&runtime).await?;
            }
            runtime.settings.enabled = enabled;
            self.save_settings(&runtime).await?;
            if enabled {
                self.0.wake.notify_one();
            }
            Ok(())
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    fn scope(&self, runtime: &Runtime, vault: &str, key: &str) -> SyncResult<SyncScope> {
        Ok(SyncScope {
            service_origin: self.origin(),
            owner_github_id: self.owner(runtime)?.into(),
            vault_id: vault.into(),
            key_id: key.into(),
            device_id: self.0.data.device().id.clone(),
        })
    }

    fn active_scope(&self, runtime: &Runtime) -> SyncResult<SyncScope> {
        let meta = self.metadata(runtime)?.value();
        self.scope(
            runtime,
            meta.vault_id
                .as_ref()
                .ok_or_else(|| error("cloud_deleted"))?
                .as_str(),
            meta.key_id
                .as_ref()
                .ok_or_else(|| error("cloud_deleted"))?
                .as_str(),
        )
    }

    fn baseline_documents(&self, runtime: &Runtime) -> SyncResult<Option<ValidatedSyncDocuments>> {
        runtime
            .baseline
            .as_ref()
            .filter(|b| {
                Some(&b.vault_id)
                    == runtime
                        .metadata
                        .as_ref()
                        .and_then(|m| m.value().vault_id.as_ref())
            })
            .map(|b| {
                documents::validate_sync_documents(b.documents.clone(), b.revision)
                    .map_err(document_error)
            })
            .transpose()
    }

    async fn make_preview(
        &self,
        runtime: &mut Runtime,
        remote: ValidatedSyncDocuments,
    ) -> SyncResult<RestorePreview> {
        let scope = self.active_scope(runtime)?;
        let local = self.0.data.export(scope.clone()).await?;
        let baseline = self.baseline_documents(runtime)?;
        let merged = documents::diff_sync_documents(baseline.as_ref(), &local.documents, &remote)
            .map_err(document_error)?;
        let conflicts = merged
            .conflicts()
            .iter()
            .map(|conflict| SyncConflictItem {
                id: conflict.conflict_id.clone(),
                kind: wire_name(&conflict.kind),
                reason: wire_name(&conflict.reason),
            })
            .collect();
        let mut counts = BTreeMap::new();
        for doc in &remote.documents().documents {
            *counts.entry(wire_name(&doc.kind)).or_insert(0) += 1;
        }
        let public = RestorePreview {
            preview_id: uuid::Uuid::new_v4().to_string(),
            unconfirmed_operation_id: self
                .reviewable_pending(runtime)
                .await?
                .map(|p| p.operation_id),
            observed_revision: remote.observed_revision().as_str().into(),
            local_generation: local.generation.as_str().into(),
            counts,
            device_settings_to_review: remote
                .documents()
                .documents
                .iter()
                .filter(|d| d.kind == DocumentKind::DeviceProfile)
                .map(|_| "device_profile".to_string())
                .take(1)
                .collect(),
            conflicts,
        };
        runtime.preview = Some(PreviewState {
            public: public.clone(),
            remote,
            local,
            baseline,
            vault_id: scope.vault_id.clone(),
            key_id: scope.key_id.clone(),
        });
        self.0.events.publish(
            None,
            BackendEventKind::CloudSyncConflictDetected(EncryptedSyncConflictEvent {
                sequence: self
                    .0
                    .sequence
                    .fetch_add(1, Ordering::AcqRel)
                    .saturating_add(1)
                    .to_string(),
                account_id: scope.owner_github_id,
                vault_id: scope.vault_id,
                task_id: self.status().task_id,
                preview: public.clone(),
            }),
        );
        Ok(public)
    }

    pub(crate) async fn preview_restore(
        &self,
        observed_revision: String,
    ) -> SyncResult<RestorePreview> {
        let observed = Revision::parse(&observed_revision).map_err(protocol_error)?;
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result = async {
            self.connect(&mut runtime).await?;
            self.reconcile_for_review(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            if self.metadata(&runtime)?.value().revision != observed {
                return Err(error("stale_preview"));
            }
            let key = self.unlocked(&runtime)?;
            let snapshot = self.download(&mut runtime).await?;
            let remote = self.decrypt(&runtime, snapshot.clone(), key).await?;
            runtime.snapshot = Some(snapshot);
            self.current_account(&runtime).await?;
            self.make_preview(&mut runtime, remote).await
        }
        .await;
        self.finish(&mut runtime, &result);
        result
    }

    pub(crate) async fn apply_restore(
        &self,
        preview_id: String,
        mode: RestoreMode,
        choices: Vec<SyncConflictChoice>,
    ) -> SyncResult<EncryptedSyncStatus> {
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result = async {
            self.current_account(&runtime).await?;
            self.unlocked(&runtime)?;
            self.refresh_metadata(&mut runtime).await?;
            let preview = runtime
                .preview
                .as_ref()
                .ok_or_else(|| error("stale_preview"))?;
            let scope = self.active_scope(&runtime)?;
            if preview.public.preview_id != preview_id
                || self.metadata(&runtime)?.value().revision.as_str()
                    != preview.public.observed_revision
                || self.0.data.generation()?.as_str() != preview.public.local_generation
                || scope.vault_id != preview.vault_id
                || scope.key_id != preview.key_id
            {
                return Err(error("stale_preview"));
            }
            let desired = match mode {
                RestoreMode::Replace => preview.remote.clone(),
                RestoreMode::Merge => {
                    let choices: Vec<documents::ConflictChoice> = choices
                        .into_iter()
                        .map(|c| documents::ConflictChoice {
                            conflict_id: c.id,
                            side: match c.side {
                                ConflictSide::Local => documents::ConflictSide::Local,
                                ConflictSide::Cloud => documents::ConflictSide::Remote,
                            },
                        })
                        .collect();
                    documents::diff_sync_documents(
                        preview.baseline.as_ref(),
                        &preview.local.documents,
                        &preview.remote,
                    )
                    .map_err(document_error)?
                    .resolve(&choices)
                    .map_err(document_error)?
                }
            };
            let remote = preview.remote.clone();
            let context = RestoreContext {
                scope: scope.clone(),
                operation_id: uuid::Uuid::new_v4().to_string(),
                observed_revision: remote.observed_revision(),
                local_generation: preview.local.generation,
                target_device: self.0.data.device(),
            };
            let reviewed_operation = preview.public.unconfirmed_operation_id.clone();
            self.current_account(&runtime).await?;
            self.retire_reviewed_pending(&mut runtime, reviewed_operation.as_deref())
                .await?;
            // After journal prepare, cancellation must finish commit/rollback.
            let applied = self.0.data.restore(desired, context).await?;
            let baseline = Baseline {
                revision: remote.observed_revision(),
                vault_id: UuidV4::parse(&scope.vault_id).map_err(protocol_error)?,
                documents: remote.documents().clone(),
            };
            self.0
                .local
                .write(&scope.owner_github_id, "baseline", baseline.clone())
                .await?;
            self.0
                .data
                .baseline(scope.clone(), baseline.documents.clone(), baseline.revision)
                .await?;
            runtime.baseline = Some(baseline);
            runtime.preview = None;
            runtime.settings.last_generation =
                if same_documents(applied.documents.documents(), remote.documents()) {
                    Some(applied.generation.as_str().into())
                } else {
                    None
                };
            if !self.0.cancelled.load(Ordering::Acquire) {
                runtime.settings.enabled = true;
            }
            self.save_settings(&runtime).await?;
            let ui_preferences = applied
                .documents
                .documents()
                .documents
                .iter()
                .filter(|d| d.kind == DocumentKind::UiPreferences)
                .filter_map(|d| d.value.as_str().map(|v| (d.id.clone(), v.to_owned())))
                .collect();
            self.0.events.publish(
                None,
                BackendEventKind::CloudSyncRestoreCompleted(EncryptedSyncRestoreEvent {
                    sequence: self
                        .0
                        .sequence
                        .fetch_add(1, Ordering::AcqRel)
                        .saturating_add(1)
                        .to_string(),
                    account_id: scope.owner_github_id,
                    vault_id: scope.vault_id,
                    task_id: self.status().task_id,
                    local_generation: applied.generation.as_str().into(),
                    ui_preferences,
                }),
            );
            self.check_cancelled()?;
            // A local conflict choice can differ from the current cloud head.
            // Use the same CAS/merge path to upload it, never claim it synced early.
            self.run_once(&mut runtime).await
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn sync_now(&self) -> SyncResult<EncryptedSyncStatus> {
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result = self.run_once(&mut runtime).await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    async fn run_once(&self, runtime: &mut Runtime) -> SyncResult<()> {
        self.connect(runtime).await?;
        self.reconcile(runtime).await?;
        if !runtime.settings.enabled {
            return Ok(());
        }
        if runtime.settings.consent_version.as_deref() != Some(CONSENT_VERSION) {
            return Err(error("consent_required"));
        }
        self.refresh_metadata(runtime).await?;
        if self.metadata(runtime)?.value().state != VaultState::Active {
            self.save_settings(runtime).await?;
            return Err(error("cloud_deleted"));
        }
        self.try_remembered(runtime).await?;
        let key = self.unlocked(runtime)?;
        let snapshot = self.download(runtime).await?;
        let remote = self.decrypt(runtime, snapshot.clone(), key.clone()).await?;
        runtime.snapshot = Some(snapshot.clone());
        let baseline = self.baseline_documents(runtime)?;
        if baseline.is_none() {
            self.make_preview(runtime, remote).await?;
            return Err(error("restore_review_required"));
        }
        let scope = self.active_scope(runtime)?;
        let mut captured = self.0.data.export(scope.clone()).await?;
        let merge = documents::diff_sync_documents(baseline.as_ref(), &captured.documents, &remote)
            .map_err(document_error)?;
        if !merge.conflicts().is_empty() {
            self.make_preview(runtime, remote).await?;
            return Err(error("conflict"));
        }
        let desired = merge.resolve(&[]).map_err(document_error)?;
        self.current_account(runtime).await?;
        if !same_documents(captured.documents.documents(), desired.documents()) {
            let context = RestoreContext {
                scope: scope.clone(),
                operation_id: uuid::Uuid::new_v4().to_string(),
                observed_revision: remote.observed_revision(),
                local_generation: captured.generation,
                target_device: self.0.data.device(),
            };
            captured = self.0.data.restore(desired, context).await?;
            self.emit_restored(&scope, &captured);
        }
        if same_documents(captured.documents.documents(), remote.documents()) {
            let baseline = Baseline {
                revision: remote.observed_revision(),
                vault_id: snapshot.vault_id.clone(),
                documents: remote.documents().clone(),
            };
            self.0
                .local
                .write(&scope.owner_github_id, "baseline", baseline.clone())
                .await?;
            self.0
                .data
                .baseline(scope, baseline.documents.clone(), baseline.revision)
                .await?;
            runtime.baseline = Some(baseline);
            runtime.settings.last_generation = Some(captured.generation.as_str().into());
            runtime.settings.last_success = Some(chrono::Utc::now().to_rfc3339());
            self.save_settings(runtime).await?;
            return Ok(());
        }
        self.current_account(runtime).await?;
        let documents = captured.documents.documents().clone();
        let context = EncryptContext {
            owner_github_id: snapshot.owner_github_id.clone(),
            vault_id: snapshot.vault_id.clone(),
            key_id: snapshot.key_id.clone(),
            base_revision: self.metadata(runtime)?.value().revision,
            operation_id: UuidV4::random().map_err(protocol_error)?,
            kind: UploadKind::Snapshot,
            kdf: snapshot.kdf.clone(),
        };
        let encrypted = tokio::task::spawn_blocking(move || {
            crypto::encrypt_snapshot(&documents, &context, &key).map_err(protocol_error)
        })
        .await
        .map_err(|_| error("crypto_worker_failed"))??;
        self.upload(runtime, encrypted, captured, None).await
    }

    pub(crate) async fn change_password(
        &self,
        current_password: String,
        new_password: String,
        confirmation: String,
        remember_key: bool,
    ) -> SyncResult<EncryptedSyncStatus> {
        let current_password = SecretInput::new(current_password);
        let new_password = SecretInput::new(new_password);
        let confirmation = SecretInput::new(confirmation);
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result = async {
            self.run_once(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            let previous = self.download(&mut runtime).await?;
            let metadata = self.metadata(&runtime)?.value().clone();
            if !runtime.baseline.as_ref().is_some_and(|baseline| {
                baseline.revision == metadata.revision && baseline.vault_id == previous.vault_id
            }) {
                // Another device advanced the head after run_once merged it.
                // Never encrypt the older local image against the newer CAS token.
                return Err(error("revision_conflict"));
            }
            let scope = self.active_scope(&runtime)?;
            let captured = self.0.data.export(scope).await?;
            let prior = previous.clone();
            let content = captured.documents.documents().clone();
            let (snapshot, key) = tokio::task::spawn_blocking(move || {
                let (_, _) = crypto::decrypt_snapshot_with_password(
                    &prior,
                    &metadata,
                    &prior.owner_github_id,
                    current_password,
                )
                .map_err(protocol_error)?;
                let password = NormalizedPassword::confirmed_new(new_password, confirmation)
                    .map_err(protocol_error)?;
                let context = EncryptContext {
                    owner_github_id: prior.owner_github_id.clone(),
                    vault_id: prior.vault_id.clone(),
                    key_id: UuidV4::random().map_err(protocol_error)?,
                    base_revision: metadata.revision,
                    operation_id: UuidV4::random().map_err(protocol_error)?,
                    kind: UploadKind::PasswordChange,
                    kdf: Kdf::fixed(Salt::random().map_err(protocol_error)?),
                };
                let key =
                    Arc::new(crypto::derive_key(&password, &context.kdf).map_err(protocol_error)?);
                let snapshot =
                    crypto::encrypt_snapshot(&content, &context, &key).map_err(protocol_error)?;
                Ok::<_, BackendError>((snapshot, key))
            })
            .await
            .map_err(|_| error("crypto_worker_failed"))??;
            self.current_account(&runtime).await?;
            let old_key = runtime.key.clone();
            let old_settings = runtime.settings.clone();
            // Both scoped key entries survive an unknown result; delete the old
            // remembered key only after the password-change receipt is verified.
            self.remember(&mut runtime, &snapshot, key, remember_key)
                .await?;
            runtime.snapshot = Some(previous.clone());
            let result = self
                .upload(
                    &mut runtime,
                    snapshot,
                    captured,
                    Some(previous.key_id.as_str().into()),
                )
                .await;
            if result.is_err() {
                let epoch = runtime.settings.key_preference_epoch;
                let remember = runtime.settings.remember_key;
                runtime.key = old_key;
                runtime.settings = old_settings;
                runtime.settings.key_preference_epoch = epoch;
                runtime.settings.remember_key = remember;
                self.save_settings(&runtime).await?;
            }
            result
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn delete_remote(
        &self,
        expected_vault_id: String,
        observed_revision: String,
        confirmed: bool,
    ) -> SyncResult<EncryptedSyncStatus> {
        if !confirmed {
            return Err(error("delete_confirmation_required"));
        }
        let observed = Revision::parse(&observed_revision).map_err(protocol_error)?;
        let expected = UuidV4::parse(&expected_vault_id).map_err(protocol_error)?;
        let mut runtime = self.0.runtime.try_lock().map_err(|_| error("busy"))?;
        self.begin();
        let result = async {
            self.connect(&mut runtime).await?;
            self.reconcile_for_review(&mut runtime).await?;
            self.refresh_metadata(&mut runtime).await?;
            let metadata = self.metadata(&runtime)?;
            if metadata.value().revision != observed
                || metadata.value().vault_id.as_ref() != Some(&expected)
                || metadata.value().state != VaultState::Active
            {
                return Err(error("revision_conflict"));
            }
            self.retire_reviewed_pending(&mut runtime, None).await?;
            let metadata = self.metadata(&runtime)?;
            let connection = runtime
                .connection
                .as_ref()
                .ok_or_else(|| error("sign_in_required"))?;
            let operation = connection
                .transport
                .prepare_delete(
                    &connection.session,
                    metadata,
                    UuidV4::random().map_err(protocol_error)?,
                )
                .map_err(protocol_error)?;
            let pending = Pending {
                record: String::from_utf8(operation.to_pending_record().map_err(protocol_error)?)
                    .map_err(|_| error("invalid_response"))?,
                operation_id: operation.operation_id().as_str().into(),
                vault_id: expected_vault_id,
                key_id: None,
                local_generation: self.0.data.generation()?,
                deleted: true,
                remember_key: false,
                key_preference_epoch: runtime.settings.key_preference_epoch,
                base_revision: observed,
                old_key_id: None,
            };
            self.0
                .local
                .write(self.owner(&runtime)?, "pending", pending.clone())
                .await?;
            self.update(|s| s.pending_operation_id = Some(pending.operation_id.clone()));
            self.current_account(&runtime).await?;
            let receipt = match connection
                .transport
                .submit(&connection.session, &operation)
                .await
            {
                Ok(receipt) => receipt,
                Err(e) if definite_rejection(&e) => {
                    self.discard_rejected(&runtime, &pending).await?;
                    return Err(protocol_error(e));
                }
                Err(_) => return Err(error("outcome_unknown")),
            };
            self.accept_receipt(&mut runtime, &pending, &receipt.receipt)
                .await
        }
        .await;
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn sign_out(&self) -> SyncResult<EncryptedSyncStatus> {
        self.0.signing_out.store(true, Ordering::Release);
        self.0.auto_suspended.store(true, Ordering::Release);
        self.0.cancelled.store(true, Ordering::Release);
        let service = self.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let result = service.sign_out_inner().await;
            let _ = sender.send(result);
        });
        receiver.await.map_err(|_| error("cancelled"))?
    }

    async fn sign_out_inner(&self) -> SyncResult<EncryptedSyncStatus> {
        use crate::domains::MarketplaceApi;
        let _sign_out_guard = SignOutGuard(&self.0.signing_out);
        let mut runtime = self.0.runtime.lock().await;
        let connection_is_custom = runtime.connection.as_ref().is_some_and(|connection| {
            matches!(&connection.credential, ConnectionCredential::CustomToken(_))
        });
        let connection_is_github = runtime.connection.as_ref().is_some_and(|connection| {
            matches!(&connection.credential, ConnectionCredential::Github(_))
        });
        let custom_target_configured = self.read_custom_target().await;
        let should_logout_github = !connection_is_custom
            && (connection_is_github
                || custom_target_configured
                    .as_ref()
                    .is_ok_and(|target| target.is_none())
                || custom_target_configured.is_err());
        if should_logout_github {
            // Preserve the account API's existing fail-closed guarantee before
            // reading potentially blocking secure storage. A custom-token
            // session must not revoke the unrelated Marketplace credential.
            self.0.marketplace.invalidate_authentication();
        }
        runtime.key = None;
        runtime.preview = None;
        runtime.settings.enabled = false;
        runtime.settings.remember_key = false;
        let connection = runtime.connection.take();
        // A custom-token session must never delete the marketplace's GitHub
        // credential: that identity is unrelated to this sync server and may
        // still be in active use by the unrelated plugin-marketplace feature.
        let logout_github = || async {
            self.0
                .marketplace
                .logout()
                .await
                .map_err(|_| error("secure_storage_denied"))
        };
        let logout_result: SyncResult<()> = if connection_is_custom
            || (!connection_is_github
                && custom_target_configured
                    .as_ref()
                    .is_ok_and(|target| target.is_some()))
        {
            self.clear_custom_server_config().await
        } else if connection_is_github
            || custom_target_configured
                .as_ref()
                .is_ok_and(|target| target.is_none())
        {
            logout_github().await
        } else {
            // If there is no active connection and the custom-store read failed,
            // remove both independent credentials rather than guessing which mode
            // was active. Never let a custom-store error skip GitHub cleanup.
            let custom_result = self.clear_custom_server_config().await;
            let github_result = logout_github().await;
            custom_result.and(github_result)
        };
        let cleanup_result = async {
            self.0.local.set_lockout(true).await?;
            self.initialize(&mut runtime).await?;
            runtime.settings.enabled = false;
            runtime.settings.remember_key = false;
            runtime.settings.key_preference_epoch = runtime
                .settings
                .key_preference_epoch
                .checked_add(1)
                .ok_or_else(|| error("recovery_required"))?;
            self.save_settings(&runtime).await?;
            self.forget_unlock_material(&runtime).await
        }
        .await;
        if let Some(Connection {
            transport,
            session,
            credential,
            ..
        }) = connection
        {
            drop(credential);
            // Local sign-out remains possible offline; the discarded session
            // also has a short server-enforced expiry.
            let _ = tokio::time::timeout(Duration::from_secs(5), transport.revoke_session(session))
                .await;
        }
        runtime.settings.enabled = false;
        runtime.settings.remember_key = false;
        runtime.metadata = None;
        runtime.snapshot = None;
        runtime.baseline = None;
        self.update(|s| {
            s.account = None;
            s.auth_state = AuthState::SignedOut;
            s.has_cloud_snapshot = None;
            s.vault_id = None;
            s.key_id = None;
            s.remote_revision = None;
            s.pending_operation_id = None;
        });
        let result = cleanup_result.and(logout_result);
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    /// Single-shot equivalent of the GitHub device-flow sign-in for a
    /// self-hosted server: there is no polling loop because the token
    /// exchange is one HTTP round trip, not an out-of-band user approval.
    /// Requires a custom token to already be saved (see `CloudSyncSection`'s
    /// save action) — this never falls back to a GitHub identity, so clicking
    /// it without a saved token fails clearly instead of silently doing the
    /// wrong thing.
    pub(crate) async fn sign_in_with_custom_token(&self) -> SyncResult<EncryptedSyncStatus> {
        log::warn!("[e2ee-token] sign_in_with_custom_token: entered");
        if self.read_custom_target().await?.is_none() {
            log::warn!("[e2ee-token] sign_in_with_custom_token: no custom token found in credential store, aborting before any network call");
            return Err(error("sign_in_required"));
        }
        log::warn!("[e2ee-token] sign_in_with_custom_token: token found, attempting to acquire runtime lock");
        let mut runtime = match self.0.runtime.try_lock() {
            Ok(runtime) => runtime,
            Err(_) => {
                log::warn!("[e2ee-token] sign_in_with_custom_token: runtime lock busy");
                return Err(error("busy"));
            }
        };
        self.begin();
        log::warn!("[e2ee-token] sign_in_with_custom_token: calling connect()");
        let result = self.connect(&mut runtime).await;
        match &result {
            Ok(()) => log::warn!("[e2ee-token] sign_in_with_custom_token: connect() succeeded"),
            Err(e) => log::warn!(
                "[e2ee-token] sign_in_with_custom_token: connect() failed: code={:?} message={}",
                e.code,
                e.message
            ),
        }
        self.finish(&mut runtime, &result);
        result.map(|()| self.status())
    }

    pub(crate) async fn begin_sign_in(&self) -> SyncResult<EncryptedSyncSignIn> {
        use crate::domains::MarketplaceApi;
        let flow = self
            .0
            .marketplace
            .start_device_flow()
            .await
            .map_err(|_| error("sign_in_required"))?;
        if flow.verification_uri != "https://github.com/login/device" {
            return Err(error("invalid_response"));
        }
        let expires = chrono::Utc::now()
            .checked_add_signed(chrono::Duration::seconds(
                i64::try_from(flow.expires_in_secs).map_err(|_| error("invalid_response"))?,
            ))
            .ok_or_else(|| error("invalid_response"))?;
        Ok(EncryptedSyncSignIn {
            authorization_session_id: flow.flow_id,
            user_code: flow.user_code,
            verification_uri: flow.verification_uri,
            expires_at: expires.to_rfc3339(),
            interval_seconds: flow.interval_secs,
        })
    }

    async fn reset_remote_state(
        &self,
        runtime: &mut Runtime,
        additional_owner: Option<&str>,
    ) -> SyncResult<()> {
        let mut owners = runtime.settings.known_owner_ids.clone();
        owners.extend(
            runtime
                .settings
                .known_accounts
                .iter()
                .map(|account| account.owner_id.clone()),
        );
        owners.extend(
            self.0
                .data
                .recovery_scopes()?
                .into_iter()
                .map(|scope| scope.owner_github_id),
        );
        if let Some(owner) = runtime.settings.owner_id.clone() {
            owners.push(owner);
        }
        if let Some(owner) = additional_owner {
            owners.push(owner.to_owned());
        }
        owners.retain(|owner| !owner.trim().is_empty());
        owners.sort();
        owners.dedup();

        let target_owner = additional_owner;
        if runtime.settings.owner_id.as_deref() != target_owner {
            self.forget_unlock_material(runtime).await?;
        }
        for owner in owners {
            if target_owner.is_some_and(|target| target == owner) {
                continue;
            }
            for record in ["baseline", "pending", "last-seen-revision"] {
                self.0.local.remove(&owner, record).await?;
            }
        }
        Self::invalidate_connection(runtime);
        runtime.settings.enabled = false;
        runtime.settings.consent_version = None;
        runtime.settings.remember_key = false;
        runtime.settings.last_success = None;
        runtime.settings.last_generation = None;
        runtime.settings.owner_id = None;
        runtime.settings.vault_id = None;
        runtime.settings.key_id = None;
        runtime.settings.key_preference_epoch = runtime
            .settings
            .key_preference_epoch
            .checked_add(1)
            .ok_or_else(|| error("recovery_required"))?;
        Ok(())
    }

    async fn forget_unlock_material(&self, runtime: &Runtime) -> SyncResult<()> {
        let Some(owner) = &runtime.settings.owner_id else {
            return Ok(());
        };
        if let (Some(vault), Some(key)) = (&runtime.settings.vault_id, &runtime.settings.key_id) {
            self.0.local.forget(owner, vault, key).await?;
        }
        // An interrupted password rotation can have two remembered keys. Lock
        // and sign-out clear both while retaining the encrypted replay record.
        if let Some(pending) = self.0.local.read::<Pending>(owner, "pending").await? {
            for key in [pending.key_id.as_ref(), pending.old_key_id.as_ref()]
                .into_iter()
                .flatten()
            {
                self.0.local.forget(owner, &pending.vault_id, key).await?;
            }
        }
        Ok(())
    }

    pub(crate) async fn poll_sign_in(
        &self,
        session: String,
    ) -> SyncResult<EncryptedSyncSignInResult> {
        use crate::domains::MarketplaceApi;
        use crate::domains::OAuthPollResult;
        match self
            .0
            .marketplace
            .poll_device_flow(session)
            .await
            .map_err(|_| error("sign_in_required"))?
        {
            OAuthPollResult::Authorized { .. } => {
                let (_, account) = self
                    .0
                    .marketplace
                    .sync_identity()
                    .await
                    .map_err(|_| error("sign_in_required"))?;
                Ok(EncryptedSyncSignInResult::SignedIn {
                    account: SyncAccount {
                        github_id: account.github_id.as_str().into(),
                        login: account.login,
                    },
                })
            }
            OAuthPollResult::Pending => Ok(EncryptedSyncSignInResult::Pending { slow_down: false }),
            OAuthPollResult::SlowDown => Ok(EncryptedSyncSignInResult::Pending { slow_down: true }),
            OAuthPollResult::Error { message }
                if message == "OAuth 设备码已过期，请重新发起登录" =>
            {
                Ok(EncryptedSyncSignInResult::Expired)
            }
            OAuthPollResult::Error { message }
                if message.contains("拒绝") || message.contains("取消") =>
            {
                Ok(EncryptedSyncSignInResult::Denied)
            }
            OAuthPollResult::Error { .. } => Err(error("sign_in_required")),
        }
    }

    pub(crate) async fn cancel_sign_in(&self, session: String) -> SyncResult<()> {
        use crate::domains::MarketplaceApi;
        self.0.marketplace.cancel_device_flow(Some(session)).await
    }

    pub(crate) async fn start(&self, spawner: Arc<dyn crate::TaskSpawner>) -> SyncResult<()> {
        if self.0.auto_started.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        {
            let mut runtime = self.0.runtime.lock().await;
            let result = self.initialize(&mut runtime).await;
            self.finish(&mut runtime, &result);
            if result.is_err() {
                self.0.auto_started.store(false, Ordering::Release);
            }
            result?;
        }
        let service = self.clone();
        spawner.spawn(Box::pin(async move {
            let mut changes = service.0.data.changes();
            // One startup trigger. Only a bounded local-busy retry may schedule
            // another attempt without a user change; no network polling timer.
            service.auto_sync(true).await;
            loop {
                let mut forced = tokio::select! {
                    changed = changes.changed() => { if changed.is_err() { break; } false },
                    _ = service.0.wake.notified() => true,
                };
                if service.0.shutdown.load(Ordering::Acquire) { break; }
                let first = Instant::now();
                let mut deadline = tokio::time::Instant::now() + Duration::from_millis(750);
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep_until(deadline) => break,
                        changed = changes.changed() => {
                            if changed.is_err() { return; }
                            let elapsed = first.elapsed();
                            if elapsed >= Duration::from_secs(5) { break; }
                            deadline = tokio::time::Instant::now() + Duration::from_millis(750).min(Duration::from_secs(5) - elapsed);
                        },
                        _ = service.0.wake.notified() => { if service.0.shutdown.load(Ordering::Acquire) { return; } forced = true; },
                    }
                }
                let change = *changes.borrow_and_update();
                if forced || matches!(change.origin, crate::cloud_sync_e2ee_store::gate::ChangeOrigin::User) { service.auto_sync(false).await; }
            }
        }));
        Ok(())
    }

    async fn auto_sync(&self, startup: bool) {
        // A LocalOnly writer can overlap the debounce/capture boundary without
        // emitting another dirty generation when it finishes. Retain this
        // trigger briefly instead of losing it after SourceChanged. This budget
        // applies only to local contention, never transport/CAS/unknown results.
        let mut delays = [250, 750, 1_500, 3_000].into_iter();
        let mut retry_sequence = None;
        loop {
            let mut runtime = self.0.runtime.lock().await;
            if self.0.shutdown.load(Ordering::Acquire)
                || self.0.auto_suspended.load(Ordering::Acquire)
                || !runtime.settings.enabled
                || runtime.preview.is_some()
            {
                return;
            }
            if retry_sequence.is_some_and(|sequence| {
                self.0.cancelled.load(Ordering::Acquire)
                    || self.0.sequence.load(Ordering::Acquire) != sequence
            }) {
                return;
            }
            if runtime.key.is_none() && !(startup && runtime.settings.remember_key) {
                return;
            }
            if runtime.retry_at.is_some_and(|at| at > Instant::now()) {
                return;
            }
            self.begin();
            let result = self.run_once(&mut runtime).await;
            let local_busy = result.as_ref().err().is_some_and(|failure| {
                failure.code == crate::BackendErrorCode::Busy
                    && matches!(
                        failure.message.as_str(),
                        "sync_documents_source_changed" | "runtime_busy"
                    )
            });
            self.finish(&mut runtime, &result);
            let completed_sequence = self.0.sequence.load(Ordering::Acquire);
            // Pause/lock/manual work must be able to acquire the runtime during
            // the delay. Every retry revalidates the current service state.
            drop(runtime);
            #[cfg(test)]
            self.0.runtime_release_count.fetch_add(1, Ordering::Release);
            if !local_busy || self.0.cancelled.load(Ordering::Acquire) {
                return;
            }
            let Some(delay) = delays.next() else {
                return;
            };
            // A later manual operation owns its own outcome. In particular an
            // old local retry must never reconcile that operation's unknown PUT.
            retry_sequence = Some(completed_sequence);
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.0.shutdown.store(true, Ordering::Release);
        self.0.cancelled.store(true, Ordering::Release);
        self.0.wake.notify_waiters();
        let mut runtime = self.0.runtime.lock().await;
        runtime.key = None;
        runtime.preview = None;
        runtime.connection = None;
    }

    async fn discard_rejected(&self, runtime: &Runtime, pending: &Pending) -> SyncResult<()> {
        let owner = self.owner(runtime)?;
        self.0.local.remove(owner, "pending").await?;
        let _ = self
            .0
            .local
            .remove(owner, &format!("proposal:{}", pending.operation_id))
            .await;
        self.update(|status| status.pending_operation_id = None);
        Ok(())
    }

    async fn reconcile_for_review(&self, runtime: &mut Runtime) -> SyncResult<()> {
        match self.reconcile(runtime).await {
            Err(e) if e.message == "outcome_unknown" => Ok(()),
            result => result,
        }
    }

    async fn reviewable_pending(&self, runtime: &Runtime) -> SyncResult<Option<Pending>> {
        let pending: Option<Pending> = self.0.local.read(self.owner(runtime)?, "pending").await?;
        if let Some(pending) = &pending {
            // Once a newer head is observed, the original If-Match can no
            // longer commit. Until then, only exact replay is safe.
            if self.metadata(runtime)?.value().revision <= pending.base_revision {
                return Err(error("outcome_unknown"));
            }
        }
        Ok(pending)
    }

    async fn retire_reviewed_pending(
        &self,
        runtime: &mut Runtime,
        expected_id: Option<&str>,
    ) -> SyncResult<()> {
        let pending = self.reviewable_pending(runtime).await?;
        if expected_id.is_some() && pending.as_ref().map(|p| p.operation_id.as_str()) != expected_id
        {
            return Err(error("stale_preview"));
        }
        if let Some(pending) = pending {
            self.current_account(runtime).await?;
            let head = self.metadata(runtime)?.value();
            for key_id in [pending.key_id.as_ref(), pending.old_key_id.as_ref()]
                .into_iter()
                .flatten()
            {
                let active = head.vault_id.as_ref().map(UuidV4::as_str)
                    == Some(pending.vault_id.as_str())
                    && head.key_id.as_ref().map(UuidV4::as_str) == Some(key_id.as_str());
                if !active {
                    self.0
                        .local
                        .forget(self.owner(runtime)?, &pending.vault_id, key_id)
                        .await?;
                }
            }
            // Explicit restore/create/delete supersedes the old CAS only after
            // review. Preserve the uncertain operation as protected evidence;
            // never mislabel it failed or committed without its receipt.
            self.0
                .local
                .write(
                    self.owner(runtime)?,
                    &format!("reviewed-uncertain:{}", pending.operation_id),
                    pending.clone(),
                )
                .await?;
            self.0.local.remove(self.owner(runtime)?, "pending").await?;
            self.update(|status| status.pending_operation_id = None);
        }
        Ok(())
    }

    pub(crate) async fn ui_preferences(&self) -> SyncResult<Option<crate::CloudSyncUiPreferences>> {
        Ok(self.ui_preferences_snapshot().await?.preferences)
    }

    pub(crate) async fn ui_preferences_snapshot(
        &self,
    ) -> SyncResult<EncryptedUiPreferencesSnapshot> {
        let Some(envelope) = self.0.local.read_ui().await? else {
            return Ok(EncryptedUiPreferencesSnapshot {
                preferences: None,
                revision: None,
            });
        };
        let preferences = serde_json::from_value(serde_json::json!({ "locale": envelope.value.expose().get("locale"), "fontScale": envelope.value.expose().get("fontScale") }))
            .map_err(|_| error("recovery_required"))?;
        Ok(EncryptedUiPreferencesSnapshot {
            preferences: Some(preferences),
            revision: Some(envelope.revision),
        })
    }

    fn emit_restored(&self, scope: &SyncScope, applied: &ExportedDocuments) {
        let ui_preferences = applied
            .documents
            .documents()
            .documents
            .iter()
            .filter(|d| d.kind == DocumentKind::UiPreferences)
            .filter_map(|d| d.value.as_str().map(|v| (d.id.clone(), v.to_owned())))
            .collect();
        self.0.events.publish(
            None,
            BackendEventKind::CloudSyncRestoreCompleted(EncryptedSyncRestoreEvent {
                sequence: self
                    .0
                    .sequence
                    .fetch_add(1, Ordering::AcqRel)
                    .saturating_add(1)
                    .to_string(),
                account_id: scope.owner_github_id.clone(),
                vault_id: scope.vault_id.clone(),
                task_id: self.status().task_id,
                local_generation: applied.generation.as_str().into(),
                ui_preferences,
            }),
        );
    }
}

fn wire_name(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn same_documents(left: &DocumentSet, right: &DocumentSet) -> bool {
    // Device/time metadata describes an export, not a user edit.
    left.documents == right.documents && left.tombstones == right.tombstones
}

fn definite_rejection(error: &crate::cloud_sync_e2ee_protocol::Error) -> bool {
    // Only for the first submit of a freshly generated operation ID. A retry of
    // an older unknown request cannot use a CAS rejection as proof of failure.
    matches!(
        error,
        crate::cloud_sync_e2ee_protocol::Error::Api {
            status: 400..=499,
            ..
        }
    )
}
