//! Integration tests for the `conjure-cli` binary.
//!
//! Each test spawns the real binary (built by cargo as
//! `CARGO_BIN_EXE_conjure-cli`) inside a private, nonce-named temp project
//! directory and drives the full milestone-2 pipeline end to end. Tests never
//! share a directory, never touch the process cwd, never use the network, and
//! stay deterministic through the fixed seed 42.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use conjure_core::{Asset, AssetRequest};
use conjure_export::Manifest;

const REQUEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/demo/request.json");

/// Project dir that removes itself (and everything under it) on drop.
/// Two tests must never share a directory.
struct TempProject(PathBuf);

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Unique per call: pid + nanos + label, so parallel runs can never collide.
fn nonce_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "conjure-cli-test-{}-{nanos}-{label}",
        std::process::id()
    ))
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_conjure-cli"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|err| panic!("failed to run conjure-cli {args:?}: {err}"))
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Creates the project dir, generates assets with seed 42, and captures the
/// generated JSON to `<dir>/assets.json`. Asserts the generation succeeded.
fn setup_project(label: &str) -> (PathBuf, TempProject) {
    let dir = nonce_dir(label);
    fs::create_dir(&dir)
        .unwrap_or_else(|err| panic!("failed to create project dir {}: {err}", dir.display()));
    let generated = run(&dir, &["generate", REQUEST, "42"]);
    assert_eq!(
        code(&generated),
        0,
        "generate failed: {}",
        stderr(&generated)
    );
    fs::write(dir.join("assets.json"), stdout(&generated))
        .unwrap_or_else(|err| panic!("failed to write assets.json: {err}"));
    (dir.clone(), TempProject(dir))
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn hex64<'a>(value: &'a str, what: &str) -> &'a str {
    assert!(
        is_hex64(value),
        "{what} must be 64 chars of [0-9a-f], got {value:?}"
    );
    value
}

fn parse_json<T: serde::de::DeserializeOwned>(text: &str, what: &str) -> T {
    serde_json::from_str(text).unwrap_or_else(|err| panic!("{what} is not valid json: {err}"))
}

/// Non-empty lines of the `assets:` section of `conjure ls` output.
fn asset_lines(text: &str) -> Vec<&str> {
    let (_, rest) = text
        .split_once("assets:")
        .unwrap_or_else(|| panic!("ls output has no assets section: {text}"));
    rest.lines()
        .filter(|line| !line.trim().is_empty())
        .collect()
}

fn read_assets(dir: &Path) -> Vec<Asset> {
    let text = fs::read_to_string(dir.join("assets.json"))
        .unwrap_or_else(|err| panic!("failed to read assets.json: {err}"));
    parse_json(&text, "assets.json")
}

fn save_output_hashes(dir: &Path) -> (String, Vec<String>) {
    let save = run(dir, &["save", REQUEST, "assets.json"]);
    assert_eq!(code(&save), 0, "save failed: {}", stderr(&save));
    let value: serde_json::Value = parse_json(&stdout(&save), "save output");
    let card_hash = value
        .get("card_hash")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    hex64(&card_hash, "card_hash");
    let asset_hashes: Vec<String> = value
        .get("asset_hashes")
        .and_then(serde_json::Value::as_array)
        .map(|hashes| {
            hashes
                .iter()
                .filter_map(|hash| hash.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(asset_hashes.len(), 3, "asset_hashes: {asset_hashes:?}");
    for hash in &asset_hashes {
        hex64(hash, "asset hash");
    }
    (card_hash, asset_hashes)
}

// U14.1: init → generate → save → ls → export, then decode the exported PNG
// and manifest and check every byte against the request and the assets.
#[test]
fn full_pipeline_generates_saves_lists_and_exports() {
    let (dir, _project) = setup_project("full-pipeline");

    let init = run(&dir, &["init"]);
    assert_eq!(code(&init), 0, "init failed: {}", stderr(&init));

    let (card_hash, asset_hashes) = save_output_hashes(&dir);

    let list = run(&dir, &["ls"]);
    assert_eq!(code(&list), 0, "ls failed: {}", stderr(&list));
    let list_out = stdout(&list);
    assert!(list_out.contains("demo"), "ls output: {list_out}");
    assert!(list_out.contains("cards:"), "ls output: {list_out}");
    assert!(list_out.contains("assets:"), "ls output: {list_out}");

    // Export all three assets: the U14.1 assertions below (24x8 sheet, 192
    // bytes, three frames) describe a full-strip export. The CLI accepts
    // multiple prefixes for exactly this.
    let prefixes: Vec<String> = asset_hashes.iter().map(|h| h[..16].to_string()).collect();
    let export = run(
        &dir,
        &[
            "export",
            &prefixes[0],
            &prefixes[1],
            &prefixes[2],
            "--out",
            "build",
        ],
    );
    assert_eq!(code(&export), 0, "export failed: {}", stderr(&export));
    let export_out = stdout(&export);
    let lines: Vec<&str> = export_out.lines().collect();
    assert_eq!(lines.len(), 2, "export stdout: {export_out}");
    assert!(
        lines[0].ends_with("/demo.png") || lines[0] == "demo.png",
        "png line: {}",
        lines[0]
    );
    assert!(
        lines[1].ends_with("/demo.json") || lines[1] == "demo.json",
        "json line: {}",
        lines[1]
    );
    assert!(dir.join("build").join("demo.png").exists());
    assert!(dir.join("build").join("demo.json").exists());

    let png_bytes = fs::read(dir.join("build").join("demo.png"))
        .unwrap_or_else(|err| panic!("failed to read exported png: {err}"));
    let decoded = conjure_export::decode_indexed_png(&png_bytes)
        .unwrap_or_else(|err| panic!("failed to decode exported png: {err:?}"));
    assert_eq!(decoded.width, 24);
    assert_eq!(decoded.height, 8);

    let assets = read_assets(&dir);
    assert_eq!(assets.len(), 3);
    // Strip layout, left to right: each sheet row is the three tiles' rows
    // concatenated in asset order (3 x 8x8 = 192 bytes).
    let mut expected_indices = Vec::with_capacity(24 * 8);
    for row in 0..8usize {
        for asset in &assets {
            assert_eq!((asset.width, asset.height), (8, 8));
            expected_indices.extend_from_slice(&asset.pixels[row * 8..row * 8 + 8]);
        }
    }
    assert_eq!(decoded.indices.len(), 192);
    assert_eq!(decoded.indices, expected_indices);

    let request_text = fs::read_to_string(REQUEST)
        .unwrap_or_else(|err| panic!("failed to read request fixture: {err}"));
    let request: AssetRequest = parse_json(&request_text, "request.json");
    let expected_palette: Vec<[u8; 3]> = request
        .style_card
        .palette
        .iter()
        .map(|color| color.rgb().expect("palette hex"))
        .collect();
    assert_eq!(decoded.palette, expected_palette);
    assert_eq!(decoded.trns, Some(vec![0]));

    let manifest_text = fs::read_to_string(dir.join("build").join("demo.json"))
        .unwrap_or_else(|err| panic!("failed to read exported manifest: {err}"));
    let manifest: Manifest = parse_json(&manifest_text, "demo.json");
    assert_eq!(manifest.manifest_version, 1);
    assert_eq!(manifest.style_card.hash, Some(card_hash));
    assert_eq!(manifest.frames.len(), 3);
    assert_eq!(manifest.frames[0].name, "demo#0");
    assert_eq!(manifest.image.layout, "strip_horizontal");
    assert_eq!(manifest.image.png_width, 24);
    assert_eq!(manifest.image.scale, 1);
}

// U14.2: a save whose asset count does not match the request exits 1 with a
// validation report on stdout and leaves the store empty.
#[test]
fn bad_save_exits_1_and_writes_nothing() {
    let (dir, _project) = setup_project("bad-save");

    let assets = read_assets(&dir);
    assert_eq!(assets.len(), 3);
    let truncated: Vec<Asset> = assets[..2].to_vec();
    let truncated_json = serde_json::to_string_pretty(&truncated)
        .unwrap_or_else(|err| panic!("failed to serialize truncated assets: {err}"));
    fs::write(dir.join("assets.json"), truncated_json)
        .unwrap_or_else(|err| panic!("failed to rewrite assets.json: {err}"));

    let save = run(&dir, &["save", REQUEST, "assets.json"]);
    assert_eq!(code(&save), 1, "save stderr: {}", stderr(&save));
    let report: serde_json::Value = parse_json(&stdout(&save), "save report");
    assert_eq!(
        report.get("passed").and_then(serde_json::Value::as_bool),
        Some(false),
        "save stdout: {}",
        stdout(&save)
    );

    let list = run(&dir, &["ls"]);
    assert_eq!(code(&list), 0, "ls failed: {}", stderr(&list));
    let list_out = stdout(&list);
    let empty = asset_lines(&list_out);
    assert!(
        empty.is_empty(),
        "ls assets section must be empty, got {empty:?}"
    );

    let blobs = fs::read_dir(dir.join(".conjure").join("blobs"))
        .unwrap_or_else(|err| panic!("failed to read blobs dir: {err}"));
    assert_eq!(blobs.count(), 0, "failed save must not write blobs");
}

// U14.3: save works without init (implicit .conjure creation), and ls in a
// directory with no store exits 2 with actionable guidance.
#[test]
fn implicit_store_created_and_missing_store_ls_fails() {
    let (dir_a, _project_a) = setup_project("implicit-store");
    let save = run(&dir_a, &["save", REQUEST, "assets.json"]);
    assert_eq!(code(&save), 0, "save failed: {}", stderr(&save));
    assert!(dir_a.join(".conjure").join("conjure.sqlite").exists());

    let dir_b = nonce_dir("missing-store");
    fs::create_dir(&dir_b)
        .unwrap_or_else(|err| panic!("failed to create project dir {}: {err}", dir_b.display()));
    let _project_b = TempProject(dir_b.clone());
    let list = run(&dir_b, &["ls"]);
    assert_eq!(code(&list), 2, "ls must fail without a store");
    assert!(
        stderr(&list).contains("no .conjure/ found (run conjure init)"),
        "ls stderr: {}",
        stderr(&list)
    );
}

// U14.4: tagging an asset makes it appear under `ls --tag`, and a tag with
// no matches yields an empty assets section.
#[test]
fn tag_filters_ls_output() {
    let (dir, _project) = setup_project("tag-filter");
    let (_, asset_hashes) = save_output_hashes(&dir);
    let prefix = &asset_hashes[0][..16];

    let tag = run(&dir, &["tag", prefix, "dungeon"]);
    assert_eq!(code(&tag), 0, "tag failed: {}", stderr(&tag));
    assert_eq!(stdout(&tag), "", "tag must be silent on success");

    let listed = run(&dir, &["ls", "--tag", "dungeon"]);
    assert_eq!(code(&listed), 0, "ls --tag failed: {}", stderr(&listed));
    assert!(
        stdout(&listed).contains("demo#0"),
        "ls --tag dungeon: {}",
        stdout(&listed)
    );

    let nomatch = run(&dir, &["ls", "--tag", "nomatch"]);
    assert_eq!(code(&nomatch), 0, "ls --tag failed: {}", stderr(&nomatch));
    let nomatch_out = stdout(&nomatch);
    let empty = asset_lines(&nomatch_out);
    assert!(
        empty.is_empty(),
        "ls --tag nomatch must show no assets, got {empty:?}"
    );
}

// U14.5: `show` resolves a card prefix to a style card and an asset prefix to
// an asset.
#[test]
fn show_resolves_cards_and_assets_by_prefix() {
    let (dir, _project) = setup_project("show");

    let init = run(&dir, &["init"]);
    assert_eq!(code(&init), 0, "init failed: {}", stderr(&init));

    let (card_hash, asset_hashes) = save_output_hashes(&dir);

    let card = run(&dir, &["show", &card_hash[..16]]);
    assert_eq!(code(&card), 0, "show card failed: {}", stderr(&card));
    let card_value: serde_json::Value = parse_json(&stdout(&card), "show card output");
    assert!(
        card_value.get("palette").is_some(),
        "show card output: {}",
        stdout(&card)
    );

    let asset = run(&dir, &["show", &asset_hashes[0][..16]]);
    assert_eq!(code(&asset), 0, "show asset failed: {}", stderr(&asset));
    let asset_value: serde_json::Value = parse_json(&stdout(&asset), "show asset output");
    assert!(
        asset_value.get("pixels").is_some(),
        "show asset output: {}",
        stdout(&asset)
    );
}

// Security: an asset name containing a path separator (`evil/name`) is fine
// as store metadata, but `export` must refuse it before creating the output
// directory or writing any file — a name like `../../evil` or `/abs/x` would
// otherwise write outside `--out`.
#[test]
fn export_refuses_unsafe_asset_names() {
    let dir = nonce_dir("unsafe-name");
    fs::create_dir(&dir)
        .unwrap_or_else(|err| panic!("failed to create project dir {}: {err}", dir.display()));
    let _project = TempProject(dir.clone());

    let request_text =
        fs::read_to_string(REQUEST).unwrap_or_else(|err| panic!("failed to read {REQUEST}: {err}"));
    let mut request: serde_json::Value = parse_json(&request_text, "request.json");
    request["name"] = serde_json::Value::String("evil/name".to_string());
    let request_json = serde_json::to_string_pretty(&request)
        .unwrap_or_else(|err| panic!("failed to serialize modified request: {err}"));
    fs::write(dir.join("request.json"), request_json)
        .unwrap_or_else(|err| panic!("failed to write request.json: {err}"));

    let generate = run(&dir, &["generate", "request.json", "42"]);
    assert_eq!(code(&generate), 0, "generate failed: {}", stderr(&generate));
    fs::write(dir.join("assets.json"), stdout(&generate))
        .unwrap_or_else(|err| panic!("failed to write assets.json: {err}"));

    // The store accepts the unsafe name: names are metadata, not paths.
    let save = run(&dir, &["save", "request.json", "assets.json"]);
    assert_eq!(code(&save), 0, "save failed: {}", stderr(&save));
    let save_value: serde_json::Value = parse_json(&stdout(&save), "save output");
    let first_hash = save_value
        .get("asset_hashes")
        .and_then(serde_json::Value::as_array)
        .and_then(|hashes| hashes.first())
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    hex64(&first_hash, "asset hash");
    let prefix = first_hash[..16].to_string();

    let export = run(&dir, &["export", &prefix, "--out", "build"]);
    assert_eq!(code(&export), 2, "export stderr: {}", stderr(&export));
    assert!(
        stderr(&export).contains("unsafe"),
        "export stderr: {}",
        stderr(&export)
    );
    assert!(
        !dir.join("build").exists(),
        "refused export must not create the build directory"
    );
}
