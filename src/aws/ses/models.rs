//! SES runtime models, typed errors, and the verification state machine.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::config::ResolvedRule;

// ── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SesErrorKind {
    NotFound,
    AlreadyExists,
    BadRequest,
    MessageRejected,
    MailFromDomainNotVerified,
    SendingPaused,
    ConfigurationSetDoesNotExist,
    EventDestinationDoesNotExist,
    RuleSetDoesNotExist,
    RuleDoesNotExist,
    CannotDelete,
    InvalidS3Configuration,
    InvalidSnsTopic,
    Internal,
}

#[derive(Debug, Clone)]
pub struct SesError {
    pub kind: SesErrorKind,
    pub message: String,
}

pub type SesResult<T> = Result<T, SesError>;

impl SesError {
    pub fn new(kind: SesErrorKind, message: impl Into<String>) -> Self {
        SesError {
            kind,
            message: message.into(),
        }
    }

    /// Error code for the SES v2 REST/JSON API (`x-amzn-ErrorType`).
    pub fn v2_code(&self) -> &'static str {
        use SesErrorKind::*;
        match self.kind {
            NotFound
            | ConfigurationSetDoesNotExist
            | EventDestinationDoesNotExist
            | RuleSetDoesNotExist
            | RuleDoesNotExist => "NotFoundException",
            AlreadyExists => "AlreadyExistsException",
            MessageRejected => "MessageRejected",
            MailFromDomainNotVerified => "MailFromDomainNotVerifiedException",
            SendingPaused => "SendingPausedException",
            Internal => "InternalServiceErrorException",
            BadRequest | CannotDelete | InvalidS3Configuration | InvalidSnsTopic => {
                "BadRequestException"
            }
        }
    }

    /// Error code for the SES v1 Query API.
    pub fn v1_code(&self) -> &'static str {
        use SesErrorKind::*;
        match self.kind {
            NotFound => "NotFound",
            AlreadyExists => "AlreadyExists",
            BadRequest => "InvalidParameterValue",
            MessageRejected => "MessageRejected",
            MailFromDomainNotVerified => "MailFromDomainNotVerifiedException",
            SendingPaused => "AccountSendingPausedException",
            ConfigurationSetDoesNotExist => "ConfigurationSetDoesNotExist",
            EventDestinationDoesNotExist => "EventDestinationDoesNotExist",
            RuleSetDoesNotExist => "RuleSetDoesNotExist",
            RuleDoesNotExist => "RuleDoesNotExist",
            CannotDelete => "CannotDelete",
            InvalidS3Configuration => "InvalidS3Configuration",
            InvalidSnsTopic => "InvalidSnsTopic",
            Internal => "InternalFailure",
        }
    }

    pub fn v2_status(&self) -> u16 {
        use SesErrorKind::*;
        match self.kind {
            NotFound
            | ConfigurationSetDoesNotExist
            | EventDestinationDoesNotExist
            | RuleSetDoesNotExist
            | RuleDoesNotExist => 404,
            Internal => 500,
            _ => 400,
        }
    }

    pub fn v1_status(&self) -> u16 {
        match self.kind {
            SesErrorKind::Internal => 500,
            _ => 400,
        }
    }
}

impl From<anyhow::Error> for SesError {
    fn from(e: anyhow::Error) -> Self {
        SesError::new(SesErrorKind::Internal, e.to_string())
    }
}

pub fn not_found(msg: impl Into<String>) -> SesError {
    SesError::new(SesErrorKind::NotFound, msg)
}

pub fn bad_request(msg: impl Into<String>) -> SesError {
    SesError::new(SesErrorKind::BadRequest, msg)
}

// ── Verification state machine ──────────────────────────────────────────────

pub const PENDING: &str = "PENDING";
pub const SUCCESS: &str = "SUCCESS";

/// Verification progress for an identity or its custom MAIL FROM domain.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VerificationState {
    pub status: String,
    pub mode: String,
    pub outcome: String,
    pub checks_left: u32,
    pub verify_at: Option<String>,
}

fn normalize_outcome(s: &str) -> String {
    match s.to_ascii_uppercase().as_str() {
        "FAILED" | "FAILURE" => "FAILED".into(),
        "TEMPORARY_FAILURE" | "TEMPORARYFAILURE" => "TEMPORARY_FAILURE".into(),
        "PENDING" => PENDING.into(),
        "NOT_STARTED" => "NOT_STARTED".into(),
        _ => SUCCESS.into(),
    }
}

impl VerificationState {
    /// Initial state for a newly created identity under `rule`.
    pub fn start(rule: &ResolvedRule, now: DateTime<Utc>) -> Self {
        let outcome = normalize_outcome(&rule.outcome);
        let mut s = VerificationState {
            status: PENDING.into(),
            mode: rule.mode.clone(),
            outcome: outcome.clone(),
            checks_left: 0,
            verify_at: None,
        };
        match rule.mode.as_str() {
            "manual" | "pending" => {}
            "after_checks" | "checks" if rule.checks > 0 => s.checks_left = rule.checks,
            "delay" if rule.delay_seconds > 0 => {
                s.verify_at =
                    Some((now + Duration::seconds(rule.delay_seconds as i64)).to_rfc3339())
            }
            _ => s.status = outcome,
        }
        s
    }

    /// Already-verified state (seeds, legacy rows).
    pub fn resolved(status: &str) -> Self {
        VerificationState {
            status: normalize_outcome(status),
            mode: "auto".into(),
            outcome: normalize_outcome(status),
            ..Default::default()
        }
    }

    /// Advance a pending state. `count_check` consumes one read for
    /// `after_checks` mode. Returns whether the status changed.
    pub fn advance(&mut self, count_check: bool, now: DateTime<Utc>) -> bool {
        if self.status != PENDING {
            return false;
        }
        match self.mode.as_str() {
            "after_checks" | "checks" => {
                if count_check && self.checks_left > 0 {
                    self.checks_left -= 1;
                }
                if self.checks_left == 0 {
                    self.status = self.outcome.clone();
                    return true;
                }
            }
            "delay" => {
                let due = self
                    .verify_at
                    .as_deref()
                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| now >= t)
                    .unwrap_or(true);
                if due {
                    self.status = self.outcome.clone();
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    /// Force a status (control endpoint). `PENDING` re-enters manual mode.
    pub fn force(&mut self, status: &str) {
        let s = normalize_outcome(status);
        if s == PENDING {
            self.mode = "manual".into();
        }
        self.status = s;
        self.checks_left = 0;
        self.verify_at = None;
    }
}

/// SES v1 spelling of a verification status.
pub fn v1_status(status: &str) -> &'static str {
    match status {
        "SUCCESS" => "Success",
        "FAILED" => "Failed",
        "TEMPORARY_FAILURE" => "TemporaryFailure",
        "NOT_STARTED" => "NotStarted",
        _ => "Pending",
    }
}

// ── Identities ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SesIdentity {
    pub identity: String,
    pub kind: String, // "email" | "domain"
    pub status: String,
    pub verification: VerificationState,
    pub token: Option<String>,
    pub dkim_tokens: Vec<String>,
    pub dkim_signing_enabled: bool,
    pub dkim_origin: String,
    pub mail_from_domain: Option<String>,
    pub mail_from_behavior: String,
    pub mail_from: VerificationState,
    pub feedback_forwarding: bool,
    pub configuration_set: Option<String>,
    pub tags: Value,
    pub created_at: String,
    pub last_checked_at: Option<String>,
    pub last_success_at: Option<String>,
}

impl SesIdentity {
    pub fn is_domain(&self) -> bool {
        self.kind == "domain"
    }

    pub fn verified(&self) -> bool {
        self.status == SUCCESS
    }

    pub fn identity_type(&self) -> &'static str {
        if self.is_domain() {
            "DOMAIN"
        } else {
            "EMAIL_ADDRESS"
        }
    }

    /// DKIM status: domains follow their verification status; email
    /// identities have no DKIM records of their own.
    pub fn dkim_status(&self) -> &str {
        if self.is_domain() {
            &self.status
        } else {
            "NOT_STARTED"
        }
    }
}

/// Parse an RFC 3339 timestamp into epoch seconds (restJson timestamp format).
pub fn epoch(ts: &str) -> f64 {
    DateTime::parse_from_rfc3339(ts)
        .map(|t| t.timestamp_millis() as f64 / 1000.0)
        .unwrap_or(0.0)
}

/// SES event / notification timestamp format.
pub fn iso_ms(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(mode: &str, checks: u32, delay: u64, outcome: &str) -> ResolvedRule {
        ResolvedRule {
            mode: mode.into(),
            checks,
            delay_seconds: delay,
            outcome: outcome.into(),
        }
    }

    #[test]
    fn verification_modes() {
        let now = Utc::now();
        assert_eq!(
            VerificationState::start(&rule("auto", 0, 0, "SUCCESS"), now).status,
            "SUCCESS"
        );
        assert_eq!(
            VerificationState::start(&rule("auto", 0, 0, "failed"), now).status,
            "FAILED"
        );

        let mut manual = VerificationState::start(&rule("manual", 3, 0, "SUCCESS"), now);
        assert!(!manual.advance(true, now));
        assert_eq!(manual.status, "PENDING");
        manual.force("TEMPORARY_FAILURE");
        assert_eq!(manual.status, "TEMPORARY_FAILURE");

        let mut checks = VerificationState::start(&rule("after_checks", 2, 0, "SUCCESS"), now);
        assert!(!checks.advance(false, now));
        assert!(!checks.advance(true, now));
        assert!(checks.advance(true, now));
        assert_eq!(checks.status, "SUCCESS");

        let mut delay = VerificationState::start(&rule("delay", 0, 60, "FAILED"), now);
        assert!(!delay.advance(true, now));
        assert!(delay.advance(false, now + Duration::seconds(61)));
        assert_eq!(delay.status, "FAILED");
    }
}
