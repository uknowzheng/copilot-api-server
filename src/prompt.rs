use crate::types::{ChatMessage, ContentPart, MessageContent};

/// 把 OpenAI messages 数组拼接为单个 Copilot prompt。
///
/// 策略：
/// - 历史消息（除最后一条 user 外）用 XML 标签包裹，按角色分组：
///   `<previous_system>...</previous_system>`、`<previous_user>...</previous_user>`、
///   `<previous_assistant>...</previous_assistant>`、`<tool_result tool_call_id="...">...</tool_result>`
/// - 最后一条 user 消息单独作为主 prompt 末尾，不加标签。
/// - 多模态 image_url 部分降级为 `[image: <url>]` 文本提示，避免静默丢弃。
pub fn build_prompt(messages: &[ChatMessage]) -> String {
    if messages.is_empty() {
        return String::new();
    }

    // 找到最后一条 user 消息的索引
    let last_user_idx = messages
        .iter()
        .enumerate()
        .rev()
        .find(|(_, m)| m.role == "user")
        .map(|(i, _)| i);

    let mut buf = String::new();

    for (i, msg) in messages.iter().enumerate() {
        let text = extract_text(&msg.content);

        // 是最后一条 user 则跳过，留到末尾输出
        if Some(i) == last_user_idx {
            continue;
        }

        match msg.role.as_str() {
            "system" => {
                if !text.is_empty() {
                    buf.push_str("<previous_system>\n");
                    buf.push_str(&text);
                    buf.push_str("\n</previous_system>\n\n");
                }
            }
            "user" => {
                if !text.is_empty() {
                    buf.push_str("<previous_user>\n");
                    buf.push_str(&text);
                    buf.push_str("\n</previous_user>\n\n");
                }
            }
            "assistant" => {
                buf.push_str("<previous_assistant>\n");
                if !text.is_empty() {
                    buf.push_str(&text);
                    buf.push('\n');
                }
                if let Some(calls) = &msg.tool_calls {
                    for c in calls {
                        buf.push_str(&format!(
                            "<tool_call id=\"{}\" name=\"{}\">{}</tool_call>\n",
                            c.id, c.function.name, c.function.arguments
                        ));
                    }
                }
                buf.push_str("</previous_assistant>\n\n");
            }
            "tool" => {
                let id = msg.tool_call_id.as_deref().unwrap_or("");
                buf.push_str(&format!("<tool_result tool_call_id=\"{}\">\n", id));
                buf.push_str(&text);
                buf.push_str("\n</tool_result>\n\n");
            }
            other => {
                // 未知角色按 user 处理但保留角色名
                if !text.is_empty() {
                    buf.push_str(&format!("<previous_{other}>\n"));
                    buf.push_str(&text);
                    buf.push_str(&format!("\n</previous_{other}>\n\n"));
                }
            }
        }
    }

    // 末尾追加最后一条 user 消息（无标签，作为主 prompt）
    if let Some(idx) = last_user_idx {
        let text = extract_text(&messages[idx].content);
        if !buf.is_empty() {
            buf.push_str("---\n\n");
        }
        buf.push_str(&text);
    } else if buf.is_empty() {
        // 没有任何 user，把最后一条任意消息当 prompt
        if let Some(last) = messages.last() {
            buf.push_str(&extract_text(&last.content));
        }
    }

    buf
}

fn extract_text(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(s) => s.clone(),
        MessageContent::Null => String::new(),
        MessageContent::Parts(parts) => {
            let mut out = String::new();
            for (i, p) in parts.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                match p {
                    ContentPart::Text { text } => out.push_str(text),
                    ContentPart::ImageUrl { image_url } => {
                        out.push_str(&format!("[image: {}]", image_url.url));
                    }
                    ContentPart::Other => {}
                }
            }
            out
        }
    }
}
