use std::env;
use std::fs;
use std::process::ExitCode;

use conjure_core::{Asset, AssetRequest, generate, repair, verify_request};

const USAGE: &str = "\
usage:
  conjure generate <request.json> <seed>
  conjure validate <request.json> <assets.json>
  conjure repair <request.json> <assets.json>
  conjure models
  conjure agent <request.json> --base-url <url> --model <id>
  conjure agent <request.json> --provider opencode --model <id>
  conjure init
  conjure save <request.json> <assets.json>
  conjure ls [--tag <tag>]
  conjure show <hash-or-prefix>
  conjure export <hash-or-prefix>... --out <dir>
  conjure tag <hash-or-prefix> <tag>
  conjure gc";

const AGENT_USAGE: &str = "\
usage: conjure agent <request.json> --base-url <url> --model <id>
       conjure agent <request.json> --provider opencode --model <id>";

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().collect();
    let args: Vec<String> = argv.iter().skip(1).cloned().collect();

    let Some(cmd) = args.first() else {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    };

    match cmd.as_str() {
        "generate" => run_generate(&args[1..]),
        "validate" => run_validate(&args[1..]),
        "repair" => run_repair(&args[1..]),
        "models" => run_models(&argv),
        "agent" => run_agent(&argv),
        "init" => run_init(&args[1..]),
        "save" => run_save(&args[1..]),
        "ls" => run_ls(&args[1..]),
        "show" => run_show(&args[1..]),
        "export" => run_export(&args[1..]),
        "tag" => run_tag(&args[1..]),
        "gc" => run_gc(&args[1..]),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown command {other:?} (try `conjure help`)");
            ExitCode::from(2)
        }
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("cannot parse {path}: {e}"))
}

fn err(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(2)
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<(), String> {
    let json =
        serde_json::to_string_pretty(value).map_err(|e| format!("cannot serialize output: {e}"))?;
    println!("{json}");
    Ok(())
}

fn find_or_create_store() -> Result<conjure_store::Store, String> {
    let cwd = env::current_dir().map_err(|e| e.to_string())?;
    match conjure_store::locate(&cwd) {
        Ok(root) => conjure_store::Store::open(&root).map_err(|e| e.to_string()),
        Err(conjure_store::StoreError::NotFound(_)) => {
            conjure_store::Store::create(&cwd).map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

fn open_store() -> Result<conjure_store::Store, String> {
    let cwd = env::current_dir().map_err(|e| e.to_string())?;
    let root = match conjure_store::locate(&cwd) {
        Ok(root) => root,
        Err(conjure_store::StoreError::NotFound(message)) => return Err(message),
        Err(e) => return Err(e.to_string()),
    };
    conjure_store::Store::open(&root).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct SaveOutput {
    card_hash: String,
    asset_hashes: Vec<String>,
}

fn run_generate(args: &[String]) -> ExitCode {
    if args.len() != 2 {
        return err("generate expects <request.json> <seed>");
    }
    let request = match read_json::<AssetRequest>(&args[0]) {
        Ok(r) => r,
        Err(e) => return err(&e),
    };
    let seed = match args[1].parse::<u64>() {
        Ok(s) => s,
        Err(_) => return err(&format!("invalid seed {}", args[1])),
    };
    let assets = generate(&request, seed);
    if let Err(e) = print_json(&assets) {
        return err(&e);
    }
    ExitCode::SUCCESS
}

fn run_validate(args: &[String]) -> ExitCode {
    if args.len() != 2 {
        return err("validate expects <request.json> <assets.json>");
    }
    let request = match read_json::<AssetRequest>(&args[0]) {
        Ok(r) => r,
        Err(e) => return err(&e),
    };
    let assets = match read_json::<Vec<Asset>>(&args[1]) {
        Ok(a) => a,
        Err(e) => return err(&e),
    };
    if let Err(e) = request.style_card.validate() {
        eprintln!("error: invalid style card: {e}");
        return ExitCode::from(2);
    }
    let report = verify_request(&request, &assets);
    if let Err(e) = print_json(&report) {
        return err(&e);
    }
    if report.passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn run_repair(args: &[String]) -> ExitCode {
    if args.len() != 2 {
        return err("repair expects <request.json> <assets.json>");
    }
    let request = match read_json::<AssetRequest>(&args[0]) {
        Ok(r) => r,
        Err(e) => return err(&e),
    };
    let assets = match read_json::<Vec<Asset>>(&args[1]) {
        Ok(a) => a,
        Err(e) => return err(&e),
    };
    let fixed = repair(&request, &assets);
    if let Err(e) = print_json(&fixed) {
        return err(&e);
    }
    ExitCode::SUCCESS
}

fn run_models(argv: &[String]) -> ExitCode {
    if argv.len() != 2 {
        eprintln!("usage: conjure models");
        return ExitCode::from(2);
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return err(&e.to_string()),
    };
    let urls = conjure_inference::local_urls();
    let servers = runtime.block_on(conjure_inference::discover(&urls));
    let mut local_found = 0;
    for server in &servers {
        local_found += server.models.len();
    }
    if local_found > 0 {
        println!("local servers:");
        for server in &servers {
            for model in &server.models {
                println!("{} {}", server.base_url, model);
            }
        }
    }

    let opencode_models = match runtime.block_on(conjure_inference::list_opencode_models(
        &conjure_inference::opencode_bin(),
    )) {
        Ok(models) => models,
        Err(e) => {
            eprintln!("opencode models unavailable: {e}");
            Vec::new()
        }
    };
    if !opencode_models.is_empty() {
        println!("opencode models:");
        for model in &opencode_models {
            println!("{model}");
        }
    }

    if local_found == 0 && opencode_models.is_empty() {
        eprintln!(
            "no local model servers and no opencode models found (tried the usual ports — run ollama/lmstudio/llama.cpp, or install opencode)"
        );
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

enum AgentSpec<'a> {
    Http { base_url: &'a str, model: &'a str },
    OpenCode { model: &'a str },
}

fn agent_spec(argv: &[String]) -> Option<AgentSpec<'_>> {
    if argv.len() != 7 || argv[5] != "--model" {
        return None;
    }
    match argv[3].as_str() {
        "--base-url" => Some(AgentSpec::Http {
            base_url: &argv[4],
            model: &argv[6],
        }),
        "--provider" if argv[4] == "opencode" => Some(AgentSpec::OpenCode { model: &argv[6] }),
        _ => None,
    }
}

impl<'a> AgentSpec<'a> {
    fn backend(self) -> conjure_inference::Backend {
        match self {
            AgentSpec::Http { base_url, model } => {
                conjure_inference::Backend::Http(conjure_inference::Client::new(base_url, model))
            }
            AgentSpec::OpenCode { model } => conjure_inference::Backend::OpenCode(
                conjure_inference::OpenCodeCli::new(conjure_inference::opencode_bin(), model),
            ),
        }
    }
}

fn run_agent(argv: &[String]) -> ExitCode {
    let Some(spec) = agent_spec(argv) else {
        eprintln!("{AGENT_USAGE}");
        return ExitCode::from(2);
    };
    let request = match read_json::<AssetRequest>(&argv[2]) {
        Ok(r) => r,
        Err(e) => return err(&e),
    };
    if let Err(e) = request.style_card.validate() {
        eprintln!("error: invalid style card: {e}");
        return ExitCode::from(2);
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return err(&e.to_string()),
    };
    let backend = spec.backend();
    let result = match runtime.block_on(conjure_agent::generate(
        &backend,
        &request,
        &conjure_agent::GenOpts::default(),
    )) {
        Ok(r) => r,
        Err(e) => return err(&e),
    };
    if let Err(e) = print_json(&result.assets) {
        return err(&e);
    }
    if result.used_fallback || result.repairs_used > 0 {
        eprintln!(
            "note: {} repair turns used, fallback engaged: {}",
            result.repairs_used, result.used_fallback
        );
    }
    ExitCode::SUCCESS
}

fn run_init(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        return err("init expects no arguments");
    }
    let cwd = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(e) => return err(&e.to_string()),
    };
    match conjure_store::Store::create(&cwd) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => err(&e.to_string()),
    }
}

fn run_save(args: &[String]) -> ExitCode {
    if args.len() != 2 {
        return err("save expects <request.json> <assets.json>");
    }
    let request = match read_json::<AssetRequest>(&args[0]) {
        Ok(r) => r,
        Err(e) => return err(&e),
    };
    let assets = match read_json::<Vec<Asset>>(&args[1]) {
        Ok(a) => a,
        Err(e) => return err(&e),
    };
    let store = match find_or_create_store() {
        Ok(s) => s,
        Err(e) => return err(&e),
    };
    let hashes = match store.save_assets(&request, &assets) {
        Ok(h) => h,
        Err(conjure_store::StoreError::Verify(conjure_store::Verification(report))) => {
            if let Err(e) = print_json(&report) {
                return err(&e);
            }
            return ExitCode::from(1);
        }
        Err(e) => return err(&e.to_string()),
    };
    let card_hash = match conjure_store::card_hash(&request.style_card) {
        Ok(h) => h,
        Err(e) => return err(&e.to_string()),
    };
    let output = SaveOutput {
        card_hash,
        asset_hashes: hashes,
    };
    if let Err(e) = print_json(&output) {
        return err(&e);
    }
    ExitCode::SUCCESS
}

fn run_ls(args: &[String]) -> ExitCode {
    let tag = match args.len() {
        0 => None,
        2 if args[0] == "--tag" => Some(args[1].clone()),
        _ => return err("ls expects [--tag <tag>]"),
    };
    let store = match open_store() {
        Ok(s) => s,
        Err(e) => return err(&e),
    };
    let cards = match store.list_cards() {
        Ok(c) => c,
        Err(e) => return err(&e.to_string()),
    };
    let assets = match store.list_assets(&conjure_store::ListFilter { tag }) {
        Ok(a) => a,
        Err(e) => return err(&e.to_string()),
    };
    println!("cards:");
    for card in &cards {
        println!("  card {} {}", card.name, &card.hash[..16]);
    }
    println!("assets:");
    for summary in &assets {
        if summary.tags.is_empty() {
            println!(
                "  {} {} {}x{} {}",
                summary.name,
                summary.kind,
                summary.width,
                summary.height,
                &summary.hash[..16]
            );
        } else {
            println!(
                "  {} {} {}x{} {} tags:{}",
                summary.name,
                summary.kind,
                summary.width,
                summary.height,
                &summary.hash[..16],
                summary.tags.join(",")
            );
        }
    }
    ExitCode::SUCCESS
}

fn run_show(args: &[String]) -> ExitCode {
    if args.len() != 1 {
        return err("show expects <hash-or-prefix>");
    }
    let store = match open_store() {
        Ok(s) => s,
        Err(e) => return err(&e),
    };
    match store.resolve_card(&args[0]) {
        Ok(hash) => {
            let card = match store.load_card(&hash) {
                Ok(c) => c,
                Err(e) => return err(&e.to_string()),
            };
            if let Err(e) = print_json(&card) {
                return err(&e);
            }
            ExitCode::SUCCESS
        }
        Err(conjure_store::StoreError::NotFound(_)) => match store.resolve_asset(&args[0]) {
            Ok(hash) => {
                let asset = match store.load_asset(&hash) {
                    Ok(a) => a,
                    Err(e) => return err(&e.to_string()),
                };
                if let Err(e) = print_json(&asset) {
                    return err(&e);
                }
                ExitCode::SUCCESS
            }
            Err(conjure_store::StoreError::NotFound(_)) => {
                err(&format!("no card or asset matches {}", args[0]))
            }
            Err(e) => err(&e.to_string()),
        },
        Err(e) => err(&e.to_string()),
    }
}

/// Derives the output stem from the manifest image file name, refusing any
/// name that could escape the export directory. Refuses (does not sanitize)
/// slashes, absolute paths, `..` segments, hidden names, and non-ASCII.
fn safe_output_stem(file: &str) -> Option<String> {
    let stem = file.strip_suffix(".png")?;
    if stem.is_empty() {
        return None;
    }
    if stem.starts_with('.') {
        return None;
    }
    if !stem
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return None;
    }
    if stem.split('.').any(|part| part.is_empty()) {
        return None; // blocks ".." segments
    }
    Some(stem.to_string())
}

fn run_export(args: &[String]) -> ExitCode {
    if args.len() < 3 || args[args.len() - 2] != "--out" {
        return err("export expects <hash-or-prefix>... --out <dir>");
    }
    let prefixes = &args[..args.len() - 2];
    let out_dir = std::path::PathBuf::from(&args[args.len() - 1]);
    let store = match open_store() {
        Ok(s) => s,
        Err(e) => return err(&e),
    };
    let mut summaries = Vec::with_capacity(prefixes.len());
    for prefix in prefixes {
        let hash = match store.resolve_asset(prefix) {
            Ok(h) => h,
            Err(e) => return err(&e.to_string()),
        };
        let summary = match store.asset_summary(&hash) {
            Ok(s) => s,
            Err(e) => return err(&e.to_string()),
        };
        summaries.push(summary);
    }
    let style_card_hash = summaries[0].style_card_hash.clone();
    if summaries
        .iter()
        .any(|s| s.style_card_hash != style_card_hash)
    {
        return err("export requires assets from one style card");
    }
    let card = match store.load_card(&style_card_hash) {
        Ok(c) => c,
        Err(e) => return err(&e.to_string()),
    };
    let mut assets = Vec::with_capacity(summaries.len());
    let mut names = Vec::with_capacity(summaries.len());
    for summary in &summaries {
        let asset = match store.load_asset(&summary.hash) {
            Ok(a) => a,
            Err(e) => return err(&e.to_string()),
        };
        assets.push(asset);
        names.push(summary.name.clone());
    }
    let opts = conjure_export::ExportOpts::default();
    let png = match conjure_export::export_png(&card, &assets, &opts) {
        Ok(p) => p,
        Err(e) => return err(&e.to_string()),
    };
    let mut manifest = match conjure_export::export_manifest(&card, &assets, &names, &opts) {
        Ok(m) => m,
        Err(e) => return err(&e.to_string()),
    };
    manifest.style_card.hash = Some(style_card_hash.clone());
    let Some(stem) = safe_output_stem(&manifest.image.file) else {
        return err(&format!(
            "export output name {:?} is unsafe",
            manifest.image.file
        ));
    };
    let png_path = out_dir.join(&manifest.image.file);
    let json_path = out_dir.join(format!("{stem}.json"));
    if let Err(e) = fs::create_dir_all(&out_dir) {
        return err(&e.to_string());
    }
    if let Err(e) = fs::write(&png_path, &png) {
        return err(&e.to_string());
    }
    let json = match serde_json::to_string_pretty(&manifest) {
        Ok(j) => j,
        Err(e) => return err(&format!("cannot serialize manifest: {e}")),
    };
    if let Err(e) = fs::write(&json_path, json) {
        return err(&e.to_string());
    }
    println!("{}", png_path.display());
    println!("{}", json_path.display());
    ExitCode::SUCCESS
}

fn run_tag(args: &[String]) -> ExitCode {
    if args.len() != 2 {
        return err("tag expects <hash-or-prefix> <tag>");
    }
    let store = match open_store() {
        Ok(s) => s,
        Err(e) => return err(&e),
    };
    let hash = match store.resolve_asset(&args[0]) {
        Ok(h) => h,
        Err(e) => return err(&e.to_string()),
    };
    match store.tag(&hash, &args[1]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => err(&e.to_string()),
    }
}

fn run_gc(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        return err("gc expects no arguments");
    }
    let store = match open_store() {
        Ok(s) => s,
        Err(e) => return err(&e),
    };
    let report = match store.gc() {
        Ok(r) => r,
        Err(e) => return err(&e.to_string()),
    };
    println!(
        "removed {} orphan blob(s), kept {}",
        report.removed, report.kept
    );
    ExitCode::SUCCESS
}
