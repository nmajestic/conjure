use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self::for_role("system", content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::for_role("user", content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::for_role("assistant", content)
    }

    fn for_role(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    pub base_url: String,
    pub model: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: String,
}

impl Client {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
        }
    }

    pub async fn chat(&self, messages: &[Message]) -> Result<String, String> {
        let endpoint = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|err| format!("cannot build http client: {err}"))?;
        let response = http
            .post(&endpoint)
            .json(&serde_json::json!({
                "model": self.model,
                "messages": messages,
                "stream": false,
            }))
            .send()
            .await
            .map_err(|err| format!("cannot reach model server at {}: {err}", self.base_url))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "server at {endpoint} returned {status}: {}",
                snippet(&body)
            ));
        }

        let payload: ChatResponse = response
            .json()
            .await
            .map_err(|err| format!("server at {endpoint} returned a malformed response: {err}"))?;
        let Some(choice) = payload.choices.into_iter().next() else {
            return Err(format!("server at {endpoint} returned no choices"));
        };
        Ok(choice.message.content)
    }
}

fn snippet(body: &str) -> String {
    let mut text: String = body.chars().take(200).collect();
    if body.chars().count() > 200 {
        text.push_str("...");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[tokio::test]
    async fn chat_returns_assistant_content() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).json_body(serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": "hello"}}]
            }));
        });
        let client = Client::new(server.base_url(), "test-model");

        let reply = client.chat(&[Message::user("hi")]).await;

        mock.assert();
        assert_eq!(reply.unwrap(), "hello");
    }

    #[tokio::test]
    async fn chat_sends_no_authorization_header() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .header_missing("authorization");
            then.status(200).json_body(serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": "hello"}}]
            }));
        });
        let client = Client::new(server.base_url(), "test-model");

        let reply = client.chat(&[Message::user("hi")]).await;

        mock.assert();
        assert_eq!(reply.unwrap(), "hello");
    }

    #[tokio::test]
    async fn chat_reports_non_2xx_status() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(500)
                .json_body(serde_json::json!({"error": "boom"}));
        });
        let client = Client::new(server.base_url(), "test-model");

        let reply = client.chat(&[Message::user("hi")]).await;

        mock.assert();
        let err = reply.unwrap_err();
        assert!(err.contains("500"), "unexpected error: {err}");
    }
}
