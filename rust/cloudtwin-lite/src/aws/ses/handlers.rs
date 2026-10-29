//! SES v1 AWS Query protocol handlers (form-urlencoded in, XML out).

use std::collections::HashMap;
use std::sync::Arc;

use axum::{http::StatusCode, response::Response};
use base64::Engine;
use serde_json::{json, Value};

use super::models::*;
use super::sending::{SendContent, SendRequest};
use super::service::{CreateIdentityOpts, SesService};
use crate::proto::{json_to_xml, query_to_json, wrap_xml, xml_error_response, xml_escape, xml_ok};
use crate::telemetry;
use crate::AppState;

const NS: &str = "http://ses.amazonaws.com/doc/2010-12-01/";

/// AWS Query actions routed to SES.
pub const QUERY_ACTIONS: &[&str] = &[
    "SendEmail",
    "SendRawEmail",
    "VerifyEmailIdentity",
    "VerifyEmailAddress",
    "VerifyDomainIdentity",
    "VerifyDomainDkim",
    "ListIdentities",
    "ListVerifiedEmailAddresses",
    "DeleteIdentity",
    "DeleteVerifiedEmailAddress",
    "GetIdentityVerificationAttributes",
    "GetIdentityDkimAttributes",
    "SetIdentityDkimEnabled",
    "SetIdentityMailFromDomain",
    "GetIdentityMailFromDomainAttributes",
    "SetIdentityFeedbackForwardingEnabled",
    "GetIdentityNotificationAttributes",
    "GetSendQuota",
    "GetSendStatistics",
    "GetAccountSendingEnabled",
    "UpdateAccountSendingEnabled",
    "CreateReceiptRuleSet",
    "DeleteReceiptRuleSet",
    "ListReceiptRuleSets",
    "DescribeReceiptRuleSet",
    "CloneReceiptRuleSet",
    "SetActiveReceiptRuleSet",
    "DescribeActiveReceiptRuleSet",
    "ReorderReceiptRuleSet",
    "CreateReceiptRule",
    "UpdateReceiptRule",
    "DeleteReceiptRule",
    "DescribeReceiptRule",
    "SetReceiptRulePosition",
    "CreateConfigurationSet",
    "DeleteConfigurationSet",
    "ListConfigurationSets",
    "DescribeConfigurationSet",
    "CreateConfigurationSetEventDestination",
    "UpdateConfigurationSetEventDestination",
    "DeleteConfigurationSetEventDestination",
];

fn ok(action: &str, inner: &str) -> Response {
    xml_ok(wrap_xml(
        action,
        NS,
        &format!("<{action}Result>{inner}</{action}Result>"),
    ))
}

fn err(e: SesError) -> Response {
    xml_error_response(
        StatusCode::from_u16(e.v1_status()).unwrap_or(StatusCode::BAD_REQUEST),
        e.v1_code(),
        &e.message,
    )
}

fn param<'a>(p: &'a HashMap<String, String>, k: &str) -> &'a str {
    p.get(k).map(|s| s.as_str()).unwrap_or("")
}

fn members(p: &HashMap<String, String>, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 1;
    while let Some(v) = p.get(&format!("{prefix}.member.{i}")) {
        out.push(v.clone());
        i += 1;
    }
    out
}

fn tags(p: &HashMap<String, String>, prefix: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 1;
    while let Some(n) = p.get(&format!("{prefix}.member.{i}.Name")) {
        let v = p
            .get(&format!("{prefix}.member.{i}.Value"))
            .cloned()
            .unwrap_or_default();
        out.push((n.clone(), v));
        i += 1;
    }
    out
}

fn ts_xml(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|t| iso_ms(t.with_timezone(&chrono::Utc)))
        .unwrap_or_else(|_| rfc3339.to_string())
}

fn opt(v: Option<&str>) -> Option<&str> {
    v.filter(|s| !s.is_empty())
}

/// v1 event types (`send`, `renderingFailure`) → stored v2 names (`SEND`, `RENDERING_FAILURE`).
fn v2_event_type(t: &str) -> String {
    let mut out = String::new();
    for (i, c) in t.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

fn v1_event_type(t: &str) -> String {
    let lower = t.to_ascii_lowercase();
    let mut out = String::new();
    let mut upper_next = false;
    for c in lower.chars() {
        if c == '_' {
            upper_next = true;
        } else if upper_next {
            out.push(c.to_ascii_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Decode a v1 `EventDestination.*` structure to the stored v2 definition.
fn v1_destination(p: &HashMap<String, String>) -> (String, Value) {
    let d = query_to_json(p, "EventDestination");
    let name = d.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let types: Vec<Value> = d
        .get("MatchingEventTypes")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str())
                .map(|t| json!(v2_event_type(t)))
                .collect()
        })
        .unwrap_or_default();
    let mut def = json!({
        "Enabled": d.get("Enabled").and_then(|v| v.as_str()).map(|s| s == "true").unwrap_or(false),
        "MatchingEventTypes": types,
    });
    if let Some(topic) = d.get("SNSDestination").and_then(|s| s.get("TopicARN")) {
        def["SnsDestination"] = json!({"TopicArn": topic});
    }
    for (v1, v2) in [
        ("CloudWatchDestination", "CloudWatchDestination"),
        ("KinesisFirehoseDestination", "KinesisFirehoseDestination"),
    ] {
        if let Some(x) = d.get(v1) {
            def[v2] = x.clone();
        }
    }
    (name, def)
}

fn v1_destination_xml(name: &str, def: &Value) -> String {
    let types: String = def["MatchingEventTypes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t.as_str())
        .map(|t| format!("<member>{}</member>", v1_event_type(t)))
        .collect();
    let sns = def
        .get("SnsDestination")
        .and_then(|s| s.get("TopicArn"))
        .and_then(|v| v.as_str())
        .map(|t| format!("<SNSDestination><TopicARN>{}</TopicARN></SNSDestination>", xml_escape(t)))
        .unwrap_or_default();
    format!(
        "<member><Name>{}</Name><Enabled>{}</Enabled><MatchingEventTypes>{types}</MatchingEventTypes>{sns}</member>",
        xml_escape(name),
        def["Enabled"].as_bool().unwrap_or(false)
    )
}

pub async fn handle_query(
    state: &Arc<AppState>,
    action: &str,
    p: &HashMap<String, String>,
) -> Response {
    let svc = SesService::new(state.clone());
    let emit = |a: &'static str, payload: Value| {
        let db = state.db.clone();
        async move { telemetry::emit(&db, "aws", "ses", a, &payload.to_string()).await }
    };

    match action {
        "VerifyEmailIdentity" | "VerifyEmailAddress" => {
            let email = param(p, "EmailAddress");
            if email.is_empty() {
                return err(bad_request("EmailAddress is required."));
            }
            match svc.create_identity(email, CreateIdentityOpts::default(), false).await {
                Ok(_) => {
                    emit("verify_identity", json!({"identity": email})).await;
                    if action == "VerifyEmailAddress" {
                        xml_ok(wrap_xml(action, NS, ""))
                    } else {
                        ok(action, "")
                    }
                }
                Err(e) => err(e),
            }
        }
        "VerifyDomainIdentity" => {
            let domain = param(p, "Domain");
            if domain.is_empty() {
                return err(bad_request("Domain is required."));
            }
            match svc.create_identity(domain, CreateIdentityOpts::default(), false).await {
                Ok(id) => {
                    emit("verify_identity", json!({"identity": domain})).await;
                    ok(
                        action,
                        &format!(
                            "<VerificationToken>{}</VerificationToken>",
                            xml_escape(id.token.as_deref().unwrap_or(""))
                        ),
                    )
                }
                Err(e) => err(e),
            }
        }
        "VerifyDomainDkim" => {
            let domain = param(p, "Domain");
            match svc.create_identity(domain, CreateIdentityOpts::default(), false).await {
                Ok(id) => {
                    let tokens: String = id
                        .dkim_tokens
                        .iter()
                        .map(|t| format!("<member>{}</member>", xml_escape(t)))
                        .collect();
                    ok(action, &format!("<DkimTokens>{tokens}</DkimTokens>"))
                }
                Err(e) => err(e),
            }
        }
        "ListIdentities" | "ListVerifiedEmailAddresses" => {
            let kind = param(p, "IdentityType");
            match svc.list_identities().await {
                Ok(ids) => {
                    let filtered = ids.iter().filter(|i| match (action, kind) {
                        ("ListVerifiedEmailAddresses", _) => !i.is_domain() && i.verified(),
                        (_, "Domain") => i.is_domain(),
                        (_, "EmailAddress") => !i.is_domain(),
                        _ => true,
                    });
                    let list: String = filtered
                        .map(|i| format!("<member>{}</member>", xml_escape(&i.identity)))
                        .collect();
                    if action == "ListIdentities" {
                        ok(action, &format!("<Identities>{list}</Identities>"))
                    } else {
                        ok(action, &format!("<VerifiedEmailAddresses>{list}</VerifiedEmailAddresses>"))
                    }
                }
                Err(e) => err(e),
            }
        }
        "DeleteIdentity" | "DeleteVerifiedEmailAddress" => {
            let identity = if action == "DeleteIdentity" {
                param(p, "Identity")
            } else {
                param(p, "EmailAddress")
            };
            match svc.delete_identity(identity).await {
                Ok(_) => {
                    emit("delete_identity", json!({"identity": identity})).await;
                    if action == "DeleteIdentity" {
                        ok(action, "")
                    } else {
                        xml_ok(wrap_xml(action, NS, ""))
                    }
                }
                Err(e) => err(e),
            }
        }
        "GetIdentityVerificationAttributes" => {
            let mut entries = String::new();
            for ident in members(p, "Identities") {
                let (status, token) = match svc.get_identity(&ident, true).await {
                    Ok(Some(rec)) => (v1_status(&rec.status), rec.token.unwrap_or_default()),
                    Ok(None) => ("NotStarted", String::new()),
                    Err(e) => return err(e),
                };
                let token_xml = if token.is_empty() {
                    String::new()
                } else {
                    format!("<VerificationToken>{}</VerificationToken>", xml_escape(&token))
                };
                entries.push_str(&format!(
                    "<entry><key>{}</key><value><VerificationStatus>{status}</VerificationStatus>{token_xml}</value></entry>",
                    xml_escape(&ident)
                ));
            }
            ok(action, &format!("<VerificationAttributes>{entries}</VerificationAttributes>"))
        }
        "GetIdentityDkimAttributes" => {
            let mut entries = String::new();
            for ident in members(p, "Identities") {
                let rec = match svc.get_identity(&ident, true).await {
                    Ok(r) => r,
                    Err(e) => return err(e),
                };
                let (enabled, status, tokens) = match &rec {
                    Some(r) => (
                        r.dkim_signing_enabled,
                        v1_status(r.dkim_status()),
                        r.dkim_tokens
                            .iter()
                            .map(|t| format!("<member>{}</member>", xml_escape(t)))
                            .collect::<String>(),
                    ),
                    None => (false, "NotStarted", String::new()),
                };
                entries.push_str(&format!(
                    "<entry><key>{}</key><value><DkimEnabled>{enabled}</DkimEnabled>\
<DkimVerificationStatus>{status}</DkimVerificationStatus><DkimTokens>{tokens}</DkimTokens></value></entry>",
                    xml_escape(&ident)
                ));
            }
            ok(action, &format!("<DkimAttributes>{entries}</DkimAttributes>"))
        }
        "SetIdentityDkimEnabled" => {
            let enabled = param(p, "DkimEnabled") == "true";
            match svc.put_dkim_signing(param(p, "Identity"), enabled).await {
                Ok(()) => ok(action, ""),
                Err(e) => err(e),
            }
        }
        "SetIdentityMailFromDomain" => {
            let behavior = opt(p.get("BehaviorOnMXFailure").map(|s| s.as_str()));
            match svc
                .put_mail_from(param(p, "Identity"), opt(p.get("MailFromDomain").map(|s| s.as_str())), behavior)
                .await
            {
                Ok(()) => {
                    emit("put_mail_from", json!({"identity": param(p, "Identity")})).await;
                    ok(action, "")
                }
                Err(e) => err(e),
            }
        }
        "GetIdentityMailFromDomainAttributes" => {
            let mut entries = String::new();
            for ident in members(p, "Identities") {
                match svc.get_identity(&ident, true).await {
                    Ok(Some(r)) => {
                        let behavior = if r.mail_from_behavior == "REJECT_MESSAGE" {
                            "RejectMessage"
                        } else {
                            "UseDefaultValue"
                        };
                        let domain = r
                            .mail_from_domain
                            .as_deref()
                            .map(|d| {
                                format!(
                                    "<MailFromDomain>{}</MailFromDomain><MailFromDomainStatus>{}</MailFromDomainStatus>",
                                    xml_escape(d),
                                    v1_status(&r.mail_from.status)
                                )
                            })
                            .unwrap_or_default();
                        entries.push_str(&format!(
                            "<entry><key>{}</key><value>{domain}<BehaviorOnMXFailure>{behavior}</BehaviorOnMXFailure></value></entry>",
                            xml_escape(&ident)
                        ));
                    }
                    Ok(None) => {}
                    Err(e) => return err(e),
                }
            }
            ok(action, &format!("<MailFromDomainAttributes>{entries}</MailFromDomainAttributes>"))
        }
        "SetIdentityFeedbackForwardingEnabled" => {
            let enabled = param(p, "ForwardingEnabled") == "true";
            match svc.put_feedback_forwarding(param(p, "Identity"), enabled).await {
                Ok(()) => ok(action, ""),
                Err(e) => err(e),
            }
        }
        "GetIdentityNotificationAttributes" => {
            let mut entries = String::new();
            for ident in members(p, "Identities") {
                if let Ok(Some(r)) = svc.get_identity(&ident, false).await {
                    entries.push_str(&format!(
                        "<entry><key>{}</key><value><ForwardingEnabled>{}</ForwardingEnabled>\
<HeadersInBounceNotificationsEnabled>false</HeadersInBounceNotificationsEnabled>\
<HeadersInComplaintNotificationsEnabled>false</HeadersInComplaintNotificationsEnabled>\
<HeadersInDeliveryNotificationsEnabled>false</HeadersInDeliveryNotificationsEnabled></value></entry>",
                        xml_escape(&ident),
                        r.feedback_forwarding
                    ));
                }
            }
            ok(action, &format!("<NotificationAttributes>{entries}</NotificationAttributes>"))
        }
        "GetSendQuota" => match svc.account().await {
            Ok(a) => ok(
                action,
                &format!(
                    "<Max24HourSend>{}</Max24HourSend><MaxSendRate>{}</MaxSendRate><SentLast24Hours>{}</SentLast24Hours>",
                    a.max_24_hour_send, a.max_send_rate, a.sent_last_24_hours
                ),
            ),
            Err(e) => err(e),
        },
        "GetSendStatistics" => ok(action, "<SendDataPoints/>"),
        "GetAccountSendingEnabled" => match svc.account().await {
            Ok(a) => ok(action, &format!("<Enabled>{}</Enabled>", a.sending_enabled)),
            Err(e) => err(e),
        },
        "UpdateAccountSendingEnabled" => {
            let enabled = param(p, "Enabled") != "false";
            match svc.set_account_setting("sending_enabled", json!(enabled)).await {
                Ok(()) => xml_ok(wrap_xml(action, NS, "")),
                Err(e) => err(e),
            }
        }
        "SendEmail" => {
            let req = SendRequest {
                from: param(p, "Source").to_string(),
                to: members(p, "Destination.ToAddresses"),
                cc: members(p, "Destination.CcAddresses"),
                bcc: members(p, "Destination.BccAddresses"),
                reply_to: members(p, "ReplyToAddresses"),
                content: SendContent::Simple {
                    subject: param(p, "Message.Subject.Data").to_string(),
                    text: p.get("Message.Body.Text.Data").cloned(),
                    html: p.get("Message.Body.Html.Data").cloned(),
                    headers: Vec::new(),
                },
                tags: tags(p, "Tags"),
                configuration_set: opt(p.get("ConfigurationSetName").map(|s| s.as_str())).map(String::from),
                feedback_forwarding_address: None,
                return_path: opt(p.get("ReturnPath").map(|s| s.as_str())).map(String::from),
            };
            let source = req.from.clone();
            match svc.send_email(req).await {
                Ok(mid) => {
                    emit("send_email", json!({"source": source, "message_id": mid})).await;
                    ok(action, &format!("<MessageId>{}</MessageId>", xml_escape(&mid)))
                }
                Err(e) => err(e),
            }
        }
        "SendRawEmail" => {
            let data = match base64::engine::general_purpose::STANDARD.decode(param(p, "RawMessage.Data")) {
                Ok(d) => d,
                Err(_) => return err(bad_request("RawMessage.Data must be base64-encoded.")),
            };
            let req = SendRequest {
                from: param(p, "Source").to_string(),
                to: members(p, "Destinations"),
                cc: Vec::new(),
                bcc: Vec::new(),
                reply_to: Vec::new(),
                content: SendContent::Raw(data),
                tags: tags(p, "Tags"),
                configuration_set: opt(p.get("ConfigurationSetName").map(|s| s.as_str())).map(String::from),
                feedback_forwarding_address: None,
                return_path: None,
            };
            match svc.send_email(req).await {
                Ok(mid) => {
                    emit("send_raw_email", json!({"message_id": mid})).await;
                    ok(action, &format!("<MessageId>{}</MessageId>", xml_escape(&mid)))
                }
                Err(e) => err(e),
            }
        }

        // ── Receipt rules ─────────────────────────────────────────────────
        "CreateReceiptRuleSet" => match svc.create_rule_set(param(p, "RuleSetName")).await {
            Ok(()) => {
                emit("create_receipt_rule_set", json!({"name": param(p, "RuleSetName")})).await;
                ok(action, "")
            }
            Err(e) => err(e),
        },
        "DeleteReceiptRuleSet" => match svc.delete_rule_set(param(p, "RuleSetName")).await {
            Ok(()) => ok(action, ""),
            Err(e) => err(e),
        },
        "ListReceiptRuleSets" => match svc.list_rule_sets().await {
            Ok(sets) => {
                let list: String = sets
                    .iter()
                    .map(|(n, c)| {
                        format!(
                            "<member><Name>{}</Name><CreatedTimestamp>{}</CreatedTimestamp></member>",
                            xml_escape(n),
                            ts_xml(c)
                        )
                    })
                    .collect();
                ok(action, &format!("<RuleSets>{list}</RuleSets>"))
            }
            Err(e) => err(e),
        },
        "DescribeReceiptRuleSet" => {
            let name = param(p, "RuleSetName");
            match svc.describe_rule_set(name).await {
                Ok((created, rules)) => ok(action, &rule_set_xml(name, &created, &rules)),
                Err(e) => err(e),
            }
        }
        "DescribeActiveReceiptRuleSet" => match svc.active_rule_set().await {
            Ok(Some((name, created, rules))) => ok(action, &rule_set_xml(&name, &created, &rules)),
            Ok(None) => ok(action, ""),
            Err(e) => err(e),
        },
        "CloneReceiptRuleSet" => {
            match svc
                .clone_rule_set(param(p, "RuleSetName"), param(p, "OriginalRuleSetName"))
                .await
            {
                Ok(()) => ok(action, ""),
                Err(e) => err(e),
            }
        }
        "SetActiveReceiptRuleSet" => {
            match svc.set_active_rule_set(opt(p.get("RuleSetName").map(|s| s.as_str()))).await {
                Ok(()) => {
                    emit("set_active_receipt_rule_set", json!({"name": param(p, "RuleSetName")})).await;
                    ok(action, "")
                }
                Err(e) => err(e),
            }
        }
        "ReorderReceiptRuleSet" => {
            match svc
                .reorder_rule_set(param(p, "RuleSetName"), &members(p, "RuleNames"))
                .await
            {
                Ok(()) => ok(action, ""),
                Err(e) => err(e),
            }
        }
        "CreateReceiptRule" => {
            let rule = query_to_json(p, "Rule");
            match svc
                .create_rule(param(p, "RuleSetName"), rule, opt(p.get("After").map(|s| s.as_str())))
                .await
            {
                Ok(()) => {
                    emit("create_receipt_rule", json!({"rule_set": param(p, "RuleSetName")})).await;
                    ok(action, "")
                }
                Err(e) => err(e),
            }
        }
        "UpdateReceiptRule" => match svc.update_rule(param(p, "RuleSetName"), query_to_json(p, "Rule")).await {
            Ok(()) => ok(action, ""),
            Err(e) => err(e),
        },
        "DeleteReceiptRule" => match svc.delete_rule(param(p, "RuleSetName"), param(p, "RuleName")).await {
            Ok(()) => ok(action, ""),
            Err(e) => err(e),
        },
        "DescribeReceiptRule" => match svc.describe_rule(param(p, "RuleSetName"), param(p, "RuleName")).await {
            Ok(rule) => ok(action, &format!("<Rule>{}</Rule>", json_to_xml(&rule))),
            Err(e) => err(e),
        },
        "SetReceiptRulePosition" => {
            match svc
                .set_rule_position(
                    param(p, "RuleSetName"),
                    param(p, "RuleName"),
                    opt(p.get("After").map(|s| s.as_str())),
                )
                .await
            {
                Ok(()) => ok(action, ""),
                Err(e) => err(e),
            }
        }

        // ── Configuration sets (v1 view of the shared store) ──────────────
        "CreateConfigurationSet" => {
            match svc
                .create_configuration_set(param(p, "ConfigurationSet.Name"), json!({}))
                .await
            {
                Ok(()) => ok(action, ""),
                Err(e) => err(e),
            }
        }
        "DeleteConfigurationSet" => match svc.delete_configuration_set(param(p, "ConfigurationSetName")).await {
            Ok(()) => ok(action, ""),
            Err(e) => err(e),
        },
        "ListConfigurationSets" => match svc.list_configuration_sets().await {
            Ok(names) => {
                let list: String = names
                    .iter()
                    .map(|n| format!("<member><Name>{}</Name></member>", xml_escape(n)))
                    .collect();
                ok(action, &format!("<ConfigurationSets>{list}</ConfigurationSets>"))
            }
            Err(e) => err(e),
        },
        "DescribeConfigurationSet" => {
            let name = param(p, "ConfigurationSetName");
            if let Err(e) = svc.require_configuration_set(name).await {
                return err(e);
            }
            match svc.event_destinations(name).await {
                Ok(dests) => {
                    let list: String = dests.iter().map(|(n, d)| v1_destination_xml(n, d)).collect();
                    ok(
                        action,
                        &format!(
                            "<ConfigurationSet><Name>{}</Name></ConfigurationSet><EventDestinations>{list}</EventDestinations>",
                            xml_escape(name)
                        ),
                    )
                }
                Err(e) => err(e),
            }
        }
        "CreateConfigurationSetEventDestination" | "UpdateConfigurationSetEventDestination" => {
            let cs = param(p, "ConfigurationSetName");
            let (name, def) = v1_destination(p);
            let r = if action.starts_with("Create") {
                svc.create_event_destination(cs, &name, def).await
            } else {
                svc.update_event_destination(cs, &name, def).await
            };
            match r {
                Ok(()) => ok(action, ""),
                Err(e) => err(e),
            }
        }
        "DeleteConfigurationSetEventDestination" => {
            match svc
                .delete_event_destination(param(p, "ConfigurationSetName"), param(p, "EventDestinationName"))
                .await
            {
                Ok(()) => ok(action, ""),
                Err(e) => err(e),
            }
        }
        _ => xml_error_response(
            StatusCode::BAD_REQUEST,
            "InvalidAction",
            &format!("Unknown SES action: {action}"),
        ),
    }
}

fn rule_set_xml(name: &str, created: &str, rules: &[Value]) -> String {
    let rules_xml: String = rules
        .iter()
        .map(|r| format!("<member>{}</member>", json_to_xml(r)))
        .collect();
    format!(
        "<Metadata><Name>{}</Name><CreatedTimestamp>{}</CreatedTimestamp></Metadata><Rules>{rules_xml}</Rules>",
        xml_escape(name),
        ts_xml(created)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_type_case_conversion() {
        assert_eq!(v2_event_type("renderingFailure"), "RENDERING_FAILURE");
        assert_eq!(v2_event_type("send"), "SEND");
        assert_eq!(v1_event_type("DELIVERY_DELAY"), "deliveryDelay");
        assert_eq!(v1_event_type("BOUNCE"), "bounce");
    }
}
