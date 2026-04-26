//! Fire-and-forget telemetry sink.
//!
//! Every mutating handler calls `emit(db, provider, service, action, payload)`
//! after a successful operation. Errors are silently swallowed so that telemetry
//! failures never affect the main request path.

use chrono::Utc;

use crate::db::Database;

/// Write one telemetry event to the `events` table.
///
/// Designed to be called with `tokio::spawn` or directly with `.await` –
/// either way, any error is logged at WARN and discarded.
pub async fn emit(db: &Database, provider: &str, service: &str, action: &str, payload: &str) {
    let p = provider.to_string();
    let s = service.to_string();
    let a = action.to_string();
    let pl = payload.to_string();
    let now = Utc::now().to_rfc3339();
    if let Err(e) = db
        .conn
        .call(move |conn| {
            conn.execute(
                "INSERT INTO events (provider, service, action, payload, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![p, s, a, pl, now],
            )?;
            Ok(())
        })
        .await
    {
        tracing::warn!("telemetry emit failed: {e}");
    }
}
