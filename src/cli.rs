//! Command-line interface for offline (non-HTTP) usage.
//!
//! The binary keeps its primary role as an HTTP server, but a small set of subcommands make it
//! usable directly in shell pipelines:
//!
//! ```text
//! sort_it_now pack request.json     # read a PackRequest from a file
//! sort_it_now pack -                 # read a PackRequest from stdin
//! cat request.json | sort_it_now pack
//! sort_it_now --help
//! sort_it_now --version
//! ```
//!
//! The `pack` subcommand shares the exact same validation and packing logic as the `/pack`
//! HTTP endpoint via [`crate::api::run_pack`], keeping behaviour identical across transports.

use std::io::Read;

use crate::api::{PackRequest, run_pack};
use crate::config::RequestLimits;
use crate::optimizer::PackingConfig;

/// Outcome of CLI argument handling.
#[derive(Debug)]
pub enum CliOutcome {
    /// A subcommand handled the invocation; the process should exit successfully.
    Handled,
    /// A subcommand failed; the contained message should be reported and a non-zero code returned.
    Failed(String),
    /// No (recognized) subcommand was given; the caller should start the HTTP server.
    StartServer,
}

const HELP_TEXT: &str = "\
sort_it_now — 3D bin-packing optimization service

USAGE:
    sort_it_now [SUBCOMMAND]

Without a subcommand the HTTP server starts (configurable via environment variables).

SUBCOMMANDS:
    pack [FILE]      Optimize a JSON PackRequest and print the JSON PackResponse.
                     FILE defaults to '-' (read from stdin).

OPTIONS:
    -h, --help       Print this help and exit.
    -V, --version    Print version information and exit.
";

/// Dispatches CLI arguments (already stripped of the program name).
pub fn run<I>(args: I) -> CliOutcome
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();
    let Some(command) = args.first() else {
        return CliOutcome::StartServer;
    };

    match command.as_str() {
        "-h" | "--help" | "help" => {
            print!("{HELP_TEXT}");
            CliOutcome::Handled
        }
        "-V" | "--version" => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            CliOutcome::Handled
        }
        "pack" => run_pack_command(args.get(1).map(String::as_str)),
        other => CliOutcome::Failed(format!(
            "Unknown subcommand '{other}'. Run 'sort_it_now --help' for usage."
        )),
    }
}

/// Executes the `pack` subcommand, reading from `source` (a path, or `-`/`None` for stdin).
fn run_pack_command(source: Option<&str>) -> CliOutcome {
    let raw = match read_source(source) {
        Ok(raw) => raw,
        Err(err) => return CliOutcome::Failed(err),
    };

    match pack_json(&raw) {
        Ok(json) => {
            println!("{json}");
            CliOutcome::Handled
        }
        Err(err) => CliOutcome::Failed(err),
    }
}

/// Reads the request payload from a file path, or from stdin when `source` is `None` or `"-"`.
fn read_source(source: Option<&str>) -> Result<String, String> {
    match source {
        None | Some("-") => {
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|err| format!("Could not read request from stdin: {err}"))?;
            Ok(buffer)
        }
        Some(path) => std::fs::read_to_string(path)
            .map_err(|err| format!("Could not read request file '{path}': {err}")),
    }
}

/// Parses, validates, and packs a JSON request, returning the pretty-printed JSON response.
///
/// Uses the default environment-derived configuration and request limits so the CLI mirrors the
/// running server's behaviour.
fn pack_json(raw: &str) -> Result<String, String> {
    let request: PackRequest =
        serde_json::from_str(raw).map_err(|err| format!("Invalid JSON request: {err}"))?;

    let response = run_pack(request, PackingConfig::default(), RequestLimits::default())
        .map_err(|err| err.to_string())?;

    serde_json::to_string_pretty(&response)
        .map_err(|err| format!("Could not serialize response: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> String {
        r#"{
            "containers": [{"name": "Box", "dims": [10, 10, 10], "max_weight": 100}],
            "objects": [{"id": 1, "dims": [10, 10, 5], "weight": 40}]
        }"#
        .to_string()
    }

    #[test]
    fn pack_json_returns_response_for_valid_request() {
        let json = pack_json(&sample_request()).expect("valid request should pack");
        let value: serde_json::Value = serde_json::from_str(&json).expect("output is JSON");
        assert_eq!(value["results"].as_array().map(Vec::len), Some(1));
        assert_eq!(value["is_complete"], true);
    }

    #[test]
    fn pack_json_rejects_invalid_json() {
        let err = pack_json("not json").expect_err("invalid JSON should error");
        assert!(
            err.contains("Invalid JSON request"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn pack_json_rejects_request_without_containers() {
        let raw = r#"{"containers": [], "objects": []}"#;
        let err = pack_json(raw).expect_err("missing containers should error");
        assert!(
            err.contains("At least one packaging type"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn run_without_args_starts_server() {
        assert!(matches!(run(Vec::<String>::new()), CliOutcome::StartServer));
    }

    #[test]
    fn run_help_is_handled() {
        assert!(matches!(
            run(vec!["--help".to_string()]),
            CliOutcome::Handled
        ));
    }

    #[test]
    fn run_unknown_subcommand_fails() {
        assert!(matches!(
            run(vec!["frobnicate".to_string()]),
            CliOutcome::Failed(_)
        ));
    }
}
