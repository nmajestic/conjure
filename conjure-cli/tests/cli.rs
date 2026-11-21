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

/// Writes a two-colour request with the given kind to `<dir>/<file>`.
fn write_request(dir: &Path, file: &str, name: &str, kind: serde_json::Value) {
    let request = serde_json::json!({
        "name": name,
        "kind": kind,
        "style_card": {
            "id": "pixel-card",
            "name": "Pixel card",
            "palette": [
                { "name": "background", "hex": "#101010" },
                { "name": "stone", "hex": "#6d6d6d" }
            ],
            "grid": { "tile_size": { "w": 2, "h": 2 }, "unit_scale": 1 },
            "style": {
                "outline": "none",
                "outline_color": "#000000",
                "shading": "flat",
                "proportions": "chibi",
                "era": "16-bit",
                "theme": "dungeon"
            }
        }
    });
    let text = serde_json::to_string_pretty(&request)
        .unwrap_or_else(|err| panic!("failed to serialize request {file}: {err}"));
    fs::write(dir.join(file), text).unwrap_or_else(|err| panic!("failed to write {file}: {err}"));
}

fn pixel_asset(width: u32, height: u32, pixels: Vec<u8>) -> Asset {
    Asset {
        style_card_id: "pixel-card".to_string(),
        width,
        height,
        pixels,
    }
}

/// Saves `assets` under `request_file` and returns the asset hashes.
fn save_custom(dir: &Path, request_file: &str, assets: &[Asset]) -> Vec<String> {
    let assets_file = format!("{request_file}.assets.json");
    let text = serde_json::to_string_pretty(assets)
        .unwrap_or_else(|err| panic!("failed to serialize assets: {err}"));
    fs::write(dir.join(&assets_file), text)
        .unwrap_or_else(|err| panic!("failed to write {assets_file}: {err}"));
    let save = run(dir, &["save", request_file, &assets_file]);
    assert_eq!(code(&save), 0, "save failed: {}", stderr(&save));
    let value: serde_json::Value = parse_json(&stdout(&save), "save output");
    value
        .get("asset_hashes")
        .and_then(serde_json::Value::as_array)
        .map(|hashes| {
            hashes
                .iter()
                .filter_map(|hash| hash.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// `export`, the given prefixes, then the given flags.
fn export_args<'a>(prefixes: &'a [String], flags: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["export"];
    args.extend(prefixes.iter().map(String::as_str));
    args.extend_from_slice(flags);
    args
}

/// An initialized project with the demo tileset saved; returns the three
/// asset prefixes in asset order.
fn saved_demo(label: &str) -> (PathBuf, TempProject, Vec<String>) {
    let (dir, project) = setup_project(label);
    let init = run(&dir, &["init"]);
    assert_eq!(code(&init), 0, "init failed: {}", stderr(&init));
    let (_, asset_hashes) = save_output_hashes(&dir);
    let prefixes = asset_hashes.iter().map(|h| h[..16].to_string()).collect();
    (dir, project, prefixes)
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

// U3.1: four 2x2 tiles exported as a 2-column grid at scale 2 make an 8x8 sheet.
#[test]
fn export_grid_two_columns_scale_two() {
    let (dir, _project) = setup_project("grid-two-columns");
    write_request(
        &dir,
        "tiles.json",
        "tiles",
        serde_json::json!({ "type": "tileset", "tile_size": { "w": 2, "h": 2 }, "count": 4 }),
    );
    let tiles = vec![pixel_asset(2, 2, vec![0, 1, 1, 0]); 4];
    let hashes = save_custom(&dir, "tiles.json", &tiles);
    assert_eq!(hashes.len(), 4, "asset_hashes: {hashes:?}");
    let prefixes: Vec<String> = hashes.iter().map(|h| h[..16].to_string()).collect();

    let export = run(
        &dir,
        &export_args(
            &prefixes,
            &[
                "--layout",
                "grid",
                "--columns",
                "2",
                "--scale",
                "2",
                "--out",
                "build",
            ],
        ),
    );
    assert_eq!(code(&export), 0, "export failed: {}", stderr(&export));

    let png_bytes = fs::read(dir.join("build").join("tiles.png"))
        .unwrap_or_else(|err| panic!("failed to read exported png: {err}"));
    let decoded = conjure_export::decode_indexed_png(&png_bytes)
        .unwrap_or_else(|err| panic!("failed to decode exported png: {err:?}"));
    assert_eq!(decoded.width, 8);
    assert_eq!(decoded.height, 8);

    let manifest_text = fs::read_to_string(dir.join("build").join("tiles.json"))
        .unwrap_or_else(|err| panic!("failed to read exported manifest: {err}"));
    let manifest: Manifest = parse_json(&manifest_text, "tiles.json");
    assert_eq!(manifest.image.layout, "grid");
    assert_eq!(manifest.image.png_width, 8);
    assert_eq!(manifest.image.png_height, 8);
}

// U3.2: --layout grid without --columns is refused before any export runs.
#[test]
fn export_grid_without_columns_refused() {
    let (dir, _project, prefixes) = saved_demo("grid-no-columns");

    let export = run(
        &dir,
        &export_args(&prefixes, &["--layout", "grid", "--out", "build"]),
    );
    assert_eq!(code(&export), 2, "export stderr: {}", stderr(&export));
    assert!(
        stderr(&export).contains("grid layout requires --columns"),
        "export stderr: {}",
        stderr(&export)
    );
    assert!(!dir.join("build").exists());
}

// U3.3: --columns without --layout grid is refused.
#[test]
fn export_columns_without_grid_refused() {
    let (dir, _project, prefixes) = saved_demo("columns-no-grid");

    let export = run(
        &dir,
        &export_args(&prefixes, &["--columns", "2", "--out", "build"]),
    );
    assert_eq!(code(&export), 2, "export stderr: {}", stderr(&export));
    assert!(
        stderr(&export).contains("--columns only applies to grid layout"),
        "export stderr: {}",
        stderr(&export)
    );
    assert!(!dir.join("build").exists());
}

// U3.4: an unknown layout name is refused and names the bad value.
#[test]
fn export_unknown_layout_refused() {
    let (dir, _project, prefixes) = saved_demo("unknown-layout");

    let export = run(
        &dir,
        &export_args(&prefixes, &["--layout", "diag", "--out", "build"]),
    );
    assert_eq!(code(&export), 2, "export stderr: {}", stderr(&export));
    assert!(
        stderr(&export).contains("unknown layout diag"),
        "export stderr: {}",
        stderr(&export)
    );
    assert!(!dir.join("build").exists());
}

// U3.5: omitting the layout flags is the same as strip_horizontal at scale 1.
#[test]
fn export_default_matches_explicit_strip() {
    let (dir, _project, prefixes) = saved_demo("default-strip");

    let default = run(&dir, &export_args(&prefixes, &["--out", "default"]));
    assert_eq!(code(&default), 0, "default export: {}", stderr(&default));
    let explicit = run(
        &dir,
        &export_args(
            &prefixes,
            &[
                "--layout",
                "strip_horizontal",
                "--scale",
                "1",
                "--out",
                "explicit",
            ],
        ),
    );
    assert_eq!(code(&explicit), 0, "explicit export: {}", stderr(&explicit));

    let default_png = fs::read(dir.join("default").join("demo.png"))
        .unwrap_or_else(|err| panic!("failed to read default png: {err}"));
    let explicit_png = fs::read(dir.join("explicit").join("demo.png"))
        .unwrap_or_else(|err| panic!("failed to read explicit png: {err}"));
    assert!(
        default_png == explicit_png,
        "default and explicit strip PNGs differ"
    );
}

// U3.6: ls lists every name a shared-pixel asset is saved under, all with the
// same hash prefix.
#[test]
fn ls_lists_each_name_for_shared_pixels() {
    let dir = nonce_dir("ls-shared-pixels");
    fs::create_dir(&dir)
        .unwrap_or_else(|err| panic!("failed to create project dir {}: {err}", dir.display()));
    let _project = TempProject(dir.clone());
    let init = run(&dir, &["init"]);
    assert_eq!(code(&init), 0, "init failed: {}", stderr(&init));

    let pixels = vec![0, 1, 1, 0];
    write_request(
        &dir,
        "coin.json",
        "coin",
        serde_json::json!({ "type": "sprite", "size": { "w": 2, "h": 2 } }),
    );
    write_request(
        &dir,
        "floor.json",
        "floor",
        serde_json::json!({ "type": "tileset", "tile_size": { "w": 2, "h": 2 }, "count": 2 }),
    );
    let coin_hashes = save_custom(&dir, "coin.json", &[pixel_asset(2, 2, pixels.clone())]);
    assert_eq!(coin_hashes.len(), 1, "coin hashes: {coin_hashes:?}");
    let floor_hashes = save_custom(
        &dir,
        "floor.json",
        &[
            pixel_asset(2, 2, pixels.clone()),
            pixel_asset(2, 2, pixels.clone()),
        ],
    );
    assert_eq!(floor_hashes, vec![coin_hashes[0].clone(); 2]);

    let list = run(&dir, &["ls"]);
    assert_eq!(code(&list), 0, "ls failed: {}", stderr(&list));
    let list_out = stdout(&list);
    let lines = asset_lines(&list_out);
    let coin_line = lines
        .iter()
        .find(|line| line.contains("coin"))
        .unwrap_or_else(|| panic!("no coin line in ls: {list_out}"));
    let floor_line = lines
        .iter()
        .find(|line| line.contains("floor#0"))
        .unwrap_or_else(|| panic!("no floor#0 line in ls: {list_out}"));
    let floor1_line = lines
        .iter()
        .find(|line| line.contains("floor#1"))
        .unwrap_or_else(|| panic!("no floor#1 line in ls: {list_out}"));
    assert_eq!(lines.len(), 3, "ls assets section: {list_out}");

    let prefix = &coin_hashes[0][..16];
    let coin_hash = coin_line.split_whitespace().nth(3).unwrap_or_default();
    let floor_hash = floor_line.split_whitespace().nth(3).unwrap_or_default();
    let floor1_hash = floor1_line.split_whitespace().nth(3).unwrap_or_default();
    assert_eq!(coin_hash, prefix, "coin line: {coin_line}");
    assert_eq!(floor_hash, prefix, "floor#0 line: {floor_line}");
    assert_eq!(floor1_hash, prefix, "floor#1 line: {floor1_line}");
}

// U3.7: a horizontal strip of mixed heights is refused under default options.
#[test]
fn export_mixed_sizes_refused() {
    let (dir, _project) = setup_project("mixed-sizes");
    write_request(
        &dir,
        "small.json",
        "small",
        serde_json::json!({ "type": "sprite", "size": { "w": 2, "h": 2 } }),
    );
    write_request(
        &dir,
        "big.json",
        "big",
        serde_json::json!({ "type": "sprite", "size": { "w": 3, "h": 3 } }),
    );
    let small = save_custom(&dir, "small.json", &[pixel_asset(2, 2, vec![0, 1, 1, 0])]);
    let big = save_custom(&dir, "big.json", &[pixel_asset(3, 3, vec![0; 9])]);
    let prefixes = vec![small[0][..16].to_string(), big[0][..16].to_string()];

    let export = run(&dir, &export_args(&prefixes, &["--out", "build"]));
    assert_eq!(code(&export), 2, "export stderr: {}", stderr(&export));
    assert!(
        stderr(&export).contains("horizontal strip requires equal heights"),
        "export stderr: {}",
        stderr(&export)
    );
    assert!(!dir.join("build").exists());
}

fn workspace_cargo(args: &[&str]) -> Output {
    Command::new("cargo")
        .args(args)
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))
        .output()
        .unwrap_or_else(|err| panic!("failed to run cargo {args:?}: {err}"))
}

// U3.10: the whole workspace is clippy-clean with warnings denied.
#[test]
fn workspace_clippy_clean() {
    let output = workspace_cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ]);
    assert_eq!(code(&output), 0, "clippy stderr: {}", stderr(&output));
}

// U3.11: the whole workspace is rustfmt-clean.
#[test]
fn workspace_fmt_clean() {
    let output = workspace_cargo(&["fmt", "--all", "--", "--check"]);
    assert_eq!(code(&output), 0, "fmt stdout: {}", stdout(&output));
}

// U3.8: a prefix without --out keeps the original usage error.
#[test]
fn export_missing_out_usage_unchanged() {
    let (dir, _project, prefixes) = saved_demo("missing-out");

    let export = run(&dir, &export_args(&prefixes[..1], &[]));
    assert_eq!(code(&export), 2, "export stderr: {}", stderr(&export));
    assert!(
        stderr(&export).contains("export expects <hash-or-prefix>... --out <dir>"),
        "export stderr: {}",
        stderr(&export)
    );
}
