use conjure_core::{Asset, AssetRequest, CompactSpec};
use conjure_inference::Message;

use crate::prompt::{system_prompt, user_prompt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenEvent {
    ModelCall,
    SpecParsed,
    VerifyFailed { errors: usize },
    RepairTurn { turn: u32 },
    FallbackEngaged,
    Passed,
}

pub type GenObserver = std::sync::Arc<dyn Fn(&GenEvent) + Send + Sync>;

pub struct GenOpts {
    pub max_repairs: u32,
    pub on_event: Option<GenObserver>,
}

impl Default for GenOpts {
    fn default() -> Self {
        Self {
            max_repairs: 3,
            on_event: None,
        }
    }
}

fn emit(opts: &GenOpts, event: GenEvent) {
    if let Some(observer) = &opts.on_event {
        observer(&event);
    }
}

pub struct GenResult {
    pub assets: Vec<Asset>,
    pub repairs_used: u32,
    pub used_fallback: bool,
}

pub async fn generate(
    backend: &conjure_inference::Backend,
    request: &AssetRequest,
    opts: &GenOpts,
) -> Result<GenResult, String> {
    request.style_card.validate()?;
    let mut messages = vec![
        Message::system(system_prompt(request)),
        Message::user(user_prompt(request)),
    ];
    let mut last_assets: Option<Vec<Asset>> = None;

    for attempt in 0..=opts.max_repairs {
        if attempt > 0 {
            emit(opts, GenEvent::RepairTurn { turn: attempt });
        }
        emit(opts, GenEvent::ModelCall);
        let reply = backend.chat(&messages).await?;

        let spec = match parse_spec(&reply) {
            Ok(spec) => spec,
            Err(err) => {
                messages.push(Message::assistant(reply));
                messages.push(Message::user(format!(
                    "that reply was not valid JSON for the spec schema: {err}. Respond again with ONLY the JSON spec."
                )));
                continue;
            }
        };

        emit(opts, GenEvent::SpecParsed);

        let assets = match conjure_core::expand(request, &spec) {
            Ok(assets) => assets,
            Err(err) => {
                messages.push(Message::assistant(reply));
                messages.push(Message::user(format!(
                    "spec rejected: {err}. Respond again with ONLY a corrected JSON spec."
                )));
                continue;
            }
        };

        let report = conjure_core::verify_request(request, &assets);
        if report.passed {
            emit(opts, GenEvent::Passed);
            return Ok(GenResult {
                assets,
                repairs_used: attempt,
                used_fallback: false,
            });
        }
        emit(
            opts,
            GenEvent::VerifyFailed {
                errors: report.errors.len(),
            },
        );
        last_assets = Some(assets);
        messages.push(Message::assistant(reply));
        let mut problems = String::from("verification failed with these problems:\n");
        for diagnostic in &report.errors {
            problems.push_str(&format!("- {}\n", diagnostic.message));
        }
        problems.push_str("Respond again with ONLY a corrected JSON spec.");
        messages.push(Message::user(problems));
    }

    emit(opts, GenEvent::FallbackEngaged);
    let assets = match last_assets {
        Some(assets) => conjure_core::repair(request, &assets),
        None => conjure_core::generate(request, 0),
    };
    Ok(GenResult {
        assets,
        repairs_used: opts.max_repairs,
        used_fallback: true,
    })
}

fn parse_spec(reply: &str) -> Result<CompactSpec, String> {
    let text = strip_code_fence(reply);
    let json =
        balanced_json(text).ok_or_else(|| "no JSON object found in the reply".to_string())?;
    serde_json::from_str(json).map_err(|err| err.to_string())
}

fn strip_code_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    inner.strip_suffix("```").map(str::trim).unwrap_or(inner)
}

fn balanced_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, c) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '{' => depth += 1,
            '}' => match depth.checked_sub(1) {
                Some(0) => return Some(&text[start..start + idx + 1]),
                Some(remaining) => depth = remaining,
                None => return None,
            },
            '"' => in_string = true,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use conjure_core::style::{Dimensions, Grid, Outline, PaletteColor, Shading, Style, StyleCard};
    use conjure_core::{AssetKind, verify_request};
    use httpmock::{Method, MockServer};

    const SPRITE_SPEC: &str = r#"{"type":"sprite","rows":["0000","0110","0121","0000"]}"#;
    const SPRITE_SPEC_BAD_WIDTH: &str = r#"{"type":"sprite","rows":["000","0110","0121","0000"]}"#;
    const SPRITE_SPEC_BAD_ROW_COUNT: &str = r#"{"type":"sprite","rows":["0000","0000"]}"#;
    const TILESET_SPEC: &str = r#"{"type":"tileset","tiles":[["0000","0110","0121","0000"],["0000","0330","0130","0000"]]}"#;
    const GARBAGE_REPLY: &str = "sorry, i cannot draw that.";

    fn style_card() -> StyleCard {
        let palette = (0..4)
            .map(|i| PaletteColor {
                name: format!("c{i}"),
                hex: format!("#{:06x}", i * 0x101010),
            })
            .collect();
        StyleCard {
            id: "card".to_string(),
            name: "card".to_string(),
            palette,
            grid: Grid {
                tile_size: Dimensions { w: 16, h: 16 },
                unit_scale: 1,
            },
            style: Style {
                outline: Outline::None,
                outline_color: "#000000".to_string(),
                shading: Shading::Flat,
                proportions: "1:1".to_string(),
                era: "modern".to_string(),
                theme: "pixel".to_string(),
            },
        }
    }

    fn sprite_request() -> AssetRequest {
        AssetRequest {
            name: "hero".to_string(),
            kind: AssetKind::Sprite {
                size: Dimensions { w: 4, h: 4 },
            },
            style_card: style_card(),
        }
    }

    fn tileset_request() -> AssetRequest {
        AssetRequest {
            name: "floor".to_string(),
            kind: AssetKind::Tileset {
                tile_size: Dimensions { w: 4, h: 4 },
                count: 2,
            },
            style_card: style_card(),
        }
    }

    fn test_client(server: &MockServer) -> conjure_inference::Backend {
        conjure_inference::Backend::Http(conjure_inference::Client {
            base_url: server.base_url(),
            model: "mock-model".to_string(),
        })
    }

    fn completion(reply: &str) -> serde_json::Value {
        serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": reply } }]
        })
    }

    #[test]
    fn parse_spec_extracts_json_leniently() {
        assert_eq!(
            parse_spec(SPRITE_SPEC).unwrap(),
            CompactSpec::Sprite {
                rows: vec![
                    "0000".to_string(),
                    "0110".to_string(),
                    "0121".to_string(),
                    "0000".to_string()
                ]
            }
        );
        let fenced = format!("sure, here it is:\n```json\n{SPRITE_SPEC}\n```\nhope this helps");
        assert!(parse_spec(&fenced).is_ok());
        let with_trailing_prose = format!("{SPRITE_SPEC} hope this helps");
        assert!(parse_spec(&with_trailing_prose).is_ok());
        assert!(parse_spec(GARBAGE_REPLY).is_err());
    }

    fn mock_chat<'a>(server: &'a httpmock::MockServer, reply: &str) -> httpmock::Mock<'a> {
        server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(200).json_body(completion(reply));
        })
    }

    #[test]
    fn system_prompt_lists_palette_schema_and_constraints() {
        let prompt = system_prompt(&sprite_request());
        let role = prompt.find("pixel artist").expect("role statement");
        let palette = prompt
            .find("'0' -> index 0: c0 (#000000)")
            .expect("palette chart");
        let transparent = prompt
            .find("char '0' is the transparent background")
            .unwrap();
        let schema = prompt
            .find(r#""type":"sprite","rows":["4 chars per row, 4 rows"]"#)
            .expect("schema");
        let allowed = prompt
            .find("allowed row chars: '0'-'9' and 'a'-'v'")
            .unwrap();
        let style = prompt.find("style tokens: outline: none").unwrap();
        let hard = prompt
            .find("hard requirement: every row is EXACTLY the right char count (4 chars)")
            .unwrap();
        assert!(role < palette && palette < transparent && transparent < schema);
        assert!(schema < allowed && allowed < style && style < hard);
        let tileset_prompt = system_prompt(&tileset_request());
        assert!(tileset_prompt.contains(r#""type":"tileset","tiles":[["...row..."#));
        assert!(tileset_prompt.contains("with exactly 2 tiles, each 4 rows of 4 chars"));
    }

    #[tokio::test]
    async fn happy_path_sprite_passes_on_first_reply() {
        let server = MockServer::start();
        let mock = mock_chat(&server, SPRITE_SPEC);
        let backend = test_client(&server);
        let request = sprite_request();

        let result = generate(&backend, &request, &GenOpts::default())
            .await
            .unwrap();

        assert_eq!(result.repairs_used, 0);
        assert!(!result.used_fallback);
        assert_eq!(result.assets.len(), 1);
        assert!(verify_request(&request, &result.assets).passed);
        mock.assert_calls(1);
    }

    #[tokio::test]
    async fn repair_turn_recovers_from_bad_spec() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(Method::POST)
                .path("/chat/completions")
                .body_excludes("spec rejected");
            then.status(200)
                .json_body(completion(SPRITE_SPEC_BAD_WIDTH));
        });
        let repaired = server.mock(|when, then| {
            when.method(Method::POST)
                .path("/chat/completions")
                .body_includes("spec rejected");
            then.status(200).json_body(completion(SPRITE_SPEC));
        });
        let backend = test_client(&server);
        let request = sprite_request();

        let result = generate(&backend, &request, &GenOpts::default())
            .await
            .unwrap();

        assert_eq!(result.repairs_used, 1);
        assert!(!result.used_fallback);
        assert!(verify_request(&request, &result.assets).passed);
        repaired.assert_calls(1);
    }

    #[tokio::test]
    async fn invalid_json_every_time_falls_back_to_deterministic_assets() {
        let server = MockServer::start();
        let mock = mock_chat(&server, GARBAGE_REPLY);
        let backend = test_client(&server);
        let request = sprite_request();

        let result = generate(&backend, &request, &GenOpts::default())
            .await
            .unwrap();

        assert!(result.used_fallback);
        assert_eq!(result.repairs_used, 3);
        assert!(verify_request(&request, &result.assets).passed);
        mock.assert_calls(4);
    }

    #[tokio::test]
    async fn wrong_row_count_every_time_falls_back() {
        let server = MockServer::start();
        let mock = mock_chat(&server, SPRITE_SPEC_BAD_ROW_COUNT);
        let backend = test_client(&server);
        let request = sprite_request();

        let result = generate(&backend, &request, &GenOpts::default())
            .await
            .unwrap();

        assert!(result.used_fallback);
        assert_eq!(result.repairs_used, 3);
        assert!(verify_request(&request, &result.assets).passed);
        mock.assert_calls(4);
    }

    #[tokio::test]
    async fn chat_error_propagates() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(Method::POST).path("/chat/completions");
            then.status(500);
        });
        let backend = test_client(&server);
        let request = sprite_request();

        let result = generate(&backend, &request, &GenOpts::default()).await;

        assert!(result.is_err());
    }

    #[test]
    fn gen_opts_default_keeps_prior_behavior() {
        let default = GenOpts::default();
        assert_eq!(default.max_repairs, 3);
        assert!(default.on_event.is_none());
    }

    #[tokio::test]
    async fn observer_records_full_event_sequence_for_one_repair() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(Method::POST)
                .path("/chat/completions")
                .body_excludes("not valid JSON");
            then.status(200).json_body(completion(GARBAGE_REPLY));
        });
        let repaired = server.mock(|when, then| {
            when.method(Method::POST)
                .path("/chat/completions")
                .body_includes("not valid JSON");
            then.status(200).json_body(completion(SPRITE_SPEC));
        });
        let backend = test_client(&server);
        let request = sprite_request();

        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let opts = GenOpts {
            on_event: Some(std::sync::Arc::new(move |event: &GenEvent| {
                sink.lock().unwrap().push(event.clone());
            })),
            ..GenOpts::default()
        };

        let result = generate(&backend, &request, &opts).await.unwrap();

        assert_eq!(result.repairs_used, 1);
        assert!(!result.used_fallback);
        assert!(verify_request(&request, &result.assets).passed);
        let observed = events.lock().unwrap().clone();
        assert_eq!(
            observed,
            vec![
                GenEvent::ModelCall,
                GenEvent::RepairTurn { turn: 1 },
                GenEvent::ModelCall,
                GenEvent::SpecParsed,
                GenEvent::Passed,
            ]
        );
        repaired.assert_calls(1);
    }

    #[tokio::test]
    async fn happy_path_tileset_passes_with_count_tiles() {
        let server = MockServer::start();
        let mock = mock_chat(&server, TILESET_SPEC);
        let backend = test_client(&server);
        let request = tileset_request();

        let result = generate(&backend, &request, &GenOpts::default())
            .await
            .unwrap();

        assert_eq!(result.assets.len(), 2);
        assert_eq!(result.repairs_used, 0);
        assert!(!result.used_fallback);
        assert!(verify_request(&request, &result.assets).passed);
        mock.assert_calls(1);
    }
}
