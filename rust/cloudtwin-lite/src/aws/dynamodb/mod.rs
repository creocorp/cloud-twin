//! DynamoDB feature module.
//!
//! `mod.rs` is the public entry point for this service folder. It re-exports
//! the handler surface used by the top-level AWS dispatcher.

mod handlers;
mod models;
mod service;

pub use handlers::handle_json;
