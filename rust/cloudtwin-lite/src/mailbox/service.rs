//! Mailbox capture + persistence logic.
//!
//! `capture_raw` is the single ingestion path: it parses a raw RFC 822 message
//! with `mail-parser`, extracts headers/bodies/attachments, and writes them to
//! SQLite. The SMTP listener and the SES integration both funnel through here so
//! the inbox stays uniform.

use anyhow::Result;
use chrono::Utc;
use mail_parser::{Address, MessageParser, MimeHeaders};
use uuid::Uuid;

use super::models::MailboxAttachment;
use crate::db::Database;
use crate::telemetry;

/// Parse and persist a raw RFC 822 message, returning the assigned message id.
///
/// `source` records where the message came from — `"smtp"` for the listener or
/// `"ses"` for AWS SES sends.
pub async fn capture_raw(db: &Database, raw: &[u8], source: &str) -> Result<String> {
    let message_id = format!("{}@cloudtwin.local", Uuid::new_v4());

    let parsed = MessageParser::default().parse(raw);
    let (from_addr, to_addrs, cc_addrs, bcc_addrs, subject, text_body, html_body, attachments) =
        match parsed.as_ref() {
            Some(m) => {
                let from = collect_addrs(m.from()).into_iter().next().unwrap_or_default();
                let to = collect_addrs(m.to());
                let cc = collect_addrs(m.cc());
                let bcc = collect_addrs(m.bcc());
                let subject = m.subject().unwrap_or_default().to_string();
                let text = m.body_text(0).map(|c| c.to_string());
                let html = m.body_html(0).map(|c| c.to_string());
                let atts: Vec<MailboxAttachment> = m
                    .attachments()
                    .map(|p| {
                        let content = p.contents().to_vec();
                        let content_type = p
                            .content_type()
                            .map(|ct| match ct.subtype() {
                                Some(sub) => format!("{}/{}", ct.ctype(), sub),
                                None => ct.ctype().to_string(),
                            })
                            .unwrap_or_else(|| "application/octet-stream".to_string());
                        MailboxAttachment {
                            filename: p
                                .attachment_name()
                                .unwrap_or("attachment")
                                .to_string(),
                            content_type,
                            size: content.len() as i64,
                            content,
                        }
                    })
                    .collect();
                (from, to, cc, bcc, subject, text, html, atts)
            }
            None => (
                String::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                String::new(),
                None,
                None,
                Vec::new(),
            ),
        };

    let to_json = serde_json::to_string(&to_addrs).unwrap_or_else(|_| "[]".into());
    let cc_json = serde_json::to_string(&cc_addrs).unwrap_or_else(|_| "[]".into());
    let bcc_json = serde_json::to_string(&bcc_addrs).unwrap_or_else(|_| "[]".into());
    let raw_str = String::from_utf8_lossy(raw).to_string();
    let size = raw.len() as i64;
    let now = Utc::now().to_rfc3339();

    let mid = message_id.clone();
    let src = source.to_string();
    db.conn
        .call(move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO mailbox_messages
                 (message_id, from_addr, to_addrs, cc_addrs, bcc_addrs, subject,
                  text_body, html_body, raw, source, size, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![
                    mid, from_addr, to_json, cc_json, bcc_json, subject, text_body,
                    html_body, raw_str, src, size, now
                ],
            )?;
            for att in &attachments {
                tx.execute(
                    "INSERT INTO mailbox_attachments
                     (message_id, filename, content_type, size, content, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        mid,
                        att.filename,
                        att.content_type,
                        att.size,
                        att.content,
                        now
                    ],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    telemetry::emit(
        db,
        "mailbox",
        "smtp",
        "capture",
        &serde_json::json!({ "source": source, "message_id": message_id }).to_string(),
    )
    .await;

    Ok(message_id)
}

/// Capture a message originating from an AWS SES send.
///
/// SES gives us already-structured fields, so we synthesize a minimal RFC 822
/// document and reuse [`capture_raw`] for a single, uniform ingestion path.
pub async fn capture_from_ses(
    db: &Database,
    source_addr: &str,
    destinations: &[String],
    subject: &str,
    text_body: Option<&str>,
    html_body: Option<&str>,
) -> Result<String> {
    let raw = build_raw(source_addr, destinations, subject, text_body, html_body);
    capture_raw(db, raw.as_bytes(), "ses").await
}

/// Build a simple RFC 822 message from structured fields.
fn build_raw(
    from: &str,
    to: &[String],
    subject: &str,
    text: Option<&str>,
    html: Option<&str>,
) -> String {
    let to_hdr = to.join(", ");
    let header = format!(
        "From: {from}\r\nTo: {to_hdr}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\n"
    );
    match (text, html) {
        (Some(t), Some(h)) => {
            let boundary = "----=_CloudTwinBoundary";
            format!(
                "{header}Content-Type: multipart/alternative; boundary=\"{boundary}\"\r\n\r\n\
                 --{boundary}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{t}\r\n\
                 --{boundary}\r\nContent-Type: text/html; charset=utf-8\r\n\r\n{h}\r\n\
                 --{boundary}--\r\n"
            )
        }
        (None, Some(h)) => {
            format!("{header}Content-Type: text/html; charset=utf-8\r\n\r\n{h}\r\n")
        }
        (Some(t), None) => {
            format!("{header}Content-Type: text/plain; charset=utf-8\r\n\r\n{t}\r\n")
        }
        (None, None) => format!("{header}\r\n"),
    }
}

/// Flatten a parsed address header into a list of bare email addresses.
fn collect_addrs(addr: Option<&Address>) -> Vec<String> {
    let mut out = Vec::new();
    match addr {
        Some(Address::List(list)) => {
            for ad in list {
                if let Some(email) = &ad.address {
                    out.push(email.to_string());
                }
            }
        }
        Some(Address::Group(groups)) => {
            for g in groups {
                for ad in &g.addresses {
                    if let Some(email) = &ad.address {
                        out.push(email.to_string());
                    }
                }
            }
        }
        None => {}
    }
    out
}
