//! 对当前 ASR / LLM / Omni 的 HTTP origin 做无鉴权预热，把 TLS 握手付在听写之前。
//!
//! 不发 Key、不 POST 转写/润色。GET `origin/` 拿到任意 HTTP 响应（含 4xx）就算
//! 握手成功。本地 ASR、回环、WebSocket-only 渠道跳过。失败只打 debug，不弹 UI。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::net::{credential_http, origin_from_url};
use crate::persistence::{CredentialAccount, CredentialsVault};

const WARMUP_DEBOUNCE: Duration = Duration::from_millis(400);
const WARMUP_RESPONSE_TIMEOUT_PAD_SECS: u64 = 2;

static GENERATION: AtomicU64 = AtomicU64::new(0);
static IN_FLIGHT: Lazy<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>> =
    Lazy::new(|| Mutex::new(None));

/// 取消进行中的预热（换渠道去抖、进程退出）。
pub fn cancel_warmup() {
    let mut in_flight = IN_FLIGHT.lock();
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(handle) = in_flight.take() {
        handle.abort();
    }
}

/// 去抖后预热当前 HTTP 渠道。不挡调用方。
pub fn schedule_warmup() {
    let mut in_flight = IN_FLIGHT.lock();
    if let Some(handle) = in_flight.take() {
        handle.abort();
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    *in_flight = Some(tauri::async_runtime::spawn(async move {
        tokio::time::sleep(WARMUP_DEBOUNCE).await;
        if GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        warmup_active_origins().await;
    }));
}

async fn warmup_active_origins() {
    let origins = collect_active_http_origins();
    for origin in origins {
        warmup_origin(&origin).await;
    }
}

async fn warmup_origin(origin: &str) {
    let timeout = Duration::from_secs(
        crate::net::connect_timeout_secs().saturating_add(WARMUP_RESPONSE_TIMEOUT_PAD_SECS),
    );
    let url = format!("{origin}/");
    let result = credential_http().get(&url).timeout(timeout).send().await;
    match result {
        Ok(response) => {
            log::debug!(
                "[net-warmup] {} status={}",
                crate::net::sanitized_url_for_logs(origin),
                response.status().as_u16()
            );
        }
        Err(error) => {
            log::debug!(
                "[net-warmup] {} failed: {}",
                crate::net::sanitized_url_for_logs(origin),
                crate::net::request_error_kind(&error)
            );
        }
    }
}

fn collect_active_http_origins() -> Vec<String> {
    let mut origins = Vec::new();
    if let Some(origin) = asr_http_origin() {
        push_unique_origin(&mut origins, origin);
    }
    if let Some(origin) = llm_http_origin() {
        push_unique_origin(&mut origins, origin);
    }
    if let Some(origin) = omni_http_origin() {
        push_unique_origin(&mut origins, origin);
    }
    origins
}

fn push_unique_origin(origins: &mut Vec<String>, origin: String) {
    if origins.iter().any(|existing| existing == &origin) {
        return;
    }
    origins.push(origin);
}

fn asr_http_origin() -> Option<String> {
    let provider = CredentialsVault::get_active_asr();
    let model = CredentialsVault::get(CredentialAccount::AsrModel)
        .ok()
        .flatten()
        .unwrap_or_default();
    if !asr_provider_uses_http(&provider, &model) {
        return None;
    }
    let endpoint = CredentialsVault::get(CredentialAccount::AsrEndpoint)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| default_asr_endpoint(&provider))?;
    eligible_origin(&endpoint)
}

fn llm_http_origin() -> Option<String> {
    let provider = CredentialsVault::get_active_llm();
    if provider == crate::polish::CODEX_OAUTH_PROVIDER_ID {
        return eligible_origin(crate::polish::CODEX_DEFAULT_BASE_URL);
    }
    let endpoint = CredentialsVault::get(CredentialAccount::ArkEndpoint)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| default_llm_endpoint(&provider))?;
    eligible_origin(&endpoint)
}

fn omni_http_origin() -> Option<String> {
    let endpoint = CredentialsVault::get(CredentialAccount::OmniEndpoint)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())?;
    eligible_origin(&endpoint)
}

pub(crate) fn asr_provider_uses_http(provider: &str, model: &str) -> bool {
    if crate::asr::local::is_local_qwen3(provider)
        || crate::asr::local::is_local_whisper(provider)
        || provider == crate::asr::local::sherpa::PROVIDER_ID
        || provider == crate::asr::local::foundry::PROVIDER_ID
        || provider == crate::asr::local::APPLE_SPEECH_PROVIDER_ID
    {
        return false;
    }
    if provider == "volcengine"
        || provider == crate::asr::xfyun::PROVIDER_ID
        || provider == crate::asr::qwen_realtime::PROVIDER_ID
        || provider == crate::asr::stepfun_realtime::PROVIDER_ID
    {
        return false;
    }
    if provider == crate::asr::bailian::PROVIDER_ID {
        let model = model.trim();
        return crate::asr::dashscope_multimodal::protocol_for_model(model).is_some()
            && !model.contains("realtime");
    }
    if provider == "stepfun" && model.trim().ends_with("-stream") {
        return false;
    }
    true
}

pub(crate) fn eligible_origin(raw_url: &str) -> Option<String> {
    let origin = origin_from_url(raw_url)?;
    if crate::net::should_bypass_proxy(&origin, true) && is_loopback_origin(&origin) {
        return None;
    }
    Some(origin)
}

fn is_loopback_origin(origin: &str) -> bool {
    crate::net::should_bypass_proxy(origin, true)
}

fn default_asr_endpoint(provider: &str) -> Option<String> {
    if provider == crate::asr::elevenlabs::PROVIDER_ID {
        return Some(crate::asr::elevenlabs::DEFAULT_ENDPOINT.to_string());
    }
    if provider == crate::asr::mimo::PROVIDER_ID {
        return Some(crate::asr::mimo::DEFAULT_ENDPOINT.to_string());
    }
    if provider == "zenmux" {
        return Some(crate::asr::whisper::ZENMUX_DEFAULT_ENDPOINT.to_string());
    }
    None
}

fn default_llm_endpoint(provider: &str) -> Option<String> {
    if provider == "gemini" {
        return Some("https://generativelanguage.googleapis.com/v1beta".to_string());
    }
    if provider == crate::polish::CODEX_OAUTH_PROVIDER_ID {
        return Some(crate::polish::CODEX_DEFAULT_BASE_URL.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{asr_provider_uses_http, eligible_origin};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn skips_local_and_websocket_asr() {
        assert!(!asr_provider_uses_http("local-qwen3", ""));
        assert!(!asr_provider_uses_http("volcengine", ""));
        assert!(!asr_provider_uses_http("iflytek", ""));
        assert!(!asr_provider_uses_http("bailian-qwen3-realtime", ""));
        assert!(!asr_provider_uses_http("stepfun-realtime", ""));
        assert!(!asr_provider_uses_http("stepfun", "step-asr-1.1-stream"));
        assert!(asr_provider_uses_http("openai-compatible", ""));
        assert!(asr_provider_uses_http("xiaomi-mimo-asr", ""));
        assert!(asr_provider_uses_http("elevenlabs", ""));
        assert!(asr_provider_uses_http("zenmux", ""));
        assert!(asr_provider_uses_http("bailian", "qwen3-asr-flash"));
        assert!(!asr_provider_uses_http(
            "bailian",
            "qwen3-asr-flash-realtime"
        ));
    }

    #[test]
    fn eligible_origin_skips_loopback_and_ws() {
        assert_eq!(
            eligible_origin("https://api.openai.com/v1"),
            Some("https://api.openai.com".to_string())
        );
        assert_eq!(eligible_origin("http://127.0.0.1:8080/v1"), None);
        assert_eq!(eligible_origin("http://localhost:9000"), None);
        assert_eq!(eligible_origin("wss://example.com/ws"), None);
    }

    #[test]
    fn unique_origins_keep_first_copy() {
        let mut origins = Vec::new();
        super::push_unique_origin(&mut origins, "https://api.openai.com".to_string());
        super::push_unique_origin(&mut origins, "https://api.openai.com".to_string());
        super::push_unique_origin(&mut origins, "https://api.anthropic.com".to_string());
        assert_eq!(
            origins,
            vec![
                "https://api.openai.com".to_string(),
                "https://api.anthropic.com".to_string()
            ]
        );
    }

    #[test]
    fn cancel_warmup_is_safe_when_idle() {
        super::cancel_warmup();
        super::cancel_warmup();
    }

    #[test]
    fn later_schedule_invalidates_previous_generation() {
        super::schedule_warmup();
        let first = super::GENERATION.load(std::sync::atomic::Ordering::SeqCst);
        super::schedule_warmup();
        let second = super::GENERATION.load(std::sync::atomic::Ordering::SeqCst);
        assert!(second > first);
        super::cancel_warmup();
        assert!(super::IN_FLIGHT.lock().is_none());
    }

    #[test]
    fn schedule_then_cancel_clears_in_flight_handle() {
        super::schedule_warmup();
        super::cancel_warmup();
        assert!(super::IN_FLIGHT.lock().is_none());
    }

    #[tokio::test]
    async fn http_4xx_counts_as_warmup_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            stream
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        });
        let url = format!("http://{addr}/");
        let response = crate::net::credential_http()
            .get(&url)
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
        server.await.unwrap();
    }
}
