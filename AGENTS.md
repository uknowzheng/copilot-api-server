# AGENTS.md

Repo-specific notes for OpenCode sessions. See `README.md` for user-facing docs.

## What this is

Single-binary Rust HTTP server (axum + tokio) that exposes an OpenAI-compatible
`/v1/chat/completions` + `/v1/models` API and proxies to the local GitHub
Copilot CLI via the `copilot-sdk` crate. There is no database, no client SDK,
no workspace — one binary crate.

## Toolchain & dependency gotchas

- `copilot-sdk` is a **git dependency on `main`** (`Cargo.toml:15`). First
  build needs network; upstream breakage will break this repo. If the build
  suddenly fails on SDK types, check the SDK repo before assuming local bug.
- `Cargo.toml` declares `edition = "2021"` but the SDK requires **Rust 1.85+**
  (Edition 2024 transitive). If `cargo build` complains about edition, bump
  the toolchain, not the manifest.
- `Cargo.lock` is **gitignored** (`.gitignore:3`). Reproducible builds rely on
  the SDK git ref. Do not commit the lockfile unless intentionally pinning.
- No `rust-toolchain.toml`; assume the user's stable.

## Commands

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings   # CI-style; clippy must be clean
cargo build --release
RUST_LOG=debug cargo run -- --host 127.0.0.1 --port 8080
```

- **There are no tests.** Do not run `cargo test` expecting coverage; if you
  add tests, put unit tests inline (`#[cfg(test)] mod tests`) — there is no
  `tests/` directory yet.
- Running the binary end-to-end requires the **GitHub Copilot CLI installed
  and logged in** (`copilot` on `PATH`, or `COPILOT_CLI_PATH` env read by the
  SDK). `cargo build` does not need it; `cargo run` will fail at
  `client.start()` without it.

## Architecture facts agents trip on

- **The SDK only accepts a single prompt string per send.** Multi-turn
  history is flattened in `src/prompt.rs` into XML-tagged sections
  (`<previous_user>`, `<previous_assistant>`, `<tool_result …>`), with the
  last `user` message appended raw as the actual prompt. Do not look for a
  `messages`-style API on `copilot_sdk::Session` — it doesn't exist.
- **Session config is built in two places** that must stay in sync:
  `src/handlers.rs` (`chat_blocking`, ~line 79) and
  `src/streaming.rs::build_session_config`. Tool conversion logic is
  duplicated; if you change one, change the other.
- **Image inputs are intentionally downgraded** to `[image: <url>]` text in
  `src/prompt.rs::extract_text`. The SDK doesn't accept binary images. Don't
  "fix" this without understanding the limitation.
- **`/v1/models` queries the SDK live** via `client.list_models()` in
  `src/handlers.rs::list_models` and surfaces extra fields beyond the OpenAI
  spec (`display_name`, `capabilities`, `billing_multiplier`, `policy_state`,
  `supported_reasoning_efforts`, `default_reasoning_effort`). Keep these
  optional-and-skipped-if-none so strict OpenAI clients don't break.
- **Auth is opt-in:** middleware (`src/middleware.rs`) skips entirely when
  `COPILOT_API_KEY` / `--api-key` is empty. Default deployment is unauth on
  `127.0.0.1`. Don't assume requests are authenticated.
- **Shutdown ordering matters:** `src/main.rs` stops HTTP first, then calls
  `client.stop()` with a 30s timeout. If you add background tasks, hook into
  this sequence, not a separate signal handler.
- `RUST_LOG` controls log level via `tracing-subscriber` `EnvFilter`
  (default `info`). CORS is wide open (`Any/Any/Any`) by design.

## File map (only what matters)

- `src/main.rs` — wiring, shutdown, middleware order (auth → cors → trace).
- `src/prompt.rs` — the XML-history-flattening trick. Read before touching
  multi-turn behavior.
- `src/streaming.rs` — SSE chunk shapes, `[DONE]` terminator, optional
  `usage` chunk gated on `stream_options.include_usage`.
- `src/handlers.rs` — non-streaming path; mirrors streaming logic, see sync
  warning above.
- `src/types.rs` — OpenAI wire types (`MessageContent` is an untagged enum
  handling string | parts | null).
- `src/error.rs`, `src/middleware.rs`, `src/config.rs`, `src/state.rs` — small
  and self-explanatory.

## When adding features

- Keep the OpenAI wire format strict: clients (Continue, Cline, LobeChat,
  generic SDKs) rely on field names and SSE framing exactly matching OpenAI's
  spec, including the trailing `data: [DONE]\n\n`.
- New SDK event variants land in `match &event.data` blocks in both
  `handlers.rs` and `streaming.rs` — handle in both or fall through `_ => {}`
  consistently.
