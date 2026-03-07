//! SNS feature module.

mod handlers;
mod models;
mod service;

pub use handlers::{handle_query, QUERY_ACTIONS};
