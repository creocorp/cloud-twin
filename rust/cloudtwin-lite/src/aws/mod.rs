//! AWS provider namespace.
//!
//! The Python codebase uses provider folders, and this module gives the Rust
//! crate the same shape. Treat it like a feature-folder root that groups AWS
//! services behind the shared `aws::...` path.

pub mod bedrock;
pub mod dynamodb;
pub mod s3;
pub mod secretsmanager;
pub mod ses;
pub mod sns;
pub mod sqs;
