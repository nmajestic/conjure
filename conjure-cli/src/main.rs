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
  conjure agent <request.json> --base-url <url> --model <id>";

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
    let mut found = 0;
    for server in &servers {
        for model in &server.models {
            println!("{} {}", server.base_url, model);
            found += 1;
        }
    }
    if found == 0 {
        eprintln!(
            "no local model servers found (tried the usual ports — run ollama/lmstudio/llama.cpp first)"
        );
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

fn run_agent(argv: &[String]) -> ExitCode {
    if argv.len() != 7 || argv[3] != "--base-url" || argv[5] != "--model" {
        eprintln!("usage: conjure agent <request.json> --base-url <url> --model <id>");
        return ExitCode::from(2);
    }
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
    let client = conjure_inference::Client::new(argv[4].as_str(), argv[6].as_str());
    let result = match runtime.block_on(conjure_agent::generate(
        &client,
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
