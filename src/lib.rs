//! Sort-it-now: 3D Packing Optimization Service (library crate).
//!
//! This crate exposes the reusable building blocks of the packing service so they can be
//! embedded in other applications, driven from the command line, and covered by integration
//! tests. The accompanying binary (`src/main.rs`) is a thin wrapper that wires these modules
//! into an HTTP server.
//!
//! ## Modules
//!
//! - [`types`] — geometry primitives (`Vec3`, `BoundingBox`) and shared traits.
//! - [`model`] — domain models (`Box3D`, `PlacedBox`, `Container`, `ContainerBlueprint`).
//! - [`geometry`] — stateless collision and support helpers.
//! - [`optimizer`] — the heuristic packing engine and its diagnostics.
//! - [`config`] — environment-driven configuration for the API, optimizer, and updater.
//! - [`api`] — the Axum HTTP layer (router, request/response types, handlers).
//! - [`update`] — the background GitHub release updater.

pub mod api;
pub mod cli;
pub mod config;
pub mod geometry;
pub mod model;
pub mod optimizer;
pub mod types;
pub mod update;
