use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use crate::Message;

pub struct OpenCodeCli {
    pub bin: String,
    pub model: String,
    pub timeout_secs: u64,
}

impl OpenCodeCli {
    pub fn new(bin: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            bin: bin.into(),
            model: model.into(),
            timeout_secs: 180,
        }
    }

    pub async fn chat(&self, messages: &[Message]) -> Result<String, String> {
        let prompt = messages
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        let child = spawn_opencode(&self.bin, |cmd| {
            cmd.arg("run")
                .arg("--format")
                .arg("json")
                .arg("--archived")
                .arg("--model")
                .arg(&self.model)
                .arg("--agent")
                .arg("conjure")
                .arg(&prompt);
        })
        .await?;

        let output = timeout(
            Duration::from_secs(self.timeout_secs),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| format!("opencode run timed out after {}s", self.timeout_secs))?
        .map_err(|err| format!("opencode run failed to complete: {err}"))?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() {
            return Err(format!(
                "opencode run exited with {}: {}",
                exit_label(&output.status),
                failure_detail(&stderr, &stdout)
            ));
        }

        let mut text = String::new();
        for line in stdout.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let fragment = event
                .pointer("/part/text")
                .and_then(|part| part.as_str())
                .filter(|_| event.get("type").and_then(|kind| kind.as_str()) == Some("text"));
            if let Some(fragment) = fragment {
                text.push_str(fragment);
            }
        }
        if text.is_empty() {
            let detail = failure_detail("", &stdout);
            if detail.is_empty() {
                return Err("opencode run produced no text output".to_string());
            }
            return Err(format!("opencode run produced no text output: {detail}"));
        }
        Ok(text)
    }
}

pub fn opencode_bin() -> String {
    std::env::var("CONJURE_OPENCODE_BIN").unwrap_or_else(|_| "opencode".to_string())
}

pub async fn list_opencode_models(bin: &str) -> Result<Vec<String>, String> {
    list_opencode_models_with_timeout(bin, 30).await
}

pub async fn list_opencode_models_with_timeout(
    bin: &str,
    timeout_secs: u64,
) -> Result<Vec<String>, String> {
    let child = spawn_opencode(bin, |cmd| {
        cmd.arg("models");
    })
    .await?;

    let output = timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
        .await
        .map_err(|_| format!("opencode models timed out after {timeout_secs}s"))?
        .map_err(|err| format!("opencode models failed to complete: {err}"))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!(
            "opencode models exited with {}: {}",
            exit_label(&output.status),
            failure_detail(&stderr, &stdout)
        ));
    }

    Ok(stdout
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

async fn spawn_opencode(
    bin: &str,
    configure: impl Fn(&mut Command),
) -> Result<tokio::process::Child, String> {
    if bin.is_empty() {
        return Err(
            "cannot run opencode: binary path is empty (CONJURE_OPENCODE_BIN is empty)".to_string(),
        );
    }

    // OpenCode waits for stdin EOF when stdin is not a tty, even if the
    // prompt was passed as argv. A parent whose stdin is an open pipe
    // (ssh, CI, an IDE) would otherwise hang until the run timeout.
    // cwd and the parent env are inherited on purpose: OpenCode needs
    // HOME/PATH and discovers project config from the working directory.
    // No credential variables are injected; OpenCode owns auth.
    let mut last_busy = None;
    for attempt in 0..4 {
        let mut cmd = Command::new(bin);
        configure(&mut cmd);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 3 => {
                last_busy = Some(err);
                tokio::time::sleep(Duration::from_millis(20 * (attempt as u64 + 1))).await;
            }
            Err(err) => return Err(format!("cannot run opencode at {bin}: {err}")),
        }
    }

    Err(format!(
        "cannot run opencode at {bin}: {}",
        last_busy
            .as_ref()
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| "executable file busy".to_string())
    ))
}

fn failure_detail(stderr: &str, stdout: &str) -> String {
    let stderr_snip: String = stderr.trim().chars().take(300).collect();
    if !stderr_snip.is_empty() {
        return stderr_snip;
    }
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if event.get("type").and_then(|kind| kind.as_str()) != Some("error") {
            continue;
        }
        if let Some(message) = event
            .pointer("/error/message")
            .and_then(|message| message.as_str())
            .filter(|message| !message.is_empty())
        {
            return message.chars().take(300).collect();
        }
    }
    stdout.trim().chars().take(300).collect()
}

fn exit_label(status: &std::process::ExitStatus) -> String {
    status
        .code()
        .map_or_else(|| format!("{status}"), |code| code.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    use std::{fs, process};

    use super::*;

    #[cfg(unix)]
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    static ENV_LOCK: Mutex<()> = Mutex::new(());
    #[cfg(unix)]
    static STUB_SEQ: AtomicUsize = AtomicUsize::new(0);

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn with_env_var<T>(key: &str, value: Option<&str>, body: impl FnOnce() -> T) -> T {
        let original = std::env::var(key).ok();
        match value {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
        let result = body();
        match original {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
        result
    }

    #[cfg(unix)]
    struct Stub {
        path: PathBuf,
        cleanup_dir: Option<PathBuf>,
    }

    #[cfg(unix)]
    impl Drop for Stub {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
            if let Some(dir) = self.cleanup_dir.take() {
                let _ = fs::remove_dir_all(dir);
            }
        }
    }

    /// Unique per call, not just per process. Parallel tests must never share
    /// an executable path: writing a script another test is exec'ing is ETXTBSY.
    #[cfg(unix)]
    fn stub_nonce(label: &str) -> String {
        let seq = STUB_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0);
        format!(
            "conjure-opencode-test-{}-{seq}-{nanos}-{label}",
            process::id()
        )
    }

    #[cfg(unix)]
    fn write_script(path: &Path, body: &str) {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(path)
            .unwrap_or_else(|err| panic!("failed to create stub script {}: {err}", path.display()));
        file.write_all(body.as_bytes())
            .unwrap_or_else(|err| panic!("failed to write stub script {}: {err}", path.display()));
        file.sync_all()
            .unwrap_or_else(|err| panic!("failed to sync stub script {}: {err}", path.display()));
        drop(file);
        // umask can clear the execute bit that mode() requested.
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|err| panic!("failed to chmod stub script {}: {err}", path.display()));
    }

    #[cfg(unix)]
    fn write_stub_at(path: &Path, body: &str) -> Stub {
        write_script(path, body);
        Stub {
            path: path.to_owned(),
            cleanup_dir: None,
        }
    }

    #[cfg(unix)]
    fn write_stub(name: &str, body: &str) -> Stub {
        write_isolated(name, |_| body.to_string())
    }

    /// Private directory per stub so two tests can never share an inode.
    #[cfg(unix)]
    fn write_isolated(name: &str, body: impl FnOnce(&Path) -> String) -> Stub {
        let dir = std::env::temp_dir().join(stub_nonce(name));
        fs::create_dir(&dir)
            .unwrap_or_else(|err| panic!("failed to create stub dir {}: {err}", dir.display()));
        let path = dir.join("stub.sh");
        write_script(&path, &body(&dir));
        Stub {
            path,
            cleanup_dir: Some(dir),
        }
    }

    #[cfg(target_os = "linux")]
    fn process_still_running(pid: u32) -> bool {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => {
                let Some(end) = stat.rfind(')') else {
                    return true;
                };
                let state = stat[end + 1..].trim_start().chars().next();
                !matches!(state, Some('Z' | 'X') | None)
            }
            Err(_) => false,
        }
    }

    #[cfg(unix)]
    fn marker_value(text: &str, key: &str) -> String {
        let prefix = format!("{key}=");
        text.lines()
            .find_map(|line| line.strip_prefix(&prefix).map(str::to_string))
            .unwrap_or_default()
    }

    #[cfg(unix)]
    async fn chat_with_stub(name: &str, body: &str) -> Result<String, String> {
        let stub = write_stub(name, body);
        let cli = OpenCodeCli::new(stub.path.to_string_lossy(), "test-model");
        cli.chat(&[Message::user("hi")]).await
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_returns_text_part_from_ndjson() {
        let result = chat_with_stub(
            "happy",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  printf '%s\n' '{"type":"step_start","part":{}}'
  printf '%s\n' '{"type":"text","part":{"text":"ok"}}'
  printf '%s\n' '{"type":"step_finish","part":{}}'
fi
"#,
        )
        .await;

        assert_eq!(result.unwrap(), "ok");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_passes_expected_argv() {
        let result = chat_with_stub(
            "argv",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  printf '{"type":"text","part":{"text":"args: %s"}}\n' "$*"
fi
"#,
        )
        .await;

        let args = result.unwrap();
        assert!(
            args.contains("--format json --archived --model"),
            "got: {args}"
        );
        assert!(args.contains("test-model"), "got: {args}");
        assert!(args.contains("--agent conjure"), "got: {args}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_reports_non_zero_exit() {
        let result = chat_with_stub(
            "exit7",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  echo boom >&2
  exit 7
fi
"#,
        )
        .await;

        let err = result.unwrap_err();
        assert!(err.contains("exited with 7"), "got: {err}");
        assert!(err.contains("boom"), "got: {err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_times_out() {
        let stub = write_stub(
            "timeout",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  sleep 3
  printf '%s\n' '{"type":"text","part":{"text":"late"}}'
fi
"#,
        );
        let mut cli = OpenCodeCli::new(stub.path.to_string_lossy(), "test-model");
        cli.timeout_secs = 1;

        let started = Instant::now();
        let result = cli.chat(&[Message::user("hi")]).await;
        let elapsed = started.elapsed();

        let err = result.unwrap_err();
        assert!(err.contains("timed out after 1s"), "got: {err}");
        assert!(
            (Duration::from_millis(900)..=Duration::from_secs(2)).contains(&elapsed),
            "timeout fired after {elapsed:?} instead of ~1s"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_with_zero_timeout_errors_immediately() {
        let stub = write_stub(
            "zero-timeout",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  sleep 3
fi
"#,
        );
        let mut cli = OpenCodeCli::new(stub.path.to_string_lossy(), "test-model");
        cli.timeout_secs = 0;

        let started = Instant::now();
        let result = cli.chat(&[Message::user("hi")]).await;
        let elapsed = started.elapsed();

        let err = result.unwrap_err();
        assert!(err.contains("timed out after 0s"), "got: {err}");
        assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_joins_multiple_text_parts() {
        let result = chat_with_stub(
            "multipart",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  printf '%s\n' '{"type":"text","part":{"text":"a"}}'
  printf '%s\n' '{"type":"text","part":{"text":"b"}}'
fi
"#,
        )
        .await;

        assert_eq!(result.unwrap(), "ab");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_skips_malformed_lines_and_keeps_text_verbatim() {
        let stub = write_stub(
            "ndjson-adversarial",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  printf '%s\n' '{"type":"text","part":{"text":"line1\n \"q\" `tick` $var tail"}}'
  printf '%s\n' '{"type":"text","part":"not-an-object"}'
  printf '%s\n' '{"type":"text","part":{"text":42}}'
  printf '%s\n' '{"type":"step_start","part":{"text":"nope"}}'
  printf '%s\n' 'not json at all'
  printf '%s\n' '{"type":"text","part":{"text":"dup"}}'
  printf '%s\n' '{"type":"text","part":{"text":"dup"}}'
  printf '%s\n' '{"type":"text","part":{"text":"b2"}}'
fi
"#,
        );
        let cli = OpenCodeCli::new(stub.path.to_string_lossy(), "test-model");
        let result = cli.chat(&[Message::user("hi")]).await;

        let fragment = "line1\n \"q\" `tick` $var tail";
        let expected = format!("{fragment}dupdupb2");
        assert_eq!(result.unwrap(), expected);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_accepts_a_very_large_single_line() {
        let payload = "x".repeat(5 * 1024 * 1024);
        let body = r#"#!/bin/sh
if [ "$1" = "run" ]; then
  printf '%s\n' '{"type":"text","part":{"text":"__PAYLOAD__"}}'
fi
"#
        .replace("__PAYLOAD__", &payload);
        let result = chat_with_stub("large-line", &body).await;
        assert_eq!(result.unwrap(), payload);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_preserves_order_across_many_small_lines() {
        let lines = (0..10_000)
            .map(|i| {
                format!("  printf '%s\\n' '{{\"type\":\"text\",\"part\":{{\"text\":\"{i}\"}}}}'\n")
            })
            .collect::<String>();
        let body = format!("#!/bin/sh\nif [ \"$1\" = \"run\" ]; then\n{lines}fi\n");
        let expected = (0..10_000).map(|i| i.to_string()).collect::<String>();
        let result = chat_with_stub("many-lines", &body).await;
        assert_eq!(result.unwrap(), expected);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_inherits_the_parent_environment() {
        let stub = write_stub(
            "inherit-env",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  if [ -n "$HOME" ] && [ -n "$PATH" ]; then
    printf '%s\n' '{"type":"text","part":{"text":"env-ok"}}'
  fi
fi
"#,
        );
        let cli = OpenCodeCli::new(stub.path.to_string_lossy(), "test-model");
        let result = cli.chat(&[Message::user("hi")]).await;
        assert_eq!(result.unwrap(), "env-ok");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_passes_metacharacter_prompts_without_shell_interpretation() {
        let stub = write_stub(
            "nasty-prompt",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  last=
  for arg in "$@"; do last=$arg; done
  last=$(printf '%s' "$last" | tr '\n' '~')
  last=$(printf '%s' "$last" | sed 's/"/\\"/g')
  printf '%s\n' "{\"type\":\"text\",\"part\":{\"text\":\"got:$last\"}}"
fi
"#,
        );
        let cli = OpenCodeCli::new(stub.path.to_string_lossy(), "test-model");
        let nasty = "multi\nline \"quoted\" $HOME `tick` $(boom) 100%";
        let result = cli.chat(&[Message::user(nasty)]).await;

        let expected = "got:user: multi~line \"quoted\" $HOME `tick` $(boom) 100%";
        assert_eq!(result.unwrap(), expected);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_handles_bin_path_containing_spaces() {
        let dir =
            std::env::temp_dir().join(format!("conjure opencode spaces {}", stub_nonce("dir")));
        fs::create_dir_all(&dir).expect("failed to create stub dir");
        let mut stub = write_stub_at(
            &dir.join("stub with spaces.sh"),
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  printf '%s\n' '{"type":"text","part":{"text":"space-ok"}}'
fi
"#,
        );
        stub.cleanup_dir = Some(dir);
        let cli = OpenCodeCli::new(stub.path.to_string_lossy(), "test-model");
        let result = cli.chat(&[Message::user("hi")]).await;
        assert_eq!(result.unwrap(), "space-ok");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_passes_model_with_slashes_and_spaces_verbatim() {
        let stub = write_stub(
            "model-space",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  after=0
  model=
  for arg in "$@"; do
    if [ "$after" = "1" ]; then model=$arg; after=0; fi
    if [ "$arg" = "--model" ]; then after=1; fi
  done
  printf '%s\n' "{\"type\":\"text\",\"part\":{\"text\":\"$model\"}}"
fi
"#,
        );
        let cli = OpenCodeCli::new(stub.path.to_string_lossy(), "prov/vision lab");
        let result = cli.chat(&[Message::user("hi")]).await;
        assert_eq!(result.unwrap(), "prov/vision lab");
    }

    #[tokio::test]
    async fn chat_with_empty_bin_reports_actionable_error() {
        let cli = OpenCodeCli::new("", "test-model");
        let err = cli.chat(&[Message::user("hi")]).await.unwrap_err();
        assert!(err.contains("cannot run opencode"), "got: {err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn list_opencode_models_returns_trimmed_lines() {
        let stub = write_stub(
            "models",
            r#"#!/bin/sh
if [ "$1" = "models" ]; then
  printf 'm1\nm2\n'
fi
"#,
        );

        let result = list_opencode_models(&stub.path.to_string_lossy()).await;
        assert_eq!(result.unwrap(), vec!["m1".to_string(), "m2".to_string()]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn list_opencode_models_keeps_garbage_lines_and_drops_empties() {
        let stub = write_stub(
            "models-garbage",
            r#"#!/bin/sh
if [ "$1" = "models" ]; then
  printf '  g1  \n\n   \nnot-json {{{\ng2\n'
fi
"#,
        );

        let result = list_opencode_models(&stub.path.to_string_lossy()).await;
        assert_eq!(
            result.unwrap(),
            vec![
                "g1".to_string(),
                "not-json {{{".to_string(),
                "g2".to_string()
            ]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn list_opencode_models_survives_non_utf8_output() {
        let stub = write_stub(
            "models-bad-utf8",
            r#"#!/bin/sh
if [ "$1" = "models" ]; then
  printf 'bad\377beef\n\377\n'
fi
"#,
        );

        let result = list_opencode_models(&stub.path.to_string_lossy()).await;
        assert_eq!(
            result.unwrap(),
            vec!["bad\u{FFFD}beef".to_string(), "\u{FFFD}".to_string()]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn list_opencode_models_missing_bin_errors_actionably() {
        let missing = std::env::temp_dir().join(format!("{}.missing", stub_nonce("absent")));
        let result = list_opencode_models(&missing.to_string_lossy()).await;
        let err = result.unwrap_err();
        assert!(err.contains("cannot run opencode"), "got: {err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn list_opencode_models_times_out() {
        let _env = lock_env();
        let stub = write_stub(
            "models-timeout",
            r#"#!/bin/sh
if [ "$1" = "models" ]; then
  sleep 30
fi
"#,
        );
        drop(_env);

        let started = Instant::now();
        let result = list_opencode_models_with_timeout(&stub.path.to_string_lossy(), 1).await;
        let elapsed = started.elapsed();

        let err = result.unwrap_err();
        assert_eq!(err, "opencode models timed out after 1s");
        assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");
    }

    #[tokio::test]
    async fn opencode_bin_defaults_to_opencode() {
        let _env = lock_env();
        let bin = with_env_var("CONJURE_OPENCODE_BIN", None, opencode_bin);
        assert_eq!(bin, "opencode");
    }

    #[tokio::test]
    async fn opencode_bin_empty_when_env_var_is_set_to_empty_string() {
        let _env = lock_env();
        let bin = with_env_var("CONJURE_OPENCODE_BIN", Some(""), opencode_bin);
        assert!(bin.is_empty());
    }

    #[tokio::test]
    async fn empty_opencode_bin_env_is_an_actionable_chat_and_models_error() {
        let _env = lock_env();
        let bin = with_env_var("CONJURE_OPENCODE_BIN", Some(""), opencode_bin);
        drop(_env);
        assert!(bin.is_empty());

        let chat_err = OpenCodeCli::new(&bin, "prov/vision lab")
            .chat(&[Message::user("hi\n$x `y` \"z\"")])
            .await
            .unwrap_err();
        assert!(chat_err.contains("cannot run opencode"), "{chat_err}");
        assert!(
            chat_err.contains("CONJURE_OPENCODE_BIN") && chat_err.contains("empty"),
            "{chat_err}"
        );

        let models_err = list_opencode_models(&bin).await.unwrap_err();
        assert!(models_err.contains("cannot run opencode"), "{models_err}");
        assert!(models_err.contains("empty"), "{models_err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_closes_stdin_so_a_pipe_cannot_stall_the_run() {
        let result = chat_with_stub(
            "stdin-null",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  target=$(readlink /proc/self/fd/0 2>/dev/null || echo missing)
  printf '%s\n' "{\"type\":\"text\",\"part\":{\"text\":\"$target\"}}"
fi
"#,
        )
        .await;

        let got = result.unwrap();
        assert!(
            got == "/dev/null",
            "stdin was {got}; opencode run blocks until a non-tty stdin closes"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_surfaces_stdout_error_event_when_stderr_is_empty() {
        let result = chat_with_stub(
            "stdout-error",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  printf '%s\n' '{"type":"error","error":{"message":"Agent not found: \"conjure\""}}'
  exit 1
fi
"#,
        )
        .await;

        let err = result.unwrap_err();
        assert!(err.contains("exited with 1"), "{err}");
        assert!(err.contains("Agent not found"), "{err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_includes_error_event_when_text_is_missing() {
        let result = chat_with_stub(
            "error-no-text",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  printf '%s\n' '{"type":"error","error":{"message":"model unavailable"}}'
fi
"#,
        )
        .await;

        let err = result.unwrap_err();
        assert!(err.contains("no text output"), "{err}");
        assert!(err.contains("model unavailable"), "{err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_survives_non_utf8_stdout_and_keeps_later_text() {
        let result = chat_with_stub(
            "chat-bad-utf8",
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  printf 'bad\377\n'
  printf '%s\n' '{"type":"text","part":{"text":"kept"}}'
fi
"#,
        )
        .await;

        assert_eq!(result.unwrap(), "kept");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn chat_timeout_kills_the_child() {
        let stub = write_isolated("kill-child", |dir| {
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"run\" ]; then\n  echo $$ > '{}'\n  sleep 30 &\n  echo $! > '{}'\n  wait\nfi\n",
                dir.join("shell.pid").display(),
                dir.join("sleep.pid").display()
            )
        });
        let dir = stub.path.parent().expect("stub dir").to_owned();
        let mut cli = OpenCodeCli::new(stub.path.to_string_lossy(), "test-model");
        cli.timeout_secs = 1;

        let started = Instant::now();
        let err = cli.chat(&[Message::user("hi")]).await.unwrap_err();
        let elapsed = started.elapsed();
        assert!(err.contains("timed out after 1s"), "{err}");
        assert!(
            elapsed < Duration::from_secs(3),
            "timeout returned after {elapsed:?}"
        );

        let shell_pid: u32 = fs::read_to_string(dir.join("shell.pid"))
            .expect("shell pid file")
            .trim()
            .parse()
            .expect("shell pid");
        let sleep_pid = fs::read_to_string(dir.join("sleep.pid"))
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok());

        let mut running = true;
        for _ in 0..50 {
            running = process_still_running(shell_pid);
            if !running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if let Some(pid) = sleep_pid {
            let _ = process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = process::Command::new("kill")
            .args(["-9", &shell_pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        assert!(
            !running,
            "child {shell_pid} still running after timeout (kill_on_drop)"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_inherits_cwd_and_env_without_injecting_secrets() {
        let parent_home = std::env::var("HOME").unwrap_or_default();
        let parent_path = std::env::var("PATH").unwrap_or_default();
        let parent_openai = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        let parent_anthropic = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
        let cwd = std::env::current_dir().expect("cwd");
        let stub = write_isolated("inherit-all", |dir| {
            r#"#!/bin/sh
if [ "$1" = "run" ]; then
  {
    printf 'PWD=%s\n' "$PWD"
    printf 'HOME=%s\n' "$HOME"
    printf 'PATH=%s\n' "$PATH"
    printf 'OPENAI_API_KEY=%s\n' "$OPENAI_API_KEY"
    printf 'ANTHROPIC_API_KEY=%s\n' "$ANTHROPIC_API_KEY"
    printf 'CONJURE_TEST_SECRET=%s\n' "$CONJURE_TEST_SECRET"
  } > '__MARKER__'
  printf '%s\n' '{"type":"text","part":{"text":"env-ok"}}'
fi
"#
            .replace("__MARKER__", &dir.join("env.txt").display().to_string())
        });
        let marker = stub.path.parent().expect("stub dir").join("env.txt");
        let result = OpenCodeCli::new(stub.path.to_string_lossy(), "test-model")
            .chat(&[Message::user("hi")])
            .await;
        assert_eq!(result.unwrap(), "env-ok");

        let dump = fs::read_to_string(&marker).expect("env dump");
        let child_pwd = PathBuf::from(marker_value(&dump, "PWD"));
        let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.clone());
        assert!(
            child_pwd == cwd || child_pwd == canonical,
            "child pwd {child_pwd:?} did not inherit cwd {cwd:?}"
        );
        assert_eq!(
            marker_value(&dump, "HOME"),
            parent_home,
            "HOME was stripped"
        );
        assert_eq!(
            marker_value(&dump, "PATH"),
            parent_path,
            "PATH was stripped"
        );
        assert_eq!(
            marker_value(&dump, "OPENAI_API_KEY"),
            parent_openai,
            "OPENAI_API_KEY was injected or stripped"
        );
        assert_eq!(
            marker_value(&dump, "ANTHROPIC_API_KEY"),
            parent_anthropic,
            "ANTHROPIC_API_KEY was injected or stripped"
        );
        assert!(
            marker_value(&dump, "CONJURE_TEST_SECRET").is_empty(),
            "injected CONJURE_TEST_SECRET"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_passes_nasty_model_and_prompt_as_single_argv_elements() {
        let stub = write_isolated("argv-raw", |dir| {
            let args_path = dir.join("args");
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"run\" ]; then\n  : > '{}'\n  for arg in \"$@\"; do\n    printf '%s\\0' \"$arg\" >> '{}'\n  done\n  printf '%s\\n' '{{\"type\":\"text\",\"part\":{{\"text\":\"argv-ok\"}}}}'\nfi\n",
                args_path.display(),
                args_path.display()
            )
        });
        let model = "prov/vision lab $HOME `tick` \"q\" $(nope)";
        let prompt = "multi\nline \"quoted\" $HOME `tick` $(boom)";
        let result = OpenCodeCli::new(stub.path.to_string_lossy(), model)
            .chat(&[Message::user(prompt)])
            .await
            .unwrap();
        assert_eq!(result, "argv-ok");

        let bytes =
            fs::read(stub.path.parent().expect("stub dir").join("args")).expect("argv dump");
        let args: Vec<String> = bytes
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect();
        assert_eq!(args.first().map(String::as_str), Some("run"));
        let model_at = args
            .iter()
            .position(|arg| arg == "--model")
            .expect("model flag");
        assert_eq!(args.get(model_at + 1).map(String::as_str), Some(model));
        assert_eq!(
            args.last().map(String::as_str),
            Some(format!("user: {prompt}")).as_deref()
        );
    }
}
