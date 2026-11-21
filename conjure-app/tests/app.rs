//! A10 acceptance tests: the spec's eight scenarios as in-process rune runtimes,
//! plus the CLI `--snap` smoke.
//!
//! Scenarios 1–5 are UI-only and drive a bare `rune::Runtime` (the hello-example
//! pattern: `frame`, `find`, `pointer_down`/`pointer_up`, `state`). Scenarios
//! 6–8 wire the app like `main.rs` — `App::new`, `handle`, `conjure_app::r#gen::spawn`,
//! `handle.update` — and run rune's own in-process headless host
//! (`rune::tooling::headless`) with a `--script` of keyed taps and `--realtime`
//! pacing, because an `AppHandle`'s queued updates are drained only by a rune
//! event loop. The app's `poll` records a snapshot of the gen panel every frame,
//! which the tests read back as the bounded-poll trace. Scenario 9 spawns the
//! `conjure-app` binary for `--snap`.

use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use conjure_core::style::{Dimensions, Grid, Outline, PaletteColor, Shading, Style};
use conjure_core::{Asset, AssetKind, AssetRequest, StyleCard, verify_request};
use conjure_store::{ListFilter, Store};
use httpmock::{Method, MockServer};
use rune::tooling::{Args, headless};
use rune::{App, AppSpec, Env, FileDialogKind, Runtime, Tx};

use conjure_app::r#gen::GenCommand;
use conjure_app::state::{AppState, RunState, Screen};

/// The garbage the mock model replies before the repair turn, and the tileset
/// spec it repairs with: three 8×8 tiles over a four-color palette (chars
/// '0'-'3'), matching the panel's default tileset shape (8×8 ×3).
const GARBAGE_REPLY: &str = "sorry, i cannot draw that.";
const TILESET_SPEC: &str = r#"{"type":"tileset","tiles":[["00000000","01111110","01222210","01222210","01222210","01222210","01111110","00000000"],["00000000","03333300","01333310","01033310","01033310","01333310","03333300","00000000"],["00000000","01210330","01201330","01201330","01201330","01201330","01210330","00000000"]]}"#;

// ---------- nonce temp fixtures with Drop cleanup ----------

fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or(0)
}

struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_root(label: &str) -> TempRoot {
    TempRoot(std::env::temp_dir().join(format!(
        "conjure-app-acceptance-{}-{}-{label}",
        std::process::id(),
        nonce()
    )))
}

struct TempScript(PathBuf);

impl Drop for TempScript {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn script_file(body: &str) -> TempScript {
    let path = std::env::temp_dir().join(format!(
        "conjure-app-script-{}-{}",
        std::process::id(),
        nonce()
    ));
    std::fs::write(&path, body).unwrap();
    TempScript(path)
}

struct Stub {
    path: PathBuf,
    dir: PathBuf,
}

impl Drop for Stub {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A private directory per stub so two tests can never share an inode, as
/// conjure-inference's opencode tests discipline requires.
fn write_stub(label: &str, body: &str) -> Stub {
    let dir = std::env::temp_dir().join(format!(
        "conjure-app-stub-{}-{}-{label}",
        std::process::id(),
        nonce()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("stub.sh");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o755)
        .open(&path)
        .unwrap();
    file.write_all(body.as_bytes()).unwrap();
    file.sync_all().unwrap();
    drop(file);
    // umask can clear the execute bit that mode() requested.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    Stub { path, dir }
}

/// Sets an env var and restores the original value when dropped, so a panicking
/// assertion can never leak a stub path into the process env.
struct EnvVar {
    key: &'static str,
    original: Option<String>,
}

impl Drop for EnvVar {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

fn set_env_var(key: &'static str, value: &str) -> EnvVar {
    let original = std::env::var(key).ok();
    unsafe { std::env::set_var(key, value) };
    EnvVar { key, original }
}

// ---------- shared conjure fixtures ----------

fn demo_card() -> StyleCard {
    StyleCard {
        id: "card-1".into(),
        name: "card one".into(),
        palette: vec![
            PaletteColor {
                name: "a".into(),
                hex: "#000000".into(),
            },
            PaletteColor {
                name: "b".into(),
                hex: "#111111".into(),
            },
            PaletteColor {
                name: "c".into(),
                hex: "#222222".into(),
            },
            PaletteColor {
                name: "d".into(),
                hex: "#333333".into(),
            },
        ],
        grid: Grid {
            tile_size: Dimensions { w: 16, h: 16 },
            unit_scale: 1,
        },
        style: Style {
            outline: Outline::None,
            outline_color: "#000000".into(),
            shading: Shading::Flat,
            proportions: "1:1".into(),
            era: "modern".into(),
            theme: "pixel".into(),
        },
    }
}

fn demo_request() -> AssetRequest {
    AssetRequest {
        name: "demo".into(),
        kind: AssetKind::Sprite {
            size: Dimensions { w: 16, h: 16 },
        },
        style_card: demo_card(),
    }
}

/// Creates a store under `root` and saves `conjure_core::generate(demo_request, 42)`.
fn seed_store(root: &Path) -> Vec<String> {
    let store = Store::create(root).unwrap();
    let request = demo_request();
    let assets = conjure_core::generate(&request, 42);
    store.save_assets(&request, &assets).unwrap()
}

fn hash8(hash: &str) -> &str {
    &hash[..hash.len().min(8)]
}

// ---------- bare-runtime helpers (the hello-example pattern) ----------

fn runtime_for(root: &Path) -> Runtime<AppState> {
    Runtime::new(
        AppState::init_from(root),
        conjure_app::ui::view,
        (1100.0, 760.0),
        Env::new(true, 1.0),
    )
}

fn tap(rt: &mut Runtime<AppState>, key: &str) {
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("no element keyed {key:?}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
    // Let the layout's growth motion settle before the next press: a mid-flight
    // drawn rect makes the next find's point stale, and the press misses.
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

// ---------- the headless gen harness ----------

/// Serializes the gen tests (and the CLI smoke): they mutate process env vars
/// and run GPU-backed headless passes.
static GEN_LOCK: Mutex<()> = Mutex::new(());

fn gen_lock() -> MutexGuard<'static, ()> {
    GEN_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Debug)]
struct Snapshot {
    clock: f64,
    state: RunState,
    events: Vec<String>,
    note: Option<String>,
    notice: Option<String>,
    pending: Option<(AssetRequest, Vec<Asset>)>,
    names: Vec<String>,
    models: usize,
    opencode: usize,
}

static TRACE: Mutex<Vec<Snapshot>> = Mutex::new(Vec::new());

/// The app's per-frame `poll`: records what the gen cockpit came to, as the
/// bounded-poll trace the tests assert against.
fn record(s: &mut AppState, clock: f64, _tx: &mut Tx) -> bool {
    let mut trace = TRACE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    trace.push(Snapshot {
        clock,
        state: s.r#gen.state,
        events: s.r#gen.events.clone(),
        note: s.r#gen.note.clone(),
        notice: s.notice.clone(),
        pending: s
            .r#gen
            .pending
            .as_ref()
            .map(|pending| (pending.request.clone(), pending.assets.clone())),
        names: s.assets.iter().map(|row| row.name.clone()).collect(),
        models: s.r#gen.models.len(),
        opencode: s.r#gen.opencode_models.len(),
    });
    false
}

fn gen_app(root: &Path) -> App<AppState> {
    let spec = AppSpec {
        poll: Some(record),
        ..AppSpec::new(
            "Conjure",
            (1100.0, 760.0),
            AppState::init,
            conjure_app::ui::view,
        )
    };
    let root = root.to_path_buf();
    App::new(spec).init(move || AppState::init_from(&root))
}

fn run_headless(app: App<AppState>, script: &str, frames: usize) -> Vec<Snapshot> {
    TRACE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    let script = script_file(script);
    let args = Args::parse_from(vec![
        "--script".into(),
        script.0.to_string_lossy().into_owned(),
        "--frames".into(),
        frames.to_string(),
        "--fps".into(),
        "30".into(),
        "--realtime".into(),
        "--scale".into(),
        "1".into(),
    ]);
    let started = Instant::now();
    headless(app, args);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(10),
        "the headless run took {elapsed:?}, over the 10 s bound"
    );
    let trace = TRACE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert!(!trace.is_empty(), "the poll recorder saw no frames");
    trace
}

fn completion(reply: &str) -> serde_json::Value {
    serde_json::json!({
        "choices": [{ "message": { "role": "assistant", "content": reply } }]
    })
}

// ---------- scenarios 1-5: bare-runtime acceptance ----------

#[test]
fn no_library_screen_and_initialize() {
    let root = temp_root("no-library");
    std::fs::create_dir_all(&root.0).unwrap();

    let mut rt = runtime_for(&root.0);
    rt.frame(0.0);
    assert!(matches!(rt.state.screen, Screen::NoLibrary));
    assert!(
        rt.find("initialize").is_some(),
        "the initialize control renders"
    );

    tap(&mut rt, "initialize");
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }

    assert!(matches!(rt.state.screen, Screen::Browse));
    assert!(
        root.0.join(".conjure").join("conjure.sqlite").exists(),
        "the initialize press created the store under the root"
    );
}

#[test]
fn empty_store_shows_empty_state() {
    let root = temp_root("empty-store");
    Store::create(&root.0).unwrap();

    let mut rt = runtime_for(&root.0);
    rt.frame(0.0);

    assert!(matches!(rt.state.screen, Screen::Browse));
    assert!(rt.state.cards.is_empty());
    assert!(rt.state.assets.is_empty());
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.find("browse").is_some(), "the browse screen renders");
    assert!(rt.find("gen").is_some(), "the gen cockpit renders");
}

#[test]
fn seeded_browse_and_select() {
    let root = temp_root("seeded-browse");
    let hashes = seed_store(&root.0);

    let mut rt = runtime_for(&root.0);
    rt.frame(0.0);

    let card_key = format!("browse-card-{}", hash8(&rt.state.cards[0].hash));
    let asset_key = format!("browse-asset-{}", hash8(&rt.state.assets[0].hash));
    assert!(rt.find(&card_key).is_some(), "the card row renders");
    assert_eq!(rt.state.assets[0].hash, hashes[0]);

    // The asset rows render under the chosen card; select it first.
    tap(&mut rt, &card_key);
    assert!(
        rt.find(&asset_key).is_some(),
        "the asset row renders once its card is chosen"
    );

    tap(&mut rt, &asset_key);
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }

    let preview = rt.state.preview.as_ref().unwrap();
    assert_eq!(
        (preview.w, preview.h),
        (16, 16),
        "dims are the asset's at zoom 1"
    );
    assert_eq!(preview.zoom, 1);
    assert_eq!(preview.palette.len(), demo_card().palette.len());
    assert_eq!(
        rt.state.selection,
        vec![(
            hashes[0].clone(),
            rt.state
                .assets
                .iter()
                .find(|r| r.hash == hashes[0])
                .unwrap()
                .name
                .clone()
        )]
    );
    let (rect, _) = rt.presented("preview").expect("the preview pane renders");
    assert!(rect[2] > 0.0 && rect[3] > 0.0, "the preview is non-empty");
}

#[test]
fn zoom_cycles_composition() {
    let root = temp_root("zoom-cycle");
    seed_store(&root.0);

    let mut rt = runtime_for(&root.0);
    rt.frame(0.0);
    let card_key = format!("browse-card-{}", hash8(&rt.state.cards[0].hash));
    let asset_key = format!("browse-asset-{}", hash8(&rt.state.assets[0].hash));
    tap(&mut rt, &card_key);
    tap(&mut rt, &asset_key);
    assert_eq!(rt.state.zoom, 1);

    let dims = |rt: &Runtime<AppState>| {
        let preview = rt.state.preview.as_ref().unwrap();
        (preview.w, preview.h)
    };

    tap(&mut rt, "zoom-in");
    assert_eq!(rt.state.zoom, 2);
    assert_eq!(dims(&rt), (32, 32));
    tap(&mut rt, "zoom-in");
    assert_eq!(rt.state.zoom, 4);
    assert_eq!(dims(&rt), (64, 64));

    tap(&mut rt, "zoom-out");
    assert_eq!(rt.state.zoom, 2, "zoom-out steps back");
    assert_eq!(dims(&rt), (32, 32));

    // Finish the cycle: 2 -> 4 -> 8, then the next press wraps to 1.
    tap(&mut rt, "zoom-in");
    assert_eq!(rt.state.zoom, 4);
    tap(&mut rt, "zoom-in");
    assert_eq!(rt.state.zoom, 8);
    assert_eq!(dims(&rt), (128, 128));
    tap(&mut rt, "zoom-in");
    assert_eq!(
        rt.state.zoom, 1,
        "the last press wraps the [1, 2, 4, 8] cycle"
    );
    assert_eq!(dims(&rt), (16, 16));
}

#[test]
fn export_via_scripted_dialog_is_byte_identical() {
    let root = temp_root("export-dialog");
    let out_dir = temp_root("export-out");
    let hashes = seed_store(&root.0);
    std::fs::create_dir_all(&out_dir.0).unwrap();

    let mut rt = runtime_for(&root.0);
    rt.frame(0.0);
    let card_key = format!("browse-card-{}", hash8(&rt.state.cards[0].hash));
    let asset_key = format!("browse-asset-{}", hash8(&rt.state.assets[0].hash));
    tap(&mut rt, &card_key);
    tap(&mut rt, &asset_key);

    tap(&mut rt, "export-run");
    let (kind, options) = rt
        .file_dialog()
        .expect("the export press asked for a folder");
    assert_eq!(kind, FileDialogKind::Folder);
    assert_eq!(options.title, "Choose an export folder");
    // The scripted-dialog answer: the headless fixture replies with out_dir.
    rt.resolve_file_dialog(Some(out_dir.0.clone())).unwrap();
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }

    let message = rt.state.export.message.as_deref().expect("export finished");
    let lines: Vec<&str> = message.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "export reports the png and the manifest: {message}"
    );
    let png_path = PathBuf::from(lines[0]);
    let json_path = PathBuf::from(lines[1]);
    assert!(png_path.ends_with("demo.png"), "got: {message}");
    assert!(json_path.ends_with("demo.json"), "got: {message}");

    let store = Store::open(&root.0).unwrap();
    let card_hash = rt.state.assets[0].card_hash.clone();
    let card = store.load_card(&card_hash).unwrap();
    let asset = store.load_asset(&hashes[0]).unwrap();
    let opts = conjure_export::ExportOpts {
        layout: conjure_export::SheetLayout::StripHorizontal,
        scale: 1,
    };
    let expected_png =
        conjure_export::export_png(&card, std::slice::from_ref(&asset), &opts).unwrap();
    let mut expected_manifest =
        conjure_export::export_manifest(&card, &[asset], &["demo".to_string()], &opts).unwrap();
    expected_manifest.style_card.hash = Some(card_hash);
    let expected_json = serde_json::to_string_pretty(&expected_manifest).unwrap();

    assert_eq!(std::fs::read(&png_path).unwrap(), expected_png);
    assert_eq!(std::fs::read_to_string(&json_path).unwrap(), expected_json);
}

// ---------- scenarios 6-8: the gen pipeline through the app wiring ----------

#[test]
fn generate_with_httpmock_streams_events_and_saves() {
    let _guard = gen_lock();
    let root = temp_root("gen-httpmock");
    seed_store(&root.0);
    let rows_before = Store::open(&root.0)
        .unwrap()
        .list_assets(&ListFilter { tag: None })
        .unwrap()
        .len();
    let card_hash = AppState::init_from(&root.0).cards[0].hash.clone();

    // GARBAGE first, a valid CompactSpec on the repair turn — the canned
    // conjure-agent pattern (the repair request body includes "not valid JSON").
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(Method::POST)
            .path("/chat/completions")
            .body_excludes("not valid JSON");
        then.status(200).json_body(completion(GARBAGE_REPLY));
    });
    server.mock(|when, then| {
        when.method(Method::POST)
            .path("/chat/completions")
            .body_includes("not valid JSON");
        then.status(200).json_body(completion(TILESET_SPEC));
    });

    let app = gen_app(&root.0);
    let handle = app.handle();
    let tx = conjure_app::r#gen::spawn(handle.clone());
    let mock_url = server.base_url();
    handle.update(move |s, _| {
        s.gen_tx = Some(tx);
        s.r#gen.models = vec![(mock_url.clone(), "mock-model".to_string())];
        s.r#gen.model = "mock-model".to_string();
        s.r#gen.base_url = mock_url;
        s.r#gen.card_hash = Some(card_hash);
        s.r#gen.name = "test-gen".to_string();
    });

    let trace = run_headless(
        app,
        r#"[{"t":0.1,"key":"gen-run"},{"t":2.5,"key":"gen-save"}]"#,
        90,
    );

    let passed = trace
        .iter()
        .find(|snapshot| snapshot.state == RunState::Passed)
        .expect("the run reached Passed");
    let (request, assets) = passed
        .pending
        .as_ref()
        .expect("Passed carries pending assets");
    assert_eq!(request.name, "test-gen");
    assert_eq!(assets.len(), 3, "the tileset spec expanded to three tiles");
    assert!(
        verify_request(request, assets).passed,
        "the pending assets verify against the request"
    );

    let last = trace.last().unwrap();
    assert_eq!(last.notice.as_deref(), Some("saved 3 assets"));
    let expected: Vec<String> = [
        "model call",
        "repair turn 1",
        "model call",
        "spec parsed",
        "passed",
    ]
    .iter()
    .map(|event| event.to_string())
    .collect();
    assert_eq!(last.events, expected);
    for name in ["test-gen#0", "test-gen#1", "test-gen#2"] {
        assert!(
            last.names.iter().any(|row| row == name),
            "the store rows include {name}"
        );
    }
    let rows_after = Store::open(&root.0)
        .unwrap()
        .list_assets(&ListFilter { tag: None })
        .unwrap()
        .len();
    assert_eq!(rows_after, rows_before + 3);
}

#[test]
fn cancel_aborts_a_slow_model() {
    let _guard = gen_lock();
    let root = temp_root("gen-cancel");
    seed_store(&root.0);
    let rows_before = Store::open(&root.0)
        .unwrap()
        .list_assets(&ListFilter { tag: None })
        .unwrap()
        .len();
    let card_hash = AppState::init_from(&root.0).cards[0].hash.clone();

    // The backend that never answers in time: a stub that sleeps 30 s first.
    let stub = write_stub(
        "slow",
        "#!/bin/sh\nsleep 30\nprintf '%s\\n' '{\"type\":\"text\",\"part\":{\"text\":\"{}\"}}'\n",
    );
    let _env = set_env_var("CONJURE_OPENCODE_BIN", &stub.path.to_string_lossy());

    let app = gen_app(&root.0);
    let handle = app.handle();
    let tx = conjure_app::r#gen::spawn(handle.clone());
    handle.update(move |s, _| {
        s.gen_tx = Some(tx);
        s.r#gen.provider = "opencode".to_string();
        s.r#gen.base_url.clear();
        s.r#gen.model = "stub".to_string();
        s.r#gen.card_hash = Some(card_hash);
        s.r#gen.name = "test-cancel".to_string();
    });

    let trace = run_headless(
        app,
        r#"[{"t":0.1,"key":"gen-run"},{"t":2.0,"key":"gen-cancel"}]"#,
        75,
    );

    let running = trace.iter().find(|snapshot| {
        snapshot.clock < 1.9
            && snapshot.state == RunState::Running
            && snapshot
                .events
                .iter()
                .any(|event| event.as_str() == "model call")
    });
    assert!(
        running.is_some(),
        "the slow model was Running with its model call logged before the cancel"
    );

    let last = trace.last().unwrap();
    assert_eq!(
        last.state,
        RunState::Idle,
        "the cancel returned the panel to Idle"
    );
    assert!(
        last.events
            .iter()
            .any(|event| event.as_str() == "model call"),
        "the events log is kept after the cancel"
    );
    assert!(last.pending.is_none(), "nothing was generated");
    let rows_after = Store::open(&root.0)
        .unwrap()
        .list_assets(&ListFilter { tag: None })
        .unwrap()
        .len();
    assert_eq!(rows_after, rows_before, "nothing was saved");
}

#[test]
fn no_model_state_disables_run() {
    let _guard = gen_lock();
    let root = temp_root("gen-nomodel");
    seed_store(&root.0);

    // Discovery sees nothing: no local http server, and a failing opencode stub.
    let stub = write_stub("fail", "#!/bin/sh\nexit 1\n");
    let _env = set_env_var("CONJURE_OPENCODE_BIN", &stub.path.to_string_lossy());

    let app = gen_app(&root.0);
    let handle = app.handle();
    let tx = conjure_app::r#gen::spawn(handle.clone());
    let initial = tx.clone();
    handle.update(move |s, _| {
        s.gen_tx = Some(initial);
        s.r#gen.base_url.clear();
    });
    tx.send(GenCommand::Discover).unwrap();

    let trace = run_headless(app, r#"[{"t":1.0,"key":"gen-run"}]"#, 45);

    let last = trace.last().unwrap();
    assert_eq!(last.models, 0, "no http models were discovered");
    assert_eq!(last.opencode, 0, "no opencode models were discovered");
    assert_eq!(
        last.note.as_deref(),
        Some("no local model servers and no opencode models found")
    );
    assert_eq!(last.state, RunState::Idle);
    assert!(
        last.events.is_empty(),
        "the inert run control dispatched nothing"
    );
    assert!(last.pending.is_none());

    // ui.rs:602-606: that state renders the inert run control and the "no model
    // available" reason text; pressing the control changes nothing.
    let mut state = AppState::init_from(&root.0);
    state.r#gen.base_url.clear();
    let mut rt = Runtime::new(
        state,
        conjure_app::ui::view,
        (1100.0, 760.0),
        Env::new(true, 1.0),
    );
    rt.frame(0.0);
    let run = rt
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("gen-run"))
        .expect("gen-run renders");
    assert!(run.inert, "the run control is inert without any model");
    let note = rt
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("gen-note"))
        .expect("gen-note renders");
    assert_eq!(note.text.as_deref(), Some("no model available"));

    tap(&mut rt, "gen-run");
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(rt.state.r#gen.state, RunState::Idle);
    assert!(rt.state.r#gen.events.is_empty());
    assert!(rt.state.notice.is_none());
    assert!(rt.state.r#gen.pending.is_none());
}

// ---------- scenario 9: the CLI --snap smoke ----------

#[test]
fn snap_cli_smoke() {
    let _guard = gen_lock();
    let root = temp_root("snap-cli");
    seed_store(&root.0);
    let out = temp_root("snap-out");
    std::fs::create_dir_all(&out.0).unwrap();
    let png = out.0.join("app.png");

    let status = Command::new(env!("CARGO_BIN_EXE_conjure-app"))
        .current_dir(&root.0)
        .args(["--snap", png.to_str().unwrap(), "--size", "1100x760"])
        .status()
        .expect("the conjure-app binary ran");

    assert!(status.success(), "--snap exited with {status}");
    let bytes = std::fs::read(&png).expect("the snap wrote a PNG");
    assert!(
        bytes.len() > 10_000,
        "the PNG is only {} bytes",
        bytes.len()
    );
}
