use conjure_core::AssetRequest;

use crate::state::{AppState, Generated, RunState};

// The Start command crosses the channel once per generation, so the large
// request payload costs nothing; the boxed alternative would complicate the
// Wave 3 call sites for no gain.
#[allow(clippy::large_enum_variant)]
pub enum GenCommand {
    Discover,
    Start {
        provider: String,
        model: String,
        base_url: String,
        request: AssetRequest,
    },
    Cancel,
}

pub fn event_line(event: &conjure_agent::GenEvent) -> String {
    match event {
        conjure_agent::GenEvent::ModelCall => "model call".to_string(),
        conjure_agent::GenEvent::SpecParsed => "spec parsed".to_string(),
        conjure_agent::GenEvent::VerifyFailed { errors } => {
            format!("verify failed: {errors} error(s)")
        }
        conjure_agent::GenEvent::RepairTurn { turn } => format!("repair turn {turn}"),
        conjure_agent::GenEvent::FallbackEngaged => "fallback engaged".to_string(),
        conjure_agent::GenEvent::Passed => "passed".to_string(),
    }
}

/// Same rule (and exact texts) as the CLI and MCP tool.
pub fn provider_rule(provider: &str, base_url: &str) -> Result<(), String> {
    match provider {
        "http" if base_url.is_empty() => Err("error: provider http requires base_url".to_string()),
        "opencode" if !base_url.is_empty() => {
            Err("error: provider opencode does not take base_url".to_string())
        }
        "http" | "opencode" => Ok(()),
        other => Err(format!(
            "error: unknown provider {other} (use http or opencode)"
        )),
    }
}

pub fn spawn(handle: rune::AppHandle<AppState>) -> std::sync::mpsc::Sender<GenCommand> {
    let (tx, rx) = std::sync::mpsc::channel();
    let current = std::sync::Arc::new(std::sync::Mutex::new(None::<tokio::task::JoinHandle<()>>));
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        for command in rx {
            run(&rt, &handle, &current, command);
        }
    });
    tx
}

fn run(
    rt: &tokio::runtime::Runtime,
    handle: &rune::AppHandle<AppState>,
    current: &std::sync::Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    command: GenCommand,
) {
    match command {
        GenCommand::Discover => {
            let servers =
                rt.block_on(conjure_inference::discover(&conjure_inference::local_urls()));
            let opencode = rt.block_on(conjure_inference::list_opencode_models(
                &conjure_inference::opencode_bin(),
            ));
            let mut models = Vec::new();
            for server in &servers {
                for model in &server.models {
                    models.push((server.base_url.clone(), model.clone()));
                }
            }
            let opencode_models = opencode.unwrap_or_default();
            let both_empty = models.is_empty() && opencode_models.is_empty();
            let handle = handle.clone();
            handle.update(move |s, _| {
                s.r#gen.models = models;
                s.r#gen.opencode_models = opencode_models;
                if both_empty {
                    s.r#gen.note =
                        Some("no local model servers and no opencode models found".to_string());
                }
            });
        }
        GenCommand::Start {
            provider,
            model,
            base_url,
            request,
        } => {
            if let Err(note) = provider_rule(&provider, &base_url) {
                handle.update(move |s, _| {
                    s.r#gen.note = Some(note);
                    s.r#gen.state = RunState::Failed;
                });
                return;
            }
            let backend = match provider.as_str() {
                "http" => conjure_inference::Backend::Http(conjure_inference::Client::new(
                    base_url.clone(),
                    model.clone(),
                )),
                _ => conjure_inference::Backend::OpenCode(conjure_inference::OpenCodeCli::new(
                    conjure_inference::opencode_bin(),
                    model.clone(),
                )),
            };
            handle.update(|s, _| {
                s.r#gen.state = RunState::Running;
                s.r#gen.events.clear();
                s.r#gen.note = None;
                s.r#gen.used_fallback = false;
                s.r#gen.pending = None;
            });
            // The observer stays cheap: handle.update only queues the closure.
            let observer_handle = handle.clone();
            let observer: conjure_agent::GenObserver =
                std::sync::Arc::new(move |event: &conjure_agent::GenEvent| {
                    let line = event_line(event);
                    let engaged = matches!(event, conjure_agent::GenEvent::FallbackEngaged);
                    observer_handle.update(move |s, _| {
                        s.r#gen.events.push(line);
                        if engaged {
                            s.r#gen.used_fallback = true;
                        }
                    });
                });
            let opts = conjure_agent::GenOpts {
                max_repairs: 3,
                on_event: Some(observer),
            };
            let finish_handle = handle.clone();
            let join = rt.spawn(async move {
                match conjure_agent::generate(&backend, &request, &opts).await {
                    Ok(result) => {
                        finish_handle.update(move |s, _| {
                            s.r#gen.state = RunState::Passed;
                            s.r#gen.pending = Some(Generated {
                                request,
                                assets: result.assets,
                                used_fallback: result.used_fallback,
                            });
                            if result.used_fallback {
                                s.r#gen.note = Some(
                                    "fallback engaged: assets are deterministic placeholders"
                                        .to_string(),
                                );
                            }
                        });
                    }
                    Err(e) => {
                        finish_handle.update(move |s, _| {
                            s.r#gen.state = RunState::Failed;
                            s.r#gen.note = Some(format!("error: {e}"));
                        });
                    }
                }
            });
            *current.lock().unwrap() = Some(join);
        }
        GenCommand::Cancel => {
            if let Some(join) = current.lock().unwrap().take() {
                join.abort();
            }
            handle.update(|s, _| {
                s.r#gen.state = RunState::Idle;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use conjure_agent::GenEvent;

    use super::*;

    #[test]
    fn event_lines_match_the_log_contract() {
        assert_eq!(event_line(&GenEvent::ModelCall), "model call");
        assert_eq!(event_line(&GenEvent::SpecParsed), "spec parsed");
        assert_eq!(
            event_line(&GenEvent::VerifyFailed { errors: 2 }),
            "verify failed: 2 error(s)"
        );
        assert_eq!(
            event_line(&GenEvent::RepairTurn { turn: 1 }),
            "repair turn 1"
        );
        assert_eq!(event_line(&GenEvent::FallbackEngaged), "fallback engaged");
        assert_eq!(event_line(&GenEvent::Passed), "passed");
    }

    #[test]
    fn provider_rules_match_the_tool_error_texts() {
        assert_eq!(
            provider_rule("http", "").unwrap_err(),
            "error: provider http requires base_url"
        );
        assert_eq!(
            provider_rule("opencode", "http://localhost:11434/v1").unwrap_err(),
            "error: provider opencode does not take base_url"
        );
        assert_eq!(
            provider_rule("weird", "").unwrap_err(),
            "error: unknown provider weird (use http or opencode)"
        );
        assert!(provider_rule("http", "http://localhost:11434/v1").is_ok());
        assert!(provider_rule("opencode", "").is_ok());
    }
}
