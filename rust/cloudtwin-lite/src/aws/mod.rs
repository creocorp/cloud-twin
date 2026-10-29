//! AWS provider namespace.
//!
//! The Python codebase uses provider folders, and this module gives the Rust
//! crate the same shape. Treat it like a feature-folder root that groups AWS
//! services behind the shared `aws::...` path.

/// Account id used in every ARN and queue URL.
pub const ACCOUNT_ID: &str = "000000000000";
/// Region used in every ARN.
pub const REGION: &str = "us-east-1";

pub mod bedrock;
pub mod cognito;
pub mod dynamodb;
pub mod kms;
pub mod s3;
pub mod secretsmanager;
pub mod ses;
pub mod sns;
pub mod sqs;
