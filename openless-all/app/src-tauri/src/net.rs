//! 共享 HTTP 客户端 + 带重试的请求发送。
//!
//! 背景：原先每个网络命令各自 `reqwest::Client::new()`，连接池互不复用 —— 一次
//! 成功的 TLS 连接用完即弃，下一个命令又得重新握手。在握手不稳定的网络下（代理
//! 分流等）首次握手经常被重置，用户得反复重试才能用。
//!
//! 这里提供两件东西：
//! - `http()`：进程级共享客户端。一次握手成功后的连接进连接池，后续命令直接复用，
//!   不再付握手成本。
//! - `send_with_retry`：只对**连接层失败**（`is_connect()` —— 握手重置 / 连接被拒
//!   等）做指数退避重试。这类失败发生在请求送达服务端之前、且通常是瞬时的（代理
//!   分流抖动等），重试既幂等安全又有意义。**不重试超时与其他请求层错误**：超时
//!   可能发生在服务端已收到之后（重试 POST / DELETE 会重复执行）；`is_request()`
//!   类错误多为确定性失败（如 endpoint 配置错误），重试只是徒增数秒延迟。HTTP
//!   4xx/5xx 同样不重试 —— 服务端已应答，状态码交给调用方判断。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

/// 用户是否允许 app 使用系统代理（issue #869）。默认 true = 跟随系统代理，
/// 与历史行为一致；关闭后所有 reqwest 客户端 `.no_proxy()` 直连。
/// 启动时由 coordinator 用持久化设置初始化，`set_settings` 变更时同步。
static USE_SYSTEM_PROXY: AtomicBool = AtomicBool::new(true);

pub(crate) const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 8;
pub(crate) const DEFAULT_POOL_IDLE_TIMEOUT_SECS: u64 = 300;
pub(crate) const DEFAULT_REQUEST_TIMEOUT_FLOOR_SECS: u64 = 30;

const MIN_CONNECT_TIMEOUT_SECS: u64 = 5;
const MAX_CONNECT_TIMEOUT_SECS: u64 = 60;
const MIN_POOL_IDLE_TIMEOUT_SECS: u64 = 60;
const MAX_POOL_IDLE_TIMEOUT_SECS: u64 = 1800;
const MIN_REQUEST_TIMEOUT_FLOOR_SECS: u64 = 15;
const MAX_REQUEST_TIMEOUT_FLOOR_SECS: u64 = 300;

static CONNECT_TIMEOUT_SECS: AtomicU64 = AtomicU64::new(DEFAULT_CONNECT_TIMEOUT_SECS);
static POOL_IDLE_TIMEOUT_SECS: AtomicU64 = AtomicU64::new(DEFAULT_POOL_IDLE_TIMEOUT_SECS);
static REQUEST_TIMEOUT_FLOOR_SECS: AtomicU64 = AtomicU64::new(DEFAULT_REQUEST_TIMEOUT_FLOOR_SECS);

/// 共享 / provider 客户端的构建缓存。key = `(discriminator, no_proxy 决策)`。
/// 代理开关或连接池参数变化时整表清空重建，保证「存盘即生效」。
static CACHE: Lazy<Mutex<HashMap<(u64, bool), reqwest::Client>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 当前是否使用系统代理（false = 所有请求直连）。
pub(crate) fn use_system_proxy() -> bool {
    USE_SYSTEM_PROXY.load(Ordering::Relaxed)
}

/// 更新系统代理开关并清空客户端缓存，让后续请求立即按新策略重建连接池。
/// 在启动初始化与 `set_settings` 中设置值变化时调用。
pub(crate) fn set_use_system_proxy(enabled: bool) {
    USE_SYSTEM_PROXY.store(enabled, Ordering::Relaxed);
    CACHE.lock().clear();
}

pub(crate) fn clamp_connect_timeout_secs(secs: u64) -> u64 {
    secs.clamp(MIN_CONNECT_TIMEOUT_SECS, MAX_CONNECT_TIMEOUT_SECS)
}

pub(crate) fn clamp_pool_idle_timeout_secs(secs: u64) -> u64 {
    secs.clamp(MIN_POOL_IDLE_TIMEOUT_SECS, MAX_POOL_IDLE_TIMEOUT_SECS)
}

pub(crate) fn clamp_request_timeout_floor_secs(secs: u64) -> u64 {
    secs.clamp(
        MIN_REQUEST_TIMEOUT_FLOOR_SECS,
        MAX_REQUEST_TIMEOUT_FLOOR_SECS,
    )
}

/// 同步连接层超时。connect / idle 是 Client 构建时定死的，变了必须清池；
/// 请求下限只影响单次 `RequestBuilder.timeout` 与听写动态公式，不清池。
pub(crate) fn set_http_timeouts(connect_secs: u64, idle_secs: u64, request_floor_secs: u64) {
    let connect_secs = clamp_connect_timeout_secs(connect_secs);
    let idle_secs = clamp_pool_idle_timeout_secs(idle_secs);
    let request_floor_secs = clamp_request_timeout_floor_secs(request_floor_secs);
    let connect_changed =
        CONNECT_TIMEOUT_SECS.swap(connect_secs, Ordering::Relaxed) != connect_secs;
    let idle_changed = POOL_IDLE_TIMEOUT_SECS.swap(idle_secs, Ordering::Relaxed) != idle_secs;
    REQUEST_TIMEOUT_FLOOR_SECS.store(request_floor_secs, Ordering::Relaxed);
    if connect_changed || idle_changed {
        CACHE.lock().clear();
    }
}

pub(crate) fn connect_timeout_secs() -> u64 {
    CONNECT_TIMEOUT_SECS.load(Ordering::Relaxed)
}

pub(crate) fn pool_idle_timeout_secs() -> u64 {
    POOL_IDLE_TIMEOUT_SECS.load(Ordering::Relaxed)
}

pub(crate) fn request_timeout_floor_secs() -> u64 {
    REQUEST_TIMEOUT_FLOOR_SECS.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) struct HttpTimeoutTestGuard {
    _lock: parking_lot::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl HttpTimeoutTestGuard {
    pub(crate) fn lock() -> Self {
        static LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
        Self { _lock: LOCK.lock() }
    }
}

#[cfg(test)]
impl Drop for HttpTimeoutTestGuard {
    fn drop(&mut self) {
        set_http_timeouts(
            DEFAULT_CONNECT_TIMEOUT_SECS,
            DEFAULT_POOL_IDLE_TIMEOUT_SECS,
            DEFAULT_REQUEST_TIMEOUT_FLOOR_SECS,
        );
    }
}

/// 判定某 base_url 是否应绕过系统代理：回环地址恒绕过（localhost 走代理没有
/// 意义且可能自环）；全局关闭系统代理时所有地址绕过（issue #869）。
pub(crate) fn should_bypass_proxy(base_url: &str, use_system_proxy: bool) -> bool {
    !use_system_proxy || is_loopback_url(base_url)
}

fn is_loopback_url(base_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(base_url.trim()) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    // url crate 对 IPv6 host 返回带方括号的形式（"[::1]"），解析前剥掉。
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// 共享客户端的基础 builder：握手限时 + 连接池 + UA；按需禁用系统代理。
/// AI 出站（转写 / 润色 / 验证）都走这里，避免为 timeout 各建一套池（issue #998）。
pub(crate) fn base_client_builder(no_proxy: bool) -> reqwest::ClientBuilder {
    let mut builder = reqwest::Client::builder()
        // 握手单独限时：卡在握手上要尽快失败，好让 send_with_retry 立即重试。
        .connect_timeout(Duration::from_secs(connect_timeout_secs()))
        // 连接池：空闲连接默认保留 5 分钟，听写间隔几分钟也不必再付 TLS。
        .pool_idle_timeout(Duration::from_secs(pool_idle_timeout_secs()))
        .pool_max_idle_per_host(8)
        .tcp_keepalive(Duration::from_secs(30))
        .user_agent(concat!("OpenLess/", env!("CARGO_PKG_VERSION")));
    if no_proxy {
        builder = builder.no_proxy();
    }
    builder
}

/// 从用户配置的 base_url 抽出 `scheme://host[:port]`，供预热与 origin 去重。
pub(crate) fn origin_from_url(raw_url: &str) -> Option<String> {
    let Ok(url) = reqwest::Url::parse(raw_url.trim()) else {
        return None;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.trim();
    if host.is_empty() {
        return None;
    }
    let mut origin = format!("{}://{}", url.scheme(), host);
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Some(origin)
}

/// 进程级共享 HTTP 客户端。带连接池 —— 一次握手成功后的连接被后续请求复用；
/// 代理开关切换后经 CACHE 清空自动按新策略重建。
pub fn http() -> reqwest::Client {
    let no_proxy = !use_system_proxy();
    cached_client((0, no_proxy), || {
        base_client_builder(no_proxy)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// HTTP client for requests carrying OAuth device credentials or bearer tokens.
/// Redirects are disabled so secrets are never replayed to a different origin.
pub fn credential_http() -> reqwest::Client {
    let no_proxy = !use_system_proxy();
    cached_client((1, no_proxy), || {
        base_client_builder(no_proxy)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect credential HTTP client")
    })
}

/// Anonymous HTTP client for public endpoints that must fail closed on redirects.
pub fn anonymous_no_redirect_http() -> reqwest::Client {
    let no_proxy = !use_system_proxy();
    cached_client((2, no_proxy), || {
        base_client_builder(no_proxy)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build anonymous no-redirect HTTP client")
    })
}

/// 按 `(timeout_secs, no_proxy)` 缓存并复用 `reqwest::Client`。
///
/// LLM / ASR provider 过去每次请求都新建一个 `reqwest::Client`，新客户端连接池是
/// 空的 —— 于是每句话都要重新 TLS 握手（~100–300ms）。这里把建好的客户端按其配置
/// 缓存：相同配置的后续 provider 直接 `clone()` 复用同一连接池（`reqwest::Client`
/// 内部是 `Arc`，clone 共享连接池与配置），握手成本只在首次付一次。
///
/// `build` 只在首次 miss 时调用，必须产出与该 `key` 语义一致的客户端。
pub fn cached_client<F>(key: (u64, bool), build: F) -> reqwest::Client
where
    F: FnOnce() -> reqwest::Client,
{
    CACHE.lock().entry(key).or_insert_with(build).clone()
}

/// Render a user-configured URL for logs without credentials or secret-bearing components.
pub(crate) fn sanitized_url_for_logs(raw_url: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(raw_url.trim()) else {
        return "<invalid-url>".to_string();
    };
    if !matches!(url.scheme(), "http" | "https")
        || url.set_username("").is_err()
        || url.set_password(None).is_err()
    {
        return "<invalid-url>".to_string();
    }
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

/// Stable diagnostic category for a reqwest failure. Unlike `Display`, this never embeds its URL.
pub(crate) fn request_error_kind(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connection"
    } else if error.is_body() || error.is_decode() {
        "response-body"
    } else {
        "request"
    }
}

/// 单次请求最多尝试的次数。失败本身很快（握手重置 ~0.5s），10 次总耗时仍可控。
const MAX_ATTEMPTS: u32 = 10;

/// 发送请求，只对连接层失败（`is_connect()`：握手重置 / 连接被拒等）做指数退避重试。
///
/// `make` 每次尝试都重新构造 `RequestBuilder`（`send()` 会消耗它）。只重试
/// `is_connect()` —— 连接尚未建立、请求未送达服务端，且这类失败通常是瞬时的，
/// 重试幂等安全且有价值。超时（可能服务端已在处理）与其他 `is_request()` 类错误
/// （多为 endpoint 配置错误等确定性失败）都不重试。拿到任意 HTTP 响应（含
/// 4xx/5xx）即返回，状态码由调用方自行判断。
pub async fn send_with_retry<F>(make: F) -> reqwest::Result<reqwest::Response>
where
    F: Fn() -> reqwest::RequestBuilder,
{
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        match make().send().await {
            Ok(resp) => return Ok(resp),
            Err(err) => {
                let retryable = err.is_connect();
                if !retryable || attempt >= MAX_ATTEMPTS {
                    return Err(err);
                }
                // 150 / 300 / 600 / 900 / 900 … ms 退避。
                let backoff = (150u64 * 2u64.pow((attempt - 1).min(3))).min(900);
                let failure = request_error_kind(&err);
                log::warn!(
                    "[net] transient {failure} failure (attempt {attempt}/{MAX_ATTEMPTS}), retry in {backoff}ms"
                );
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{credential_http, sanitized_url_for_logs};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn proxy_bypass_decision_is_pure() {
        use super::should_bypass_proxy;
        // 回环地址无论系统代理开关如何都绕过。
        for url in [
            "http://localhost:9000/v1",
            "http://127.0.0.1:8080",
            "http://[::1]:8080",
        ] {
            assert!(
                should_bypass_proxy(url, true),
                "{url} should bypass when system proxy is on"
            );
            assert!(
                should_bypass_proxy(url, false),
                "{url} should bypass when system proxy is off"
            );
        }
        // 公开 host：开启系统代理时跟随代理，关闭时直连。
        assert!(!should_bypass_proxy("https://api.example.com/v1", true));
        assert!(should_bypass_proxy("https://api.example.com/v1", false));
        // 非法 URL 判为不可解析：开关开时不绕过，全局关闭时一律绕过。
        assert!(!should_bypass_proxy("not a url", true));
        assert!(should_bypass_proxy("not a url", false));
    }

    #[test]
    fn system_proxy_toggle_updates_flag_and_rebuilds_shared_client() {
        use super::{http, set_use_system_proxy, use_system_proxy, CACHE};
        set_use_system_proxy(true);
        CACHE.lock().clear();
        let _ = http();
        assert!(!CACHE.lock().is_empty());
        set_use_system_proxy(false);
        assert!(!use_system_proxy());
        // 下一次 http() 按「直连」决策重建（key 的 bool 位 = no_proxy）。
        let _ = http();
        assert!(CACHE.lock().contains_key(&(0, true)));
        set_use_system_proxy(true);
        assert!(use_system_proxy());
    }

    #[tokio::test]
    async fn credential_client_never_follows_redirects_or_forwards_bearer() {
        let redirect_target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_url = format!("http://{}", redirect_target.local_addr().unwrap());
        let source = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let source_url = format!("http://{}", source.local_addr().unwrap());
        let source_task = tokio::spawn(async move {
            let (mut stream, _) = source.accept().await.unwrap();
            let mut request = [0u8; 2048];
            let read = stream.read(&mut request).await.unwrap();
            assert!(String::from_utf8_lossy(&request[..read])
                .to_ascii_lowercase()
                .contains("authorization: bearer gho_redirect_test"));
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let response = credential_http()
            .get(source_url)
            .bearer_auth("gho_redirect_test")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
        assert!(
            tokio::time::timeout(Duration::from_millis(150), redirect_target.accept())
                .await
                .is_err()
        );
        source_task.await.unwrap();
    }

    #[test]
    fn log_url_removes_userinfo_query_and_fragment() {
        let rendered = sanitized_url_for_logs(
            "https://alice:password@example.com:8443/v1/models?token=secret#private",
        );
        assert_eq!(rendered, "https://example.com:8443/v1/models");
        for secret in ["alice", "password", "token", "secret", "private"] {
            assert!(!rendered.contains(secret), "log URL leaked {secret}");
        }
    }

    #[test]
    fn log_url_never_echoes_malformed_input() {
        assert_eq!(
            sanitized_url_for_logs("not a URL?token=secret#private"),
            "<invalid-url>"
        );
    }

    #[test]
    fn origin_from_url_strips_path_and_keeps_explicit_port() {
        assert_eq!(
            super::origin_from_url("https://api.openai.com/v1/audio/transcriptions"),
            Some("https://api.openai.com".to_string())
        );
        assert_eq!(
            super::origin_from_url("http://127.0.0.1:8080/v1"),
            Some("http://127.0.0.1:8080".to_string())
        );
        assert_eq!(super::origin_from_url("wss://example.com/ws"), None);
        assert_eq!(super::origin_from_url("not a url"), None);
    }

    #[test]
    fn http_timeout_prefs_clamp_and_rebuild_pool() {
        use super::{
            clamp_connect_timeout_secs, clamp_pool_idle_timeout_secs,
            clamp_request_timeout_floor_secs, credential_http, http, set_http_timeouts,
            set_use_system_proxy, HttpTimeoutTestGuard, CACHE,
        };
        let _guard = HttpTimeoutTestGuard::lock();
        set_use_system_proxy(true);
        CACHE.lock().clear();
        let _ = http();
        let _ = credential_http();
        assert!(CACHE.lock().contains_key(&(0, false)));
        assert!(CACHE.lock().contains_key(&(1, false)));
        set_http_timeouts(15, 600, 45);
        assert!(CACHE.lock().is_empty());
        assert_eq!(super::connect_timeout_secs(), 15);
        assert_eq!(super::pool_idle_timeout_secs(), 600);
        assert_eq!(super::request_timeout_floor_secs(), 45);
        assert_eq!(clamp_connect_timeout_secs(1), 5);
        assert_eq!(clamp_connect_timeout_secs(90), 60);
        assert_eq!(clamp_pool_idle_timeout_secs(10), 60);
        assert_eq!(clamp_pool_idle_timeout_secs(9999), 1800);
        assert_eq!(clamp_request_timeout_floor_secs(5), 15);
        assert_eq!(clamp_request_timeout_floor_secs(900), 300);
    }

    #[test]
    fn request_timeout_floor_change_does_not_rebuild_pool() {
        use super::{
            credential_http, set_http_timeouts, set_use_system_proxy, HttpTimeoutTestGuard, CACHE,
        };
        let _guard = HttpTimeoutTestGuard::lock();
        set_use_system_proxy(true);
        CACHE.lock().clear();
        let _ = credential_http();
        assert!(CACHE.lock().contains_key(&(1, false)));
        set_http_timeouts(8, 300, 60);
        assert!(CACHE.lock().contains_key(&(1, false)));
        assert_eq!(super::request_timeout_floor_secs(), 60);
    }

    #[test]
    fn shared_ai_clients_are_cloned_from_the_same_cache_slot() {
        use super::{credential_http, set_use_system_proxy, HttpTimeoutTestGuard, CACHE};
        let _guard = HttpTimeoutTestGuard::lock();
        set_use_system_proxy(true);
        CACHE.lock().clear();
        let first = credential_http();
        let second = credential_http();
        drop((first, second));
        assert_eq!(CACHE.lock().keys().filter(|key| key.0 == 1).count(), 1);
    }
}
