use std::sync::Arc;

use copilot_sdk::Client;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub client: Arc<Client>,
    pub config: Arc<Config>,
}

impl AppState {
    pub fn new(client: Client, config: Config) -> Self {
        Self {
            client: Arc::new(client),
            config: Arc::new(config),
        }
    }
}
