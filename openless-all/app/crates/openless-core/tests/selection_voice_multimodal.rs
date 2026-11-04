//! Selection Voice's public workflow must not leak text work into the hidden LLM.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use openless_core::credentials::{
    CredentialNamespace, OMNI_API_KEY_ACCOUNT, OMNI_ENDPOINT_ACCOUNT, OMNI_MODEL_ACCOUNT,
};
use openless_core::shared_types::PipelineMode;
use openless_core::testing::FixtureSelectionRuntime;
use openless_core::{
    BackendConfig, BackendDependencies, BackendError, BackendRepositories, CredentialKey,
    CredentialStore, DictationContext, InMemoryCredentialStore, InsertOutcome, OpenLessBackend,
    PolishOutput, SecretValue, SelectionCapture, SelectionPolishOutputMode,
    SelectionVoiceEditRequest, SelectionVoiceInstructionRequest, SelectionVoiceIntent,
    SelectionVoiceIntentMode, SelectionVoiceManualIntent, SelectionVoiceRoute, SessionId,
    TextPolisher, TextStreamSink, UserPreferences, VoiceEditPlanRequest,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct TraditionalPolisher(Mutex<Vec<DictationContext>>);

impl TextPolisher for TraditionalPolisher {
    fn polish(
        &self,
        _session_id: SessionId,
        context: Arc<DictationContext>,
        _raw_text: String,
        _partials: Arc<dyn TextStreamSink>,
    ) -> BoxFuture<'static, Result<PolishOutput, BackendError>> {
        self.0.lock().unwrap().push((*context).clone());
        Box::pin(async { Ok(PolishOutput::text(PLAN)) })
    }

    fn cancel(&self, _session_id: SessionId) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async { Ok(()) })
    }
}

const PLAN: &str = "<edit_plan><summary>rename</summary><literal_replace><find>beta</find><replace>gamma</replace></literal_replace></edit_plan>";

fn preferences(provider: &str) -> UserPreferences {
    UserPreferences {
        multimodal_pipeline_enabled: true,
        pipeline_mode: PipelineMode::Multimodal,
        active_omni_provider: provider.into(),
        // An invalid unused LLM must not prevent Omni work, even during resolution.
        active_llm_provider: String::new(),
        selection_voice_intent_mode: SelectionVoiceIntentMode::Auto,
        selection_polish_output_mode: SelectionPolishOutputMode::DirectReplace,
        ..UserPreferences::default()
    }
}

fn backend(
    preferences: UserPreferences,
    credentials: Arc<dyn CredentialStore>,
    traditional: Arc<TraditionalPolisher>,
) -> (OpenLessBackend, std::path::PathBuf) {
    let data_dir = std::env::temp_dir().join(format!(
        "openless-selection-voice-omni-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let repositories = BackendRepositories::open(&data_dir).unwrap();
    repositories.preferences.set(preferences).unwrap();
    let mut dependencies = BackendDependencies::unsupported();
    dependencies.credential_store = credentials;
    dependencies.selection_polisher = Some(traditional);
    dependencies.selection_runtime = Some(Arc::new(FixtureSelectionRuntime::successful(
        SelectionCapture {
            text: "alpha beta".into(),
            source_app: None,
        },
        InsertOutcome::Inserted,
    )));
    let backend = OpenLessBackend::new_with_repositories(
        BackendConfig {
            data_dir: data_dir.clone(),
            ..BackendConfig::default()
        },
        dependencies,
        repositories,
    )
    .unwrap();
    (backend, data_dir)
}

async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("selection voice Omni request stalled")
}

// Real shared Omni HTTP adapter, not a mock of internal routing helpers.
async fn omni_server(
    provider: &str,
    responses: Vec<&str>,
) -> (
    Arc<InMemoryCredentialStore>,
    tokio::task::JoinHandle<Vec<(String, Value)>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let credentials = Arc::new(InMemoryCredentialStore::default());
    for (account, value) in [
        (OMNI_API_KEY_ACCOUNT, "omni-only-key"),
        (OMNI_ENDPOINT_ACCOUNT, endpoint.as_str()),
        (OMNI_MODEL_ACCOUNT, "selected-omni-model"),
    ] {
        credentials
            .write(
                CredentialKey::new(CredentialNamespace::Omni, Some(provider.into()), account)
                    .unwrap(),
                SecretValue::new(value),
            )
            .await
            .unwrap();
    }
    let gemini = provider == "gemini";
    let responses: Vec<String> = responses.into_iter().map(str::to_owned).collect();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response in responses {
            let (mut socket, _) = within(listener.accept()).await.unwrap();
            let mut request = Vec::new();
            let (header_end, length) = loop {
                let mut buffer = [0; 4096];
                let read = within(socket.read(&mut buffer)).await.unwrap();
                assert_ne!(read, 0, "HTTP request ended before its body");
                request.extend_from_slice(&buffer[..read]);
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break (end, length);
                    }
                }
            };
            requests.push((
                String::from_utf8(request[..header_end].to_vec()).unwrap(),
                serde_json::from_slice(&request[header_end + 4..header_end + 4 + length]).unwrap(),
            ));
            let body = if gemini {
                json!({"candidates": [{"content": {"parts": [{"text": response}]}}]})
            } else {
                json!({"choices": [{"message": {"content": response}}]})
            }
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body
            );
            within(socket.write_all(response.as_bytes())).await.unwrap();
        }
        requests
    });
    (credentials, server)
}

#[tokio::test]
async fn auto_intent_and_direct_edit_plan_use_selected_omni_without_an_llm() {
    for provider in ["openai-compatible", "gemini"] {
        let (credentials, server) = omni_server(
            provider,
            vec!["why change this?", "<intent>edit</intent>", PLAN],
        )
        .await;
        let traditional = Arc::new(TraditionalPolisher::default());
        let (backend, directory) = backend(preferences(provider), credentials, traditional.clone());
        let voice = &backend.services().selection_voice;
        let id = voice
            .begin(SelectionCapture {
                text: "alpha beta".into(),
                source_app: None,
            })
            .await
            .unwrap();
        voice.mark_processing(id).await.unwrap();
        let disposition = within(voice.process_transcript(id, "why change this?".into()))
            .await
            .unwrap();
        // The classifier's Edit beats the question heuristic.
        assert_eq!(disposition.intent(), Some(SelectionVoiceIntent::Edit));
        let route = within(voice.route_disposition(disposition)).await.unwrap();
        let SelectionVoiceRoute::ReadyToApply { preview } = route else {
            panic!("expected the core direct-replace preview");
        };
        assert_eq!(preview.text, "alpha gamma");
        assert_eq!(preview.summary.as_deref(), Some("rename"));
        let requests = within(server).await.unwrap();
        assert_eq!(requests.len(), 3);
        for (headers, body) in &requests {
            if provider == "gemini" {
                assert!(headers.contains("/models/selected-omni-model:generateContent"));
                assert!(headers
                    .to_ascii_lowercase()
                    .contains("x-goog-api-key: omni-only-key"));
            } else {
                assert!(headers.starts_with("POST /v1/chat/completions "));
                assert!(headers
                    .to_ascii_lowercase()
                    .contains("authorization: bearer omni-only-key"));
                assert_eq!(body["model"], "selected-omni-model");
                assert_eq!(body["stream"], false);
            }
        }
        assert!(requests[1].1.to_string().contains("意图分类"));
        assert!(requests[2].1.to_string().contains("语音编辑"));
        assert!(requests[2].1.to_string().contains("alpha beta"));
        assert!(traditional.0.lock().unwrap().is_empty());
        voice.cancel(Some(id)).await.unwrap();
        drop(backend);
        let _ = std::fs::remove_dir_all(directory);
    }
}

#[tokio::test]
async fn voice_edit_session_plan_uses_the_same_omni_route_and_field_context() {
    let (credentials, server) = omni_server("openai-compatible", vec![PLAN]).await;
    let traditional = Arc::new(TraditionalPolisher::default());
    let (backend, directory) = backend(
        preferences("openai-compatible"),
        credentials,
        traditional.clone(),
    );
    let output = within(
        backend
            .services()
            .selection_voice
            .voice_edit_plan(VoiceEditPlanRequest {
                session_id: SessionId::new(),
                field_context: "surrounding field".into(),
                draft: "alpha beta".into(),
                instruction_raw: "rename beta".into(),
                instruction_polished: "rename beta".into(),
            }),
    )
    .await
    .unwrap();
    assert_eq!(output.preview, "alpha gamma");
    let requests = within(server).await.unwrap();
    assert!(requests[0].1.to_string().contains("surrounding field"));
    assert_eq!(requests[0].1["model"], "selected-omni-model");
    assert!(traditional.0.lock().unwrap().is_empty());
    drop(backend);
    let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn selection_translation_keeps_its_target_prompt_on_the_omni_route() {
    let (credentials, server) = omni_server("openai-compatible", vec!["你好"]).await;
    let traditional = Arc::new(TraditionalPolisher::default());
    let (backend, directory) = backend(
        preferences("openai-compatible"),
        credentials,
        traditional.clone(),
    );
    let preview = within(backend.services().selection_voice.edit_preview(
        SelectionVoiceEditRequest {
            owner_session_id: SessionId::new(),
            capture: SelectionCapture {
                text: "hello".into(),
                source_app: None,
            },
            instruction: "翻译成中文".into(),
        },
    ))
    .await
    .unwrap();
    assert_eq!(preview.text, "你好");
    let requests = within(server).await.unwrap();
    let prompt = requests[0].1["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.contains("中文"));
    assert!(prompt.contains("翻译"));
    assert!(traditional.0.lock().unwrap().is_empty());
    drop(backend);
    let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn missing_omni_credentials_never_fall_back_to_a_configured_traditional_model() {
    let traditional = Arc::new(TraditionalPolisher::default());
    let mut prefs = preferences("openai-compatible");
    prefs.active_llm_provider = "deepseek".into();
    let (backend, directory) = backend(
        prefs,
        Arc::new(InMemoryCredentialStore::default()),
        traditional.clone(),
    );
    let error = backend
        .services()
        .selection_voice
        .edit_preview(SelectionVoiceEditRequest {
            owner_session_id: SessionId::new(),
            capture: SelectionCapture {
                text: "alpha beta".into(),
                source_app: None,
            },
            instruction: "rename beta".into(),
        })
        .await
        .unwrap_err();
    assert!(error.message.contains("Omni API key"), "{error}");
    assert!(traditional.0.lock().unwrap().is_empty());
    drop(backend);
    let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn transcription_only_omni_reports_its_text_limitation_without_using_an_llm() {
    let credentials = Arc::new(InMemoryCredentialStore::default());
    for (account, value) in [
        (OMNI_API_KEY_ACCOUNT, "transcribe-test-key"),
        (OMNI_ENDPOINT_ACCOUNT, "http://127.0.0.1:9/v1beta"),
        (
            OMNI_MODEL_ACCOUNT,
            openless_core::asr::gemini::DEFAULT_MODEL,
        ),
    ] {
        credentials
            .write(
                CredentialKey::new(CredentialNamespace::Omni, Some("gemini".into()), account)
                    .unwrap(),
                SecretValue::new(value),
            )
            .await
            .unwrap();
    }
    let traditional = Arc::new(TraditionalPolisher::default());
    let (backend, directory) = backend(preferences("gemini"), credentials, traditional.clone());
    let error = within(backend.services().selection_voice.edit_preview(
        SelectionVoiceEditRequest {
            owner_session_id: SessionId::new(),
            capture: SelectionCapture {
                text: "alpha beta".into(),
                source_app: None,
            },
            instruction: "rename beta".into(),
        },
    ))
    .await
    .unwrap_err();
    assert!(error.message.contains("requires audio"), "{error}");
    assert!(traditional.0.lock().unwrap().is_empty());
    drop(backend);
    let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn empty_selection_compose_continuation_also_uses_omni() {
    let (credentials, server) = omni_server("openai-compatible", vec!["A short email."]).await;
    let traditional = Arc::new(TraditionalPolisher::default());
    let (backend, directory) = backend(
        preferences("openai-compatible"),
        credentials,
        traditional.clone(),
    );
    let voice = &backend.services().selection_voice;
    let id = voice
        .begin(SelectionCapture {
            text: String::new(),
            source_app: None,
        })
        .await
        .unwrap();
    voice.mark_processing(id).await.unwrap();
    let disposition = voice
        .resolve_instruction(SelectionVoiceInstructionRequest {
            session_id: id,
            raw: "帮我写一封短邮件".into(),
            polished: "帮我写一封短邮件".into(),
            intent_mode: SelectionVoiceIntentMode::Manual,
            manual_intent: SelectionVoiceManualIntent::Edit,
            question_keywords: Vec::new(),
            auto_classification: None,
        })
        .await
        .unwrap();
    assert_eq!(disposition.intent(), Some(SelectionVoiceIntent::Compose));
    let route = within(voice.route_disposition(disposition)).await.unwrap();
    let SelectionVoiceRoute::ReadyToApply { preview } = route else {
        panic!("expected compose preview");
    };
    assert_eq!(preview.text, "A short email.");
    let requests = within(server).await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].1["model"], "selected-omni-model");
    assert!(traditional.0.lock().unwrap().is_empty());
    voice.cancel(Some(id)).await.unwrap();
    drop(backend);
    let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn traditional_and_disabled_multimodal_modes_keep_the_existing_polisher() {
    for (enabled, mode) in [
        (true, PipelineMode::Traditional),
        (false, PipelineMode::Multimodal),
    ] {
        let traditional = Arc::new(TraditionalPolisher::default());
        let mut prefs = preferences("");
        prefs.multimodal_pipeline_enabled = enabled;
        prefs.pipeline_mode = mode;
        prefs.active_llm_provider = "deepseek".into();
        let (backend, directory) = backend(
            prefs,
            Arc::new(InMemoryCredentialStore::default()),
            traditional.clone(),
        );
        let preview = backend
            .services()
            .selection_voice
            .edit_preview(SelectionVoiceEditRequest {
                owner_session_id: SessionId::new(),
                capture: SelectionCapture {
                    text: "alpha beta".into(),
                    source_app: None,
                },
                instruction: "rename beta".into(),
            })
            .await
            .unwrap();
        assert_eq!(preview.text, "alpha gamma");
        let calls = traditional.0.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].llm.provider_id, "deepseek");
        assert_eq!(calls[0].pipeline_mode, PipelineMode::Traditional);
        drop(calls);
        drop(backend);
        let _ = std::fs::remove_dir_all(directory);
    }
}
