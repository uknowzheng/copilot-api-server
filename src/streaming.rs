use std::time::{SystemTime, UNIX_EPOCH};

use async_stream::stream;
use axum::response::sse::Event;
use copilot_sdk::{SessionConfig, SessionEventData};
use futures::Stream;
use serde_json::json;
use tracing::{debug, error, warn};

use crate::state::AppState;
use crate::types::{
    ChatCompletionChunk, ChatCompletionRequest, ChunkChoice, Delta, FunctionCallDelta,
    ToolCallDelta, Usage,
};

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn make_id() -> String {
    format!("chatcmpl-{}", uuid::Uuid::new_v4())
}

/// 构造 SDK SessionConfig：禁用文件/git 等内置工具，只保留客户端声明的工具（即使为空）。
fn build_session_config(req: &ChatCompletionRequest) -> SessionConfig {
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

    SessionConfig {
        tools,
        model: Some(req.model.clone()),
        streaming: true,
        ..Default::default()
    }
}

/// 流式：返回 SSE Event stream。捕获客户端断连：当 axum 关闭 body 时 stream 自动终止。
pub fn stream_chat(
    state: AppState,
    req: ChatCompletionRequest,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let id = make_id();
    let model = req.model.clone();
    let created = now_ts();
    let include_usage = req
        .stream_options
        .as_ref()
        .map(|o| o.include_usage)
        .unwrap_or(false);
    let prompt = crate::prompt::build_prompt(&req.messages);
    let cfg = build_session_config(&req);
    let client = state.client.clone();

    stream! {
        // 先发 role chunk（OpenAI 约定）
        let role_chunk = ChatCompletionChunk {
            id: id.clone(),
            object: "chat.completion.chunk",
            created,
            model: model.clone(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta { role: Some("assistant".to_string()), ..Default::default() },
                finish_reason: None,
            }],
            usage: None,
        };
        yield Ok(sse_data(&role_chunk));

        // 创建 session
        let session = match client.create_session(cfg).await {
            Ok(s) => s,
            Err(e) => {
                error!("create_session failed: {e}");
                yield Ok(sse_error(&id, &model, created, &format!("create_session: {e}")));
                yield Ok(Event::default().data("[DONE]"));
                return;
            }
        };

        let mut events = session.subscribe();

        // send 非阻塞
        if let Err(e) = session.send(prompt.as_str()).await {
            error!("session.send failed: {e}");
            yield Ok(sse_error(&id, &model, created, &format!("send: {e}")));
            yield Ok(Event::default().data("[DONE]"));
            return;
        }

        let mut finish_reason = "stop".to_string();
        let mut usage = Usage::default();
        let mut got_usage = false;
        let mut tool_index: u32 = 0;

        loop {
            match events.recv().await {
                Ok(event) => match &event.data {
                    SessionEventData::AssistantMessageDelta(d) => {
                        let chunk = ChatCompletionChunk {
                            id: id.clone(),
                            object: "chat.completion.chunk",
                            created,
                            model: model.clone(),
                            choices: vec![ChunkChoice {
                                index: 0,
                                delta: Delta {
                                    content: Some(d.delta_content.clone()),
                                    ..Default::default()
                                },
                                finish_reason: None,
                            }],
                            usage: None,
                        };
                        yield Ok(sse_data(&chunk));
                    }
                    SessionEventData::ToolExecutionStart(t) => {
                        finish_reason = "tool_calls".to_string();
                        let args_str = serde_json::to_string(&t.arguments)
                            .unwrap_or_else(|_| "{}".to_string());
                        let chunk = ChatCompletionChunk {
                            id: id.clone(),
                            object: "chat.completion.chunk",
                            created,
                            model: model.clone(),
                            choices: vec![ChunkChoice {
                                index: 0,
                                delta: Delta {
                                    tool_calls: Some(vec![ToolCallDelta {
                                        index: tool_index,
                                        id: Some(t.tool_call_id.clone()),
                                        call_type: Some("function".to_string()),
                                        function: Some(FunctionCallDelta {
                                            name: Some(t.tool_name.clone()),
                                            arguments: Some(args_str),
                                        }),
                                    }]),
                                    ..Default::default()
                                },
                                finish_reason: None,
                            }],
                            usage: None,
                        };
                        yield Ok(sse_data(&chunk));
                        tool_index += 1;
                    }
                    SessionEventData::AssistantUsage(u) => {
                        let p = u.input_tokens.unwrap_or(0.0) as u32;
                        let c = u.output_tokens.unwrap_or(0.0) as u32;
                        usage.prompt_tokens += p;
                        usage.completion_tokens += c;
                        usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;
                        got_usage = true;
                    }
                    SessionEventData::SessionIdle(_) => {
                        debug!("session idle");
                        break;
                    }
                    SessionEventData::SessionError(err) => {
                        error!("session error: {} ({})", err.message, err.error_type);
                        yield Ok(sse_error(&id, &model, created, &err.message));
                        yield Ok(Event::default().data("[DONE]"));
                        return;
                    }
                    _ => {}
                },
                Err(e) => {
                    warn!("event recv error: {e:?}");
                    break;
                }
            }
        }

        // finish chunk
        let finish_chunk = ChatCompletionChunk {
            id: id.clone(),
            object: "chat.completion.chunk",
            created,
            model: model.clone(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta::default(),
                finish_reason: Some(finish_reason),
            }],
            usage: None,
        };
        yield Ok(sse_data(&finish_chunk));

        // usage chunk（可选）
        if include_usage && got_usage {
            let usage_chunk = ChatCompletionChunk {
                id: id.clone(),
                object: "chat.completion.chunk",
                created,
                model: model.clone(),
                choices: vec![],
                usage: Some(usage),
            };
            yield Ok(sse_data(&usage_chunk));
        }

        yield Ok(Event::default().data("[DONE]"));
    }
}

fn sse_data<T: serde::Serialize>(payload: &T) -> Event {
    match serde_json::to_string(payload) {
        Ok(s) => Event::default().data(s),
        Err(e) => {
            error!("sse serialize error: {e}");
            Event::default().data("{}")
        }
    }
}

fn sse_error(id: &str, model: &str, created: i64, msg: &str) -> Event {
    let body = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "error": { "message": msg, "type": "upstream_error" }
    });
    Event::default().data(body.to_string())
}
