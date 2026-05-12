# copilot-api-server

OpenAI 兼容的 HTTP 服务，后端通过 [copilot-sdk-rust](https://github.com/copilot-community-sdk/copilot-sdk-rust) 调用本地 GitHub Copilot CLI。基于 **axum + tokio** 构建，一个二进制即可对外提供 `/v1/chat/completions` 与 `/v1/models`。

> 适合在本机或内网把 GitHub Copilot 桥接给任何兼容 OpenAI 协议的客户端（Continue、Cline、LobeChat、各类 SDK 等）。

---

## 特性

- **OpenAI 协议兼容**：`/v1/models`、`/v1/chat/completions`（流式 SSE & 非流式）
- **多模态消息**：支持 `content` 为字符串或 `[{type:"text"|"image_url"}, ...]`，图片以 `[image: <url>]` 文本形式传给模型
- **多轮上下文**：历史 messages 用 XML 标签拼接（`<previous_user>`、`<previous_assistant>`、`<tool_result>` 等），最后一条 user 作为主 prompt
- **工具调用透传**：客户端声明的 tools 转换为 Copilot SDK 的自定义工具；流式输出遵循 OpenAI tool_calls delta 协议
- **Bearer 鉴权**（可选）：设置 `COPILOT_API_KEY` 即启用
- **流式 usage**：客户端 `stream_options.include_usage=true` 时按 OpenAI 规范追加 usage chunk
- **优雅关闭**：先停 HTTP 再关闭 Copilot 会话，超时 30 秒
- **CORS 全开放**：方便浏览器端直接调用

---

## 环境要求

- Rust **1.85+**（Edition 2024，`copilot-sdk` 强制）
- 已安装并登录的 [GitHub Copilot CLI](https://docs.github.com/copilot/github-copilot-in-the-cli)
- `copilot` 可执行文件在 `PATH`，或设置 `COPILOT_CLI_PATH`

> Windows 上若用默认 MSVC 工具链，请先安装 Visual Studio Build Tools 的 C++ workload；用 GNU 工具链请确保用户名 / 项目路径不含空格（mingw `dlltool` 限制）。

---

## 构建与运行

```bash
git clone https://github.com/uknowzheng/copilot-api-server.git
cd copilot-api-server
cargo build --release
./target/release/copilot-api-server --host 127.0.0.1 --port 8080
```

PowerShell：

```powershell
$env:RUST_LOG = "info"
.\target\release\copilot-api-server.exe --host 127.0.0.1 --port 8080
```

### 启用鉴权

```powershell
$env:COPILOT_API_KEY = "your-secret"
.\target\release\copilot-api-server.exe
```

启用后所有请求需带 `Authorization: Bearer your-secret`。未设置时跳过鉴权（默认仅监听 `127.0.0.1`）。

---

## 命令行参数

| 参数 | 环境变量 | 默认 | 说明 |
|---|---|---|---|
| `--host` | `HOST` | `127.0.0.1` | 监听地址 |
| `--port` | `PORT` | `8080` | 监听端口 |
| `--api-key` | `COPILOT_API_KEY` | *(空)* | Bearer token；空则不鉴权 |
| `--default-model` | `DEFAULT_MODEL` | `gpt-4o` | 客户端不指定 model 时的默认值 |

日志级别通过 `RUST_LOG` 控制（`info` / `debug` / `trace`）。

---

## 接口

### `GET /v1/models`

返回常见模型 ID 列表（占位）。SDK 暂未提供真实模型查询时使用。

### `POST /v1/chat/completions`

请求体兼容 OpenAI Chat Completions 协议关键字段：`model` / `messages` / `stream` / `stream_options` / `tools` / `tool_choice` / `temperature` / `top_p` / `max_tokens` / `n` / `user`。

```bash
curl http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "stream": true,
    "messages": [
      {"role":"system","content":"你是一个简洁的助手。"},
      {"role":"user","content":"用一句话介绍 Rust。"}
    ]
  }'
```

---

## 架构

```
HTTP (axum)
  └─ /v1/chat/completions
       ├─ build_prompt(messages)        # XML 标签拼接历史
       ├─ Client::create_session(cfg)   # copilot-sdk
       ├─ session.send(prompt)
       └─ session.subscribe().recv()    # 事件 → OpenAI SSE chunk
```

源码模块：

| 文件 | 职责 |
|---|---|
| `src/main.rs` | 启动 axum、装配中间件、优雅关闭 |
| `src/config.rs` | clap CLI / 环境变量 |
| `src/state.rs` | `AppState { client, config }` |
| `src/types.rs` | OpenAI 协议类型（多模态、工具、流式 chunk） |
| `src/error.rs` | 统一 `AppError` + `IntoResponse` |
| `src/middleware.rs` | Bearer token 鉴权 |
| `src/prompt.rs` | messages → 单 prompt（XML 标签 + 多模态降级） |
| `src/streaming.rs` | SSE 流式响应（事件 → chunks → `[DONE]`） |
| `src/handlers.rs` | `/v1/models`、`/v1/chat/completions`（流 & 非流） |

---

## 已知限制

- **模型列表是占位的**：`copilot-sdk` 暴露了 `client.list_models()`，后续可改为实时查询
- **图片传递为文本提示**：Copilot CLI 不接受真实图片输入，多模态 image_url 仅转为 `[image: <url>]`
- **单 prompt 模型**：底层 SDK 只接收单字符串 prompt，多轮上下文以拼接形式传入；行为接近 ChatGPT 而非 Anthropic Messages API

---

## 开发

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## 许可证

MIT
