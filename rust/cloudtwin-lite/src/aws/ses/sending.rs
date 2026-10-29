//! Outbound sending pipeline shared by SES v1 `SendEmail`/`SendRawEmail` and
//! SES v2 `SendEmail`.
//!
//! Order of checks mirrors SES: account sending status, configuration set,
//! sender identity, sandbox recipients, MAIL FROM, then per-recipient outcomes
//! (mailbox simulator, account suppression list) and event publishing.

use std::collections::BTreeMap;

use chrono::Utc;
use mail_parser::{Address, MessageParser};
use serde_json::{json, Value};

use super::events::{self, MailContext};
use super::models::*;
use super::service::{address_domain, bare_address, SesService};

/// Hard limit on a message, including attachments (SES v2).
pub const MAX_MESSAGE_BYTES: usize = 40 * 1024 * 1024;
const MAX_RECIPIENTS: usize = 50;
const SIMULATOR_DOMAIN: &str = "simulator.amazonses.com";

pub enum SendContent {
    Simple {
        subject: String,
        text: Option<String>,
        html: Option<String>,
        headers: Vec<(String, String)>,
    },
    Raw(Vec<u8>),
}

pub struct SendRequest {
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub reply_to: Vec<String>,
    pub content: SendContent,
    pub tags: Vec<(String, String)>,
    pub configuration_set: Option<String>,
    pub feedback_forwarding_address: Option<String>,
    pub return_path: Option<String>,
}

/// What happened to one recipient.
#[derive(Debug, Clone, PartialEq)]
enum Outcome {
    Delivered,
    OutOfOffice,
    Complaint,
    Bounce(&'static str, &'static str), // (bounceSubType, diagnostic)
    Delayed,
}

fn simulator_outcome(rcpt: &str) -> Option<(Outcome, bool)> {
    let addr = bare_address(rcpt).to_ascii_lowercase();
    let (local, domain) = addr.rsplit_once('@')?;
    if domain != SIMULATOR_DOMAIN {
        return None;
    }
    let local = local.split('+').next().unwrap_or(local);
    // (outcome, rejects the whole message)
    Some(match local {
        "bounce" => (
            Outcome::Bounce("General", "smtp; 550 5.1.1 user unknown"),
            false,
        ),
        "suppressionlist" => (
            Outcome::Bounce(
                "Suppressed",
                "Amazon SES has suppressed sending to this address because it has a recent history of bouncing as an invalid address.",
            ),
            false,
        ),
        "complaint" => (Outcome::Complaint, false),
        "ooto" => (Outcome::OutOfOffice, false),
        // CloudTwin extensions for event shapes the real simulator can't produce.
        "delay" => (Outcome::Delayed, false),
        "reject" => (Outcome::Delivered, true),
        _ => (Outcome::Delivered, false),
    })
}

pub fn is_simulator(addr: &str) -> bool {
    address_domain(addr).as_deref() == Some(SIMULATOR_DOMAIN)
}

fn addr_list(addr: Option<&Address>) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |a: &mail_parser::Addr| {
        if let Some(email) = &a.address {
            out.push(email.to_string());
        }
    };
    match addr {
        Some(Address::List(list)) => list.iter().for_each(&mut push),
        Some(Address::Group(groups)) => groups.iter().flat_map(|g| g.addresses.iter()).for_each(&mut push),
        None => {}
    }
    out
}

fn rfc2822_now() -> String {
    Utc::now().format("%a, %d %b %Y %H:%M:%S +0000").to_string()
}

/// Build the RFC 822 document for `Content.Simple`. `include_bcc` keeps Bcc
/// visible in the local inbox copy (real recipients never see it).
fn build_simple(
    req: &SendRequest,
    message_id: &str,
    subject: &str,
    text: Option<&str>,
    html: Option<&str>,
    extra: &[(String, String)],
    include_bcc: bool,
) -> String {
    let mut h = format!(
        "From: {}\r\nTo: {}\r\n",
        req.from,
        req.to.join(", ")
    );
    if !req.cc.is_empty() {
        h.push_str(&format!("Cc: {}\r\n", req.cc.join(", ")));
    }
    if include_bcc && !req.bcc.is_empty() {
        h.push_str(&format!("Bcc: {}\r\n", req.bcc.join(", ")));
    }
    if !req.reply_to.is_empty() {
        h.push_str(&format!("Reply-To: {}\r\n", req.reply_to.join(", ")));
    }
    h.push_str(&format!(
        "Subject: {subject}\r\nMessage-ID: <{message_id}>\r\nDate: {}\r\nMIME-Version: 1.0\r\n",
        rfc2822_now()
    ));
    for (k, v) in extra {
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    match (text, html) {
        (Some(t), Some(ht)) => {
            let b = format!("----=_CloudTwin_{}", uuid::Uuid::new_v4().simple());
            format!(
                "{h}Content-Type: multipart/alternative; boundary=\"{b}\"\r\n\r\n\
                 --{b}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{t}\r\n\
                 --{b}\r\nContent-Type: text/html; charset=UTF-8\r\n\r\n{ht}\r\n--{b}--\r\n"
            )
        }
        (None, Some(ht)) => format!("{h}Content-Type: text/html; charset=UTF-8\r\n\r\n{ht}\r\n"),
        (Some(t), None) => format!("{h}Content-Type: text/plain; charset=UTF-8\r\n\r\n{t}\r\n"),
        (None, None) => format!("{h}\r\n"),
    }
}

/// Insert a header at the top of a raw message unless it already exists.
fn ensure_header(raw: &[u8], name: &str, value: &str) -> Vec<u8> {
    let has = events::parse_headers(raw)
        .iter()
        .any(|(n, _)| n.eq_ignore_ascii_case(name));
    if has {
        return raw.to_vec();
    }
    let mut out = format!("{name}: {value}\r\n").into_bytes();
    out.extend_from_slice(raw);
    out
}

impl SesService {
    pub async fn send_email(&self, mut req: SendRequest) -> SesResult<String> {
        let account = self.account().await?;
        if !account.sending_enabled {
            return Err(SesError::new(
                SesErrorKind::SendingPaused,
                "Sending is paused for this account.",
            ));
        }

        // Raw messages carry sender/recipients in headers when not given explicitly.
        let mut raw_subject = String::new();
        if let SendContent::Raw(raw) = &req.content {
            if raw.len() > MAX_MESSAGE_BYTES {
                return Err(SesError::new(
                    SesErrorKind::MessageRejected,
                    "Message length is more than 40 megabytes.",
                ));
            }
            if let Some(m) = MessageParser::default().parse(raw.as_slice()) {
                if req.from.is_empty() {
                    req.from = addr_list(m.from()).into_iter().next().unwrap_or_default();
                }
                if req.to.is_empty() && req.cc.is_empty() && req.bcc.is_empty() {
                    req.to = addr_list(m.to());
                    req.cc = addr_list(m.cc());
                    req.bcc = addr_list(m.bcc());
                }
                raw_subject = m.subject().unwrap_or_default().to_string();
            }
        }
        if req.from.trim().is_empty() {
            return Err(bad_request("Missing required parameter: FromEmailAddress."));
        }
        let recipients: Vec<String> = req
            .to
            .iter()
            .chain(&req.cc)
            .chain(&req.bcc)
            .cloned()
            .collect();
        if recipients.is_empty() {
            return Err(bad_request("Missing required parameter: Destination."));
        }
        if recipients.len() > MAX_RECIPIENTS {
            return Err(SesError::new(
                SesErrorKind::MessageRejected,
                format!("Recipient count exceeds {MAX_RECIPIENTS}."),
            ));
        }

        let sender = bare_address(&req.from);
        let identity = self.sending_identity(&sender).await?;
        if identity.is_none() && self.state.cfg.ses.strict_verification {
            return Err(SesError::new(
                SesErrorKind::MessageRejected,
                format!(
                    "Email address is not verified. The following identities failed the check in region US-EAST-1: {sender}"
                ),
            ));
        }

        if !account.production_access_enabled {
            let mut unverified = Vec::new();
            for r in &recipients {
                if !is_simulator(r) && self.sending_identity(r).await?.is_none() {
                    unverified.push(bare_address(r));
                }
            }
            if !unverified.is_empty() {
                return Err(SesError::new(
                    SesErrorKind::MessageRejected,
                    format!(
                        "Email address is not verified. The following identities failed the check in region US-EAST-1: {}",
                        unverified.join(", ")
                    ),
                ));
            }
        }

        let configuration_set = req
            .configuration_set
            .clone()
            .or_else(|| identity.as_ref().and_then(|i| i.configuration_set.clone()));
        if let Some(cs) = &configuration_set {
            self.require_configuration_set(cs).await?;
        }

        let mut mail_from = None;
        if let Some(id) = &identity {
            if let Some(domain) = &id.mail_from_domain {
                if id.mail_from.status == SUCCESS {
                    mail_from = Some(domain.clone());
                } else if id.mail_from_behavior == "REJECT_MESSAGE" {
                    return Err(SesError::new(
                        SesErrorKind::MailFromDomainNotVerified,
                        format!(
                            "The message could not be sent because Amazon SES could not read the MX record required to use the specified MAIL FROM domain {domain}."
                        ),
                    ));
                }
            }
        }

        let now = Utc::now();
        let message_id = format!("{}@cloudtwin.local", uuid::Uuid::new_v4());

        // Per-recipient outcomes.
        let suppressed: Vec<String> = {
            let mut s = Vec::new();
            for r in &recipients {
                if !is_simulator(r) && self.get_suppressed(r).await?.is_some() {
                    s.push(r.clone());
                }
            }
            s
        };
        let mut rejected = false;
        let mut outcomes: Vec<(String, Outcome)> = Vec::new();
        for r in &recipients {
            let outcome = if let Some((o, rejects)) = simulator_outcome(r) {
                rejected |= rejects;
                o
            } else if suppressed.contains(r) {
                Outcome::Bounce(
                    "OnAccountSuppressionList",
                    "Amazon SES did not send the message to this address because it is on the suppression list for your account.",
                )
            } else {
                Outcome::Delivered
            };
            outcomes.push((r.clone(), outcome));
        }

        // Message bodies: what recipients get, and the inbox copy (keeps Bcc).
        let (wire, inbox, subject, text_body, html_body) = match &req.content {
            SendContent::Simple { subject, text, html, headers } => {
                let wire = build_simple(&req, &message_id, subject, text.as_deref(), html.as_deref(), headers, false);
                let inbox = build_simple(&req, &message_id, subject, text.as_deref(), html.as_deref(), headers, true);
                (wire.into_bytes(), inbox.into_bytes(), subject.clone(), text.clone(), html.clone())
            }
            SendContent::Raw(raw) => {
                let with_id = ensure_header(raw, "Message-ID", &format!("<{message_id}>"));
                let with_date = ensure_header(&with_id, "Date", &rfc2822_now());
                (with_date.clone(), with_date, raw_subject.clone(), None, None)
            }
        };
        let return_path = req
            .return_path
            .clone()
            .or_else(|| mail_from.as_ref().map(|d| format!("bounces@{d}")))
            .unwrap_or_else(|| sender.clone());
        let wire = ensure_header(&wire, "Return-Path", &format!("<{return_path}>"));

        let recipient_status: BTreeMap<String, String> = outcomes
            .iter()
            .map(|(r, o)| {
                let s = match o {
                    _ if rejected => "rejected",
                    Outcome::Bounce(..) => "bounced",
                    Outcome::Delayed => "delayed",
                    Outcome::Complaint => "complained",
                    _ => "delivered",
                };
                (r.clone(), s.to_string())
            })
            .collect();
        self.store_message(StoredMessage {
            message_id: message_id.clone(),
            source: req.from.clone(),
            destinations: recipients.clone(),
            subject,
            text_body,
            html_body,
            raw: String::from_utf8_lossy(&wire).to_string(),
            reply_to: req.reply_to.clone(),
            tags: req.tags.clone(),
            configuration_set: configuration_set.clone(),
            feedback_forwarding_address: req.feedback_forwarding_address.clone(),
            status: if rejected { "rejected" } else { "sent" }.into(),
            recipient_status,
        })
        .await?;

        let delivers_any = !rejected
            && outcomes
                .iter()
                .any(|(_, o)| !matches!(o, Outcome::Bounce(..)));
        if delivers_any {
            if let Err(e) = crate::mailbox::service::capture_raw(&self.state.db, &inbox, "ses").await {
                tracing::warn!("mailbox capture of SES message failed: {e}");
            }
        }

        // Events.
        let mut tags: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if let Some(cs) = &configuration_set {
            tags.insert("ses:configuration-set".into(), vec![cs.clone()]);
        }
        tags.insert("ses:source-ip".into(), vec!["127.0.0.1".into()]);
        if let Some(d) = address_domain(&sender) {
            tags.insert("ses:from-domain".into(), vec![d]);
        }
        tags.insert("ses:caller-identity".into(), vec!["cloudtwin".into()]);
        tags.insert("ses:outgoing-ip".into(), vec!["127.0.0.1".into()]);
        for (k, v) in &req.tags {
            tags.entry(k.clone()).or_default().push(v.clone());
        }
        let ctx = MailContext {
            timestamp: now,
            message_id: message_id.clone(),
            source: req.from.clone(),
            source_arn: identity.as_ref().map(|i| {
                format!(
                    "arn:aws:ses:{}:{}:identity/{}",
                    crate::aws::REGION,
                    crate::aws::ACCOUNT_ID,
                    i.identity
                )
            }),
            destination: recipients.clone(),
            headers: events::parse_headers(&wire),
            tags,
        };

        let cs = configuration_set.as_deref();
        self.publish_event(&ctx, cs, "Send", json!({})).await;
        if rejected {
            self.publish_event(&ctx, cs, "Reject", json!({"reason": "Bad content"}))
                .await;
            return Ok(message_id);
        }

        let pick = |pred: &dyn Fn(&Outcome) -> bool| -> Vec<String> {
            outcomes
                .iter()
                .filter(|(_, o)| pred(o))
                .map(|(r, _)| bare_address(r))
                .collect()
        };
        let delivered = pick(&|o| {
            matches!(o, Outcome::Delivered | Outcome::OutOfOffice | Outcome::Complaint)
        });
        if !delivered.is_empty() {
            self.publish_event(&ctx, cs, "Delivery", events::delivery_detail(&delivered, now))
                .await;
        }
        let mut bounced: BTreeMap<(&str, &str), Vec<String>> = BTreeMap::new();
        for (r, o) in &outcomes {
            if let Outcome::Bounce(sub, diag) = o {
                bounced.entry((sub, diag)).or_default().push(bare_address(r));
            }
        }
        let feedback_enabled = identity.as_ref().map(|i| i.feedback_forwarding).unwrap_or(true);
        let feedback_to = req
            .feedback_forwarding_address
            .clone()
            .or(req.return_path.clone())
            .unwrap_or_else(|| sender.clone());
        for ((sub, diag), rcpts) in bounced {
            let detail = events::bounce_detail(&rcpts, "Permanent", sub, diag, now);
            self.publish_event(&ctx, cs, "Bounce", detail).await;
            if feedback_enabled {
                self.forward_feedback(&feedback_to, &message_id, "bounce", &rcpts, diag)
                    .await;
            }
        }
        let complained = pick(&|o| matches!(o, Outcome::Complaint));
        if !complained.is_empty() {
            self.publish_event(&ctx, cs, "Complaint", events::complaint_detail(&complained, now))
                .await;
            if feedback_enabled {
                self.forward_feedback(&feedback_to, &message_id, "complaint", &complained, "")
                    .await;
            }
        }
        let delayed = pick(&|o| matches!(o, Outcome::Delayed));
        if !delayed.is_empty() {
            self.publish_event(&ctx, cs, "DeliveryDelay", events::delivery_delay_detail(&delayed, now))
                .await;
        }
        for r in pick(&|o| matches!(o, Outcome::OutOfOffice)) {
            self.send_auto_reply(&r, &sender).await;
        }
        Ok(message_id)
    }

    /// Record an event and publish it to matching configuration-set
    /// destinations (SNS; other destination kinds are stored but not delivered).
    async fn publish_event(&self, ctx: &MailContext, cs: Option<&str>, event_type: &str, detail: Value) {
        let event = ctx.event(event_type, detail);
        self.record_event(&ctx.message_id, event_type, cs, &event).await;
        let Some(cs) = cs else { return };
        let destinations = match self.event_destinations(cs).await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("failed to load event destinations for {cs}: {}", e.message);
                return;
            }
        };
        for (name, def) in destinations {
            if def.get("Enabled").and_then(|v| v.as_bool()) == Some(false) {
                continue;
            }
            let matches = def
                .get("MatchingEventTypes")
                .and_then(|v| v.as_array())
                .map(|types| {
                    types.iter().any(|t| {
                        t.as_str().and_then(events::event_type_name) == Some(event_type)
                    })
                })
                .unwrap_or(false);
            if !matches {
                continue;
            }
            match def
                .get("SnsDestination")
                .and_then(|d| d.get("TopicArn"))
                .and_then(|v| v.as_str())
            {
                Some(topic) => {
                    if let Err(e) = crate::aws::sns::publish_internal(
                        &self.state,
                        topic,
                        event.to_string(),
                        None,
                        None,
                    )
                    .await
                    {
                        tracing::warn!("SES event destination {name} → {topic} failed: {e}");
                    }
                }
                None => tracing::debug!(
                    "SES event destination {name} has no SNS topic; event recorded only"
                ),
            }
        }
    }

    /// Capture a bounce/complaint feedback notification in the local inbox.
    async fn forward_feedback(&self, to: &str, message_id: &str, kind: &str, rcpts: &[String], diag: &str) {
        let (from, subject, body) = if kind == "bounce" {
            (
                "MAILER-DAEMON@amazonses.com",
                "Delivery Status Notification (Failure)",
                format!(
                    "An error occurred while trying to deliver the mail to the following recipients:\r\n{}\r\n\r\n{diag}\r\n\r\nOriginal message id: {message_id}\r\n",
                    rcpts.join("\r\n")
                ),
            )
        } else {
            (
                "complaints@email-abuse.amazonses.com",
                "Complaint about message",
                format!(
                    "A recipient marked your message as spam:\r\n{}\r\n\r\nOriginal message id: {message_id}\r\n",
                    rcpts.join("\r\n")
                ),
            )
        };
        let raw = format!(
            "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nDate: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{body}",
            rfc2822_now()
        );
        if let Err(e) = crate::mailbox::service::capture_raw(&self.state.db, raw.as_bytes(), "ses-feedback").await {
            tracing::warn!("failed to capture SES feedback notification: {e}");
        }
    }

    async fn send_auto_reply(&self, from: &str, to: &str) {
        let raw = format!(
            "From: {from}\r\nTo: {to}\r\nSubject: Out of the office\r\nDate: {}\r\nAuto-Submitted: auto-replied\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\nI am out of the office and will respond when I return.\r\n",
            rfc2822_now()
        );
        if let Err(e) = crate::mailbox::service::capture_raw(&self.state.db, raw.as_bytes(), "ses-simulator").await {
            tracing::warn!("failed to capture SES auto-reply: {e}");
        }
    }

    async fn store_message(&self, m: StoredMessage) -> SesResult<()> {
        self.state
            .db
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO ses_messages
                     (message_id, source, destinations, subject, text_body, html_body, status,
                      created_at, raw, reply_to, tags, configuration_set,
                      feedback_forwarding_address, recipient_status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    rusqlite::params![
                        m.message_id,
                        m.source,
                        serde_json::to_string(&m.destinations).unwrap_or_default(),
                        m.subject,
                        m.text_body,
                        m.html_body,
                        m.status,
                        Utc::now().to_rfc3339(),
                        m.raw,
                        serde_json::to_string(&m.reply_to).unwrap_or_default(),
                        serde_json::to_string(
                            &m.tags
                                .iter()
                                .map(|(k, v)| json!({"Name": k, "Value": v}))
                                .collect::<Vec<_>>()
                        )
                        .unwrap_or_default(),
                        m.configuration_set,
                        m.feedback_forwarding_address,
                        serde_json::to_string(&m.recipient_status).unwrap_or_default(),
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }
}

struct StoredMessage {
    message_id: String,
    source: String,
    destinations: Vec<String>,
    subject: String,
    text_body: Option<String>,
    html_body: Option<String>,
    raw: String,
    reply_to: Vec<String>,
    tags: Vec<(String, String)>,
    configuration_set: Option<String>,
    feedback_forwarding_address: Option<String>,
    status: String,
    recipient_status: BTreeMap<String, String>,
}
