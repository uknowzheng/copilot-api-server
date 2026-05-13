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
| `--default-model` | `DEFAULT_MODEL` | `auto` | 客户端不指定 model 时的默认值；`auto` 让 Copilot 自动选 |

日志级别通过 `RUST_LOG` 控制（`info` / `debug` / `trace`）。

---

## 接口

### `GET /v1/models`

实时调用 `copilot-sdk` 的 `client.list_models()`，返回当前账号在 GitHub Copilot 下实际可用的模型列表。除标准 OpenAI 字段外，还附带 `display_name`、`capabilities`（vision / reasoning_effort / 上下文窗口）、`billing_multiplier`、`policy_state`、`supported_reasoning_efforts`、`default_reasoning_effort`，方便客户端筛选。

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

- **图片传递为文本提示**：Copilot CLI 不接受真实图片输入，多模态 image_url 仅转为 `[image: <url>]`
- **单 prompt 模型**：底层 SDK 只接收单字符串 prompt，多轮上下文以拼接形式传入；行为接近 ChatGPT 而非 Anthropic Messages API

---

## 开发

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo build --release
```

---

## 构建产物

`Cargo.toml` 的 `[profile.release]` 已开启 `lto = true` / `codegen-units = 1` / `strip = true`，直接 release 构建即可拿到精简后的单文件二进制（无外部动态依赖，仅链接系统 libc）。

### 本机构建

```bash
cargo build --release
# 产物：target/release/copilot-api-server   (~2.9 MB on aarch64-apple-darwin)
```

直接拷走运行即可：

```bash
cp target/release/copilot-api-server ~/bin/copilot-api-server
copilot-api-server --host 127.0.0.1 --port 8080
```

### 查看 / 调整 target

```bash
rustc -vV | grep host          # 当前 host triple
rustup target list --installed
```

需要其它平台先装 target：

```bash
rustup target add x86_64-apple-darwin       # Intel macOS
rustup target add aarch64-apple-darwin      # Apple Silicon macOS
rustup target add x86_64-unknown-linux-gnu  # Linux (需对应 glibc 工具链)
rustup target add x86_64-unknown-linux-musl # Linux 静态 (musl)
```

### 跨平台编译

**macOS x86_64 ↔ arm64**（同机互编，原生 toolchain 即可）：

```bash
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
# 产物：target/<triple>/release/copilot-api-server
```

合成 Universal Binary（Intel + ARM 二合一）：

```bash
lipo -create -output copilot-api-server \
  target/x86_64-apple-darwin/release/copilot-api-server \
  target/aarch64-apple-darwin/release/copilot-api-server
file copilot-api-server   # Mach-O universal binary with 2 architectures
```

**Linux（推荐 musl 静态产物，零运行时依赖）**：

```bash
# 方式 A：cross（推荐，自动处理 C 工具链）
cargo install cross --git https://github.com/cross-rs/cross
cross build --release --target x86_64-unknown-linux-musl
# 产物：target/x86_64-unknown-linux-musl/release/copilot-api-server
```

```bash
# 方式 B：本地装好 musl-cross / lld，再用 cargo
cargo build --release --target x86_64-unknown-linux-musl
```

> ⚠️ 注意：服务运行时需要本机存在 GitHub Copilot CLI（`copilot` 在 PATH 或设置 `COPILOT_CLI_PATH`），跨编出来的二进制部署到目标机器后仍需先 `npm i -g @github/copilot` 并完成登录。

### 打包发布

```bash
TARGET=aarch64-apple-darwin
VERSION=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
tar -czf "copilot-api-server-${VERSION}-${TARGET}.tar.gz" \
  -C target/${TARGET}/release copilot-api-server
shasum -a 256 "copilot-api-server-${VERSION}-${TARGET}.tar.gz"
```

### 体积/启动加速（可选）

`[profile.release]` 已是体积优先配置。若想更小可加 `opt-level = "z"`（牺牲少量性能换体积）；若想更快启动可去掉 `lto = true`（产物变大）。

---

## 自动发布（GitHub Release）

仓库内置 `.github/workflows/release.yml`，覆盖 4 个平台：

| Target | 适用 |
|---|---|
| `aarch64-apple-darwin` | Apple Silicon Mac (M1/M2/M3/M4) |
| `x86_64-apple-darwin` | Intel Mac |
| `x86_64-unknown-linux-musl` | x86_64 Linux 服务器（静态，无 glibc 依赖） |
| `aarch64-unknown-linux-musl` | ARM64 Linux 服务器 / 树莓派 4+ |

每个 target 产物：`copilot-api-server-vX.Y.Z-<target>.tar.gz` + `.sha256`。

### 触发方式 A：推 tag（推荐）

```bash
git tag v0.1.0
git push origin v0.1.0
```

CI 自动跑：解析版本 → 多平台交叉编译 → 上传产物 → 发布 Release。

### 触发方式 B：GitHub UI 手动触发

GitHub 仓库 → **Actions** → **release** → **Run workflow**，输入版本号（如 `v0.1.0`）。tag 不存在会自动创建并推送。

### 版本号规则

- 必须形如 `vMAJOR.MINOR.PATCH`（例如 `v0.1.0`、`v1.2.3`），不符合会直接 fail。
- `Cargo.toml` 内的 `version` 字段 **不自动更新**，需要发布前手动 bump 一致（或保持解耦——版本号仅来自 tag）。

### 注意事项

- `copilot-sdk` 是 git `main` 依赖，CI 每次会重新拉取，上游若挂构建会一起挂。
- `Cargo.lock` 被 gitignore，CI 不带 `--locked`，依赖版本由 Cargo 解析当时决定。
- Release 产物里**不含** Copilot CLI，目标机器仍需自行 `npm i -g @github/copilot` 并登录。

---

## 许可证

MIT
