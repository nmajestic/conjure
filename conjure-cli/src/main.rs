use std::env;
use std::fs;
use std::process::ExitCode;

use conjure_core::{Asset, AssetRequest, generate, repair, verify_request};

const USAGE: &str = "\
usage:
  conjure generate <request.json> <seed>
  conjure validate <request.json> <assets.json>
  conjure repair <request.json> <assets.json>";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    let Some(cmd) = args.first() else {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    };

    match cmd.as_str() {
        "generate" => run_generate(&args[1..]),
        "validate" => run_validate(&args[1..]),
        "repair" => run_repair(&args[1..]),
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
