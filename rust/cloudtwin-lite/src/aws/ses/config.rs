//! SES runtime configuration (`providers.aws.ses` in `cloudtwin.yml`).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::config::{parse_bool, EnvFn};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SesConfig {
    /// Reject sends from unverified identities with `MessageRejected`, like SES.
    pub strict_verification: bool,
    /// How new identities move from `PENDING` to a final verification status.
    pub verification: VerificationConfig,
    /// What `GetAccount` reports and enforces.
    pub account: AccountConfig,
}

impl Default for SesConfig {
    fn default() -> Self {
        SesConfig {
            strict_verification: true,
            verification: VerificationConfig::default(),
            account: AccountConfig::default(),
        }
    }
}

/// Verification policy for new identities and custom MAIL FROM domains.
///
/// * `auto` — resolve to `outcome` immediately (default; outcome `SUCCESS`).
/// * `manual` — stay `PENDING` until `POST /_cloudtwin/ses/identities/{id}/verification`.
/// * `after_checks` — resolve after `checks` reads of the identity.
/// * `delay` — resolve `delay_seconds` after creation.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct VerificationConfig {
    pub mode: String,
    pub checks: u32,
    pub delay_seconds: u64,
    /// Final status: `SUCCESS`, `FAILED`, or `TEMPORARY_FAILURE`.
    pub outcome: String,
    /// Per-identity rules keyed by exact identity or a leading-`*` glob
    /// (`*.example.com`, `*@example.com`). Unset fields inherit the defaults.
    pub overrides: BTreeMap<String, VerificationRule>,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        VerificationConfig {
            mode: "auto".into(),
            checks: 3,
            delay_seconds: 30,
            outcome: "SUCCESS".into(),
            overrides: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct VerificationRule {
    pub mode: Option<String>,
    pub checks: Option<u32>,
    pub delay_seconds: Option<u64>,
    pub outcome: Option<String>,
}

/// Effective verification rule for one identity.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRule {
    pub mode: String,
    pub checks: u32,
    pub delay_seconds: u64,
    pub outcome: String,
}

impl VerificationConfig {
    /// Resolve the rule for `identity`, preferring exact overrides, then the
    /// longest matching glob.
    pub fn rule_for(&self, identity: &str) -> ResolvedRule {
        let ident = identity.to_ascii_lowercase();
        let matched = self.overrides.get(identity).or_else(|| {
            self.overrides
                .iter()
                .filter(|(pat, _)| glob_match(&pat.to_ascii_lowercase(), &ident))
                .max_by_key(|(pat, _)| pat.len())
                .map(|(_, r)| r)
        });
        let r = matched.cloned().unwrap_or_default();
        ResolvedRule {
            mode: r.mode.unwrap_or_else(|| self.mode.clone()).to_ascii_lowercase(),
            checks: r.checks.unwrap_or(self.checks),
            delay_seconds: r.delay_seconds.unwrap_or(self.delay_seconds),
            outcome: r
                .outcome
                .unwrap_or_else(|| self.outcome.clone())
                .to_ascii_uppercase(),
        }
    }
}

fn glob_match(pattern: &str, value: &str) -> bool {
    match pattern.strip_prefix('*') {
        Some(suffix) => value.ends_with(suffix) || value == suffix.trim_start_matches(['.', '@']),
        None => pattern == value,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AccountConfig {
    /// `false` = SES sandbox: recipients must also be verified identities.
    pub production_access_enabled: bool,
    pub sending_enabled: bool,
    pub max_24_hour_send: f64,
    pub max_send_rate: f64,
    pub enforcement_status: String,
    /// Reasons the account-level suppression list auto-adds addresses for.
    pub suppressed_reasons: Vec<String>,
}

impl Default for AccountConfig {
    fn default() -> Self {
        AccountConfig {
            production_access_enabled: true,
            sending_enabled: true,
            max_24_hour_send: 50000.0,
            max_send_rate: 14.0,
            enforcement_status: "HEALTHY".into(),
            suppressed_reasons: vec!["BOUNCE".into(), "COMPLAINT".into()],
        }
    }
}

impl SesConfig {
    pub fn apply_env(&mut self, env: EnvFn) {
        if let Some(v) = env("CLOUDTWIN_SES_STRICT_VERIFICATION").and_then(|s| parse_bool(&s)) {
            self.strict_verification = v;
        }
        if let Some(v) = env("CLOUDTWIN_SES_VERIFICATION_MODE") {
            self.verification.mode = v;
        }
        if let Some(v) = env("CLOUDTWIN_SES_VERIFICATION_CHECKS").and_then(|s| s.parse().ok()) {
            self.verification.checks = v;
        }
        if let Some(v) =
            env("CLOUDTWIN_SES_VERIFICATION_DELAY_SECONDS").and_then(|s| s.parse().ok())
        {
            self.verification.delay_seconds = v;
        }
        if let Some(v) = env("CLOUDTWIN_SES_VERIFICATION_OUTCOME") {
            self.verification.outcome = v;
        }
        if let Some(v) = env("CLOUDTWIN_SES_PRODUCTION_ACCESS").and_then(|s| parse_bool(&s)) {
            self.account.production_access_enabled = v;
        }
        if let Some(v) = env("CLOUDTWIN_SES_SENDING_ENABLED").and_then(|s| parse_bool(&s)) {
            self.account.sending_enabled = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_prefer_exact_then_longest_glob() {
        let mut cfg = VerificationConfig::default();
        cfg.overrides.insert(
            "*.fail.test".into(),
            VerificationRule { outcome: Some("FAILED".into()), ..Default::default() },
        );
        cfg.overrides.insert(
            "ok.fail.test".into(),
            VerificationRule { mode: Some("manual".into()), ..Default::default() },
        );
        assert_eq!(cfg.rule_for("x.fail.test").outcome, "FAILED");
        assert_eq!(cfg.rule_for("fail.test").outcome, "FAILED");
        let exact = cfg.rule_for("ok.fail.test");
        assert_eq!((exact.mode.as_str(), exact.outcome.as_str()), ("manual", "SUCCESS"));
        assert_eq!(cfg.rule_for("other.test").mode, "auto");
    }
}
