//! SQS feature module.

mod handlers;
mod models;
mod service;

pub use handlers::{handle_json, JSON_TARGETS};
pub use service::SqsService;
