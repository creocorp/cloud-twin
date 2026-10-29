//! S3 runtime configuration (`providers.aws.s3` in `cloudtwin.yml`).

use serde::Deserialize;

use crate::config::EnvFn;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct S3Config {
    /// Host suffix for virtual-hosted-style addressing: requests to
    /// `{bucket}.{suffix}[:port]` are rewritten to `/{bucket}/...`.
    /// Empty disables the rewrite.
    pub virtual_host_suffix: String,
}

impl Default for S3Config {
    fn default() -> Self {
        S3Config {
            virtual_host_suffix: "s3.localhost".into(),
        }
    }
}

impl S3Config {
    pub fn apply_env(&mut self, env: EnvFn) {
        if let Some(v) = env("CLOUDTWIN_S3_VIRTUAL_HOST_SUFFIX") {
            self.virtual_host_suffix = v;
        }
    }
}
