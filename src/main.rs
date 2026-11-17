// src/main.rs
//! Sort-it-now: 3D Packing Optimization Service (binary entrypoint).
//!
//! A high-performance Rust service for solving the bin-packing problem.
//! Efficiently places cuboids into containers considering:
//! - Weight limits and distribution
//! - Stability and center of gravity balance
//! - Layering (heavy objects at the bottom)
//!
//! Without arguments the binary starts the HTTP server. The `pack` subcommand runs a one-shot
//! optimization from a JSON request and prints the JSON response, which is convenient for
//! pipelines and scripting without a running server.

use sort_it_now::api;
use sort_it_now::cli::{self, CliOutcome};
use sort_it_now::config::AppConfig;
use sort_it_now::license::{FREE_PACK_LIMIT, LICENSE_KEY_VAR, LicenseGuard, LicenseStatus};
use sort_it_now::update;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    if let Err(err) = dotenvy::dotenv()
        && !matches!(err, dotenvy::Error::Io(ref io_err) if io_err.kind() == std::io::ErrorKind::NotFound)
    {
        eprintln!("⚠️ Could not load .env: {}", err);
    }

    // The CLI handles offline subcommands (e.g. `pack`, `--help`) and tells us whether the
    // server should still be started afterwards.
    match cli::run(std::env::args().skip(1)) {
        CliOutcome::Handled => return,
        CliOutcome::Failed(message) => {
            eprintln!("❌ {message}");
            std::process::exit(1);
        }
        CliOutcome::StartServer => {}
    }

    let app_config = AppConfig::from_env();
    let api_config = app_config.api.clone();
    let update_config = app_config.update.clone();
    let optimizer_config = app_config.optimizer.clone();

    println!("🚀 Packing Service starting...");
    let license = Arc::new(LicenseGuard::from_env());
    match license.status() {
        LicenseStatus::Licensed => println!("🔑 License: valid license key detected."),
        LicenseStatus::Unlicensed(reason) => eprintln!(
            "⚠️ License: unlicensed instance ({reason}). Packing is limited to {FREE_PACK_LIMIT} \
             operations; set {LICENSE_KEY_VAR} to a valid license key for unlimited use."
        ),
    }
    let _update_task = update::check_for_updates_background(update_config);
    api::start_api_server(api_config, optimizer_config, license).await;
}
