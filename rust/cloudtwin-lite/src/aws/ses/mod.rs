//! SES feature module.
//!
//! SES is split across two protocol surfaces: the older AWS Query API and the
//! newer SES v2 REST/JSON API. This module re-exports both entry points.

mod handlers;
mod models;
mod service;

pub use handlers::{handle_query, router_v2, QUERY_ACTIONS};
