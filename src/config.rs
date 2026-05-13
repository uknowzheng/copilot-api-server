use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "copilot-openai-server", about = "OpenAI-compatible server backed by GitHub Copilot")]
pub struct Config {
    /// 监听地址
    #[arg(long, env = "HOST", default_value = "127.0.0.1")]
    pub host: String,

    /// 监听端口
    #[arg(long, env = "PORT", default_value_t = 8080)]
    pub port: u16,

    /// 鉴权 token，留空则不启用鉴权
    #[arg(long, env = "COPILOT_API_KEY", default_value = "")]
    pub api_key: String,

    /// 默认模型 ID（当客户端请求不指定时使用）。`auto` 让 Copilot 自动选模型。
    #[arg(long, env = "DEFAULT_MODEL", default_value = "auto")]
    pub default_model: String,
}

impl Config {
    pub fn auth_token(&self) -> Option<String> {
        if self.api_key.trim().is_empty() {
            None
        } else {
            Some(self.api_key.clone())
        }
    }
}
