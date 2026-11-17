use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    pub base_url: String,
    pub models: Vec<String>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

pub fn local_urls() -> Vec<String> {
    vec![
        "http://localhost:11434/v1".into(),
        "http://localhost:1234/v1".into(),
        "http://localhost:8080/v1".into(),
        "http://localhost:8000/v1".into(),
    ]
}

pub async fn discover(urls: &[String]) -> Vec<Server> {
    let Ok(http) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        return Vec::new();
    };
    let mut servers = Vec::new();
    for url in urls {
        let Ok(response) = http
            .get(format!("{}/models", url.trim_end_matches('/')))
            .send()
            .await
        else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(payload) = response.json::<ModelsResponse>().await else {
            continue;
        };
        let models = payload.data.into_iter().map(|entry| entry.id).collect();
        servers.push(Server {
            base_url: url.clone(),
            models,
        });
    }
    servers
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[test]
    fn local_urls_lists_all_default_servers() {
        assert_eq!(
            local_urls(),
            vec![
                "http://localhost:11434/v1".to_string(),
                "http://localhost:1234/v1".to_string(),
                "http://localhost:8080/v1".to_string(),
                "http://localhost:8000/v1".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn discover_finds_models_on_responsive_server() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v1/models");
            then.status(200).json_body(serde_json::json!({
                "data": [{"id": "m1"}, {"id": "m2"}]
            }));
        });
        let urls = vec![format!("{}/v1", server.base_url())];

        let servers = discover(&urls).await;

        mock.assert();
        assert_eq!(
            servers,
            vec![Server {
                base_url: urls[0].clone(),
                models: vec!["m1".to_string(), "m2".to_string()],
            }]
        );
    }

    #[tokio::test]
    async fn discover_skips_unreachable_servers() {
        let urls = vec!["http://127.0.0.1:9/unlikely".to_string()];

        let servers = discover(&urls).await;

        assert!(servers.is_empty());
    }
}
