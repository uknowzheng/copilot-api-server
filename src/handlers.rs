use axum::{
    extract::State,
    response::{sse::KeepAlive, IntoResponse, Sse},
    Json,
};
use copilot_sdk::{SessionConfig, SessionEventData};
use tracing::{error, info};

use crate::{
    error::AppError,
    prompt::build_prompt,
    state::AppState,
    streaming::{make_id, now_ts, stream_chat},
    types::{
        ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, FunctionCall,
        MessageContent, ModelCapabilitiesView, ModelList, ModelObject, ToolCall, Usage,
    },
};

/// GET /v1/models
pub async fn list_models(State(state): State<AppState>) -> Result<Json<ModelList>, AppError> {
    let created = now_ts();
    let owned = "github-copilot".to_string();

    let models = state
        .client
        .list_models()
        .await
        .map_err(|e| AppError::Upstream(format!("list_models: {e}")))?;

    let data = models
        .into_iter()
        .map(|m| {
            let caps = ModelCapabilitiesView {
                vision: m.capabilities.supports.vision,
                reasoning_effort: m.capabilities.supports.reasoning_effort,
                max_prompt_tokens: m.capabilities.limits.max_prompt_tokens,
                max_context_window_tokens: m.capabilities.limits.max_context_window_tokens,
            };
            ModelObject {
                id: m.id,
                object: "model",
                created,
                owned_by: owned.clone(),
                display_name: Some(m.name),
                capabilities: Some(caps),
                billing_multiplier: m.billing.map(|b| b.multiplier),
                policy_state: m.policy.map(|p| p.state),
                supported_reasoning_efforts: m.supported_reasoning_efforts,
                default_reasoning_effort: m.default_reasoning_effort,
            }
        })
        .collect();

    Ok(Json(ModelList { object: "list", data }))
}

/// POST /v1/chat/completions
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(mut req): Json<ChatCompletionRequest>,
) -> Result<axum::response::Response, AppError> {
    if req.messages.is_empty() {
        return Err(AppError::BadRequest("messages must not be empty".into()));
    }
    if req.model.trim().is_empty() {
        req.model = state.config.default_model.clone();
    }

    info!(model = %req.model, stream = req.stream, n_msgs = req.messages.len(), "chat request");

    if req.stream {
        let s = stream_chat(state, req);
        let sse = Sse::new(s).keep_alive(KeepAlive::default());
        Ok(sse.into_response())
    } else {
        let resp = chat_blocking(state, req).await?;
        Ok(Json(resp).into_response())
    }
}

/// 非流式：聚合所有 delta，返回完整响应。
async fn chat_blocking(
    state: AppState,
    req: ChatCompletionRequest,
) -> Result<ChatCompletionResponse, AppError> {
    let id = make_id();
    let model = req.model.clone();
    let created = now_ts();
    let prompt = build_prompt(&req.messages);

    // 构造 SessionConfig
    let mut tools = Vec::new();
    if let Some(client_tools) = &req.tools {
        for t in client_tools {
            let mut tool = copilot_sdk::Tool::new(&t.function.name);
            if let Some(desc) = &t.function.description {
                tool = tool.description(desc);
            }
            if let Some(params) = &t.function.parameters {
                tool = tool.schema(params.clone());
            }
            tools.push(tool);
        }
    }
    let cfg = SessionConfig {
        tools,
        model: Some(req.model.clone()),
        streaming: false,
        ..Default::default()
    };

    let session = state
        .client
        .create_session(cfg)
        .await
        .map_err(|e| AppError::Upstream(format!("create_session: {e}")))?;

    let mut events = session.subscribe();
    session
        .send(prompt.as_str())
        .await
        .map_err(|e| AppError::Upstream(format!("send: {e}")))?;

    let mut content = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut usage = Usage::default();
    let mut finish_reason = "stop".to_string();

    loop {
        match events.recv().await {
            Ok(event) => match &event.data {
                SessionEventData::AssistantMessageDelta(d) => content.push_str(&d.delta_content),
                SessionEventData::AssistantMessage(m) if content.is_empty() => {
                    content = m.content.clone();
                }
                SessionEventData::ToolExecutionStart(t) => {
                    finish_reason = "tool_calls".to_string();
                    let args = serde_json::to_string(&t.arguments)
                        .unwrap_or_else(|_| "{}".to_string());
                    tool_calls.push(ToolCall {
                        id: t.tool_call_id.clone(),
                        call_type: "function".to_string(),
                        function: FunctionCall {
                            name: t.tool_name.clone(),
                            arguments: args,
                        },
                    });
                }
                SessionEventData::AssistantUsage(u) => {
                    usage.prompt_tokens += u.input_tokens.unwrap_or(0.0) as u32;
                    usage.completion_tokens += u.output_tokens.unwrap_or(0.0) as u32;
                }
                SessionEventData::SessionIdle(_) => break,
                SessionEventData::SessionError(err) => {
                    error!("session error: {}", err.message);
                    return Err(AppError::Upstream(err.message.clone()));
                }
                _ => {}
            },
            Err(e) => {
                error!("event recv error: {e:?}");
                break;
            }
        }
    }

    usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;

    let message = ChatMessage {
        role: "assistant".to_string(),
        content: if content.is_empty() {
            MessageContent::Null
        } else {
            MessageContent::Text(content)
        },
        name: None,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        tool_call_id: None,
    };

    Ok(ChatCompletionResponse {
        id,
        object: "chat.completion",
        created,
        model,
        choices: vec![Choice {
            index: 0,
            message,
            finish_reason,
        }],
        usage: Some(usage),
    })
}
