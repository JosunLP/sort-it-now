// src/main.rs
//! Sort-it-now: 3D Packing Optimization Service
//!
//! A high-performance Rust service for solving the bin-packing problem.
//! Efficiently places cuboids into containers considering:
//! - Weight limits and distribution
//! - Stability and center of gravity balance
//! - Layering (heavy objects at the bottom)

mod api;
mod config;
mod geometry;
mod model;
mod optimizer;
pub mod types;
mod update;

use config::AppConfig;

#[tokio::main]
async fn main() {
    if let Err(err) = dotenvy::dotenv() {
        if !matches!(err, dotenvy::Error::Io(ref io_err) if io_err.kind() == std::io::ErrorKind::NotFound)
        {
            eprintln!("⚠️ Could not load .env: {}", err);
        }
    }

    let app_config = AppConfig::from_env();
    let api_config = app_config.api.clone();
    let update_config = app_config.update.clone();
    let optimizer_config = app_config.optimizer.clone();

    println!("🚀 Packing Service starting...");
    let _update_task = update::check_for_updates_background(update_config);
    api::start_api_server(api_config, optimizer_config).await;
}
