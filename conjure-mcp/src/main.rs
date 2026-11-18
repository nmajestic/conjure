//! Bin launcher for the Conjure MCP server. Parses `--root` and `--http`
//! flags by hand (no clap) and delegates entirely to the library's
//! `serve_stdio` / `serve_http`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process;

use conjure_mcp::{check_loopback, serve_http, serve_stdio};

const USAGE: &str = "usage: conjure-mcp [--root <path>] [--http 127.0.0.1:9711]";

fn usage_and_exit() -> ! {
    eprintln!("{USAGE}");
    process::exit(2);
}

#[tokio::main]
async fn main() {
    let mut root: Option<PathBuf> = None;
    let mut addr: Option<SocketAddr> = None;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--root" => {
                if root.is_some() {
                    usage_and_exit();
                }
                let Some(value) = args.get(index + 1) else {
                    usage_and_exit();
                };
                root = Some(PathBuf::from(value));
                index += 2;
            }
            "--http" => {
                if addr.is_some() {
                    usage_and_exit();
                }
                let Some(value) = args.get(index + 1) else {
                    usage_and_exit();
                };
                match value.parse::<SocketAddr>() {
                    Ok(parsed) => addr = Some(parsed),
                    Err(_) => usage_and_exit(),
                }
                index += 2;
            }
            _ => usage_and_exit(),
        }
    }

    let root = match root {
        Some(root) => root,
        None => match std::env::current_dir() {
            Ok(cwd) => cwd,
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(2);
            }
        },
    };

    let result = match addr {
        Some(addr) => match check_loopback(addr) {
            Ok(()) => serve_http(root, addr).await,
            Err(message) => {
                eprintln!("error: {message}");
                process::exit(2);
            }
        },
        None => serve_stdio(root).await,
    };

    match result {
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}
