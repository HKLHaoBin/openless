//! ADB specialist helpers (debug kit branch). Bypass UI Window checks.

use serde::Deserialize;

use crate::commands::credentials::parse_account;
use crate::persistence::{CredentialAccount, CredentialsVault};

const LLM_EXTRA_HEADERS_ACCOUNT: &str = "ark.extra_headers";
const LLM_TEMPERATURE_ACCOUNT: &str = "ark.temperature";

#[derive(Debug, Deserialize)]
struct AdbCredsFile {
    active_asr: Option<String>,
    active_llm: Option<String>,
    credentials: Option<Vec<AdbCredEntry>>,
}

#[derive(Debug, Deserialize)]
struct AdbCredEntry {
    account: String,
    value: String,
    provider: Option<String>,
}

pub fn dump_log_to(dest_path: &str) -> String {
    dump_log_to_with_hint(dest_path, None)
}

pub fn dump_log_to_with_hint(dest_path: &str, app_files_dir: Option<&str>) -> String {
    let mut candidates = Vec::new();
    if let Some(dir) = app_files_dir {
        let base = std::path::PathBuf::from(dir);
        candidates.push(base.join("logs").join("openless.log"));
        candidates.push(base.join("openless.log"));
        if let Some(parent) = base.parent() {
            candidates.push(parent.join("logs").join("openless.log"));
        }
    }
    candidates.push(crate::log_dir_path().join("openless.log"));

    let src = candidates.into_iter().find(|path| path.exists());
    let Some(src) = src else {
        return format!(
            "DUMP_ERR log missing (tried filesDir/logs + log_dir_path); cold-start MainActivity first"
        );
    };
    match std::fs::copy(&src, std::path::Path::new(dest_path)) {
        Ok(_) => format!("DUMP_OK path={dest_path} src={}", src.display()),
        Err(error) => format!("DUMP_ERR copy failed: {error}"),
    }
}

pub fn set_credential(account: &str, value: &str, provider: Option<&str>) -> String {
    match set_credential_inner(account, value, provider) {
        Ok(()) => format!("SET_OK account={account}"),
        Err(error) => format!("SET_ERR account={account} err={error}"),
    }
}

fn set_credential_inner(account: &str, value: &str, provider: Option<&str>) -> Result<(), String> {
    if account == LLM_EXTRA_HEADERS_ACCOUNT {
        return CredentialsVault::set_active_llm_extra_headers_json(value).map_err(|e| e.to_string());
    }
    if account == LLM_TEMPERATURE_ACCOUNT {
        return CredentialsVault::set_active_llm_temperature(value).map_err(|e| e.to_string());
    }
    let acc = parse_account(account)?;
    if let Some(provider) = provider {
        if !matches!(
            acc,
            CredentialAccount::VolcengineAppKey
                | CredentialAccount::VolcengineAccessKey
                | CredentialAccount::VolcengineResourceId
                | CredentialAccount::AsrApiKey
                | CredentialAccount::AsrEndpoint
                | CredentialAccount::AsrModel
                | CredentialAccount::AsrVocabularyId
        ) {
            return Err("provider-scoped credential must be an ASR account".to_string());
        }
        CredentialsVault::set_for_asr_provider(provider, acc, value).map_err(|e| e.to_string())
    } else if value.is_empty() {
        CredentialsVault::remove(acc).map_err(|e| e.to_string())
    } else {
        CredentialsVault::set(acc, value).map_err(|e| e.to_string())
    }
}

pub fn set_asr_provider(provider: &str) -> String {
    if provider.is_empty() {
        return "SET_ERR provider empty".to_string();
    }
    if provider == crate::asr::local::PROVIDER_ID
        || provider == crate::asr::local::sherpa::PROVIDER_ID
        || provider == crate::asr::local::foundry::PROVIDER_ID
        || provider == crate::asr::local::APPLE_SPEECH_PROVIDER_ID
    {
        return format!("SET_ERR local ASR unavailable on mobile provider={provider}");
    }
    match CredentialsVault::set_active_asr_provider(provider) {
        Ok(()) => format!("SET_OK asr_provider={provider}"),
        Err(error) => format!("SET_ERR asr_provider={provider} err={error}"),
    }
}

pub fn set_llm_provider(provider: &str) -> String {
    if provider.is_empty() {
        return "SET_ERR provider empty".to_string();
    }
    match CredentialsVault::set_active_llm_provider(provider) {
        Ok(()) => format!("SET_OK llm_provider={provider}"),
        Err(error) => format!("SET_ERR llm_provider={provider} err={error}"),
    }
}

pub fn validate(kind: &str) -> String {
    let kind = kind.trim().to_ascii_lowercase();
    if kind != "asr" && kind != "llm" {
        return format!("VALIDATE_ERR unknown kind={kind}");
    }
    let result = tauri::async_runtime::block_on(async {
        crate::commands::validate_provider_credentials(kind.clone()).await
    });
    match result {
        Ok(_) => format!("VALIDATE_OK kind={kind}"),
        Err(error) => format!("VALIDATE_ERR kind={kind} err={error}"),
    }
}

pub fn apply_creds_json(path: &str) -> String {
    if path.is_empty() {
        return "APPLY_ERR path empty".to_string();
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return format!("APPLY_ERR read {path}: {error}"),
    };
    let file: AdbCredsFile = match serde_json::from_slice(&bytes) {
        Ok(file) => file,
        Err(error) => return format!("APPLY_ERR parse JSON: {error}"),
    };

    let mut errors = Vec::new();
    if let Some(provider) = file.active_asr.as_deref() {
        let line = set_asr_provider(provider);
        if !line.starts_with("SET_OK") {
            errors.push(line);
        }
    }
    if let Some(provider) = file.active_llm.as_deref() {
        let line = set_llm_provider(provider);
        if !line.starts_with("SET_OK") {
            errors.push(line);
        }
    }
    if let Some(entries) = file.credentials {
        for entry in entries {
            let line = set_credential(
                &entry.account,
                &entry.value,
                entry.provider.as_deref(),
            );
            if !line.starts_with("SET_OK") {
                errors.push(line);
            }
        }
    }

    if errors.is_empty() {
        "APPLY_OK".to_string()
    } else {
        format!("APPLY_ERR {}", errors.join(" | "))
    }
}
