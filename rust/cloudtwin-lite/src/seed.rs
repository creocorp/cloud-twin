//! Seed documents: provision resources at startup or via `POST /_cloudtwin/seed`.
//!
//! ```yaml
//! seed:
//!   kms:     { keys: [...] }
//!   s3:      { buckets: [...] }
//!   sqs:     { queues: [...] }
//!   sns:     { topics: [...] }
//!   ses:     { identities: [...] }
//!   cognito: { user_pools: [...] }
//! ```
//!
//! Sections are applied in dependency order (queues before the topics that
//! subscribe them, buckets before SES receipt rules that write to them).

use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::{aws, AppState};

pub async fn apply(state: &Arc<AppState>, doc: &Value) -> Result<()> {
    if !doc.is_object() {
        anyhow::bail!("seed document must be an object");
    }
    if let Some(v) = doc.get("kms") {
        aws::kms::seed(state, v).await.context("kms seed")?;
    }
    if let Some(v) = doc.get("s3") {
        aws::s3::seed(state, v).await.context("s3 seed")?;
    }
    if let Some(v) = doc.get("sqs") {
        aws::sqs::seed(state, v).await.context("sqs seed")?;
    }
    if let Some(v) = doc.get("sns") {
        aws::sns::seed(state, v).await.context("sns seed")?;
    }
    if let Some(v) = doc.get("ses") {
        aws::ses::seed(state, v).await.context("ses seed")?;
    }
    if let Some(v) = doc.get("cognito") {
        aws::cognito::seed(state, v).await.context("cognito seed")?;
    }
    Ok(())
}
