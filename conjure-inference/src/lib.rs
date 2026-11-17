pub mod client;
pub mod discover;
pub mod opencode;

pub use client::{Client, Message};
pub use discover::{Server, discover, local_urls};
pub use opencode::{OpenCodeCli, list_opencode_models, opencode_bin};

pub enum Backend {
    Http(Client),
    OpenCode(OpenCodeCli),
}

impl Backend {
    pub async fn chat(&self, messages: &[Message]) -> Result<String, String> {
        match self {
            Backend::Http(client) => client.chat(messages).await,
            Backend::OpenCode(cli) => cli.chat(messages).await,
        }
    }
}
