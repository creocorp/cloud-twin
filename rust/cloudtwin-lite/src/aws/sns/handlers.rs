//! SNS AWS Query protocol handlers.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{http::StatusCode, response::Response};

use crate::proto::{wrap_xml, xml_error_response, xml_escape, xml_ok};
use crate::AppState;
use super::service::SnsService;

fn svc(state: &Arc<AppState>) -> SnsService { SnsService::new(state.db.clone()) }

const NS: &str = "https://sns.amazonaws.com/doc/2010-03-31/";

pub const QUERY_ACTIONS: &[&str] = &[
    "CreateTopic",
    "ListTopics",
    "DeleteTopic",
    "Subscribe",
    "ListSubscriptionsByTopic",
    "Publish",
];

pub async fn handle_query(
    state: &Arc<AppState>,
    action: &str,
    params: &HashMap<String, String>,
) -> Response {
    match action {
        "CreateTopic" => {
            let name = params.get("Name").map(|s| s.as_str()).unwrap_or("");
            if name.is_empty() {
                return xml_error_response(StatusCode::BAD_REQUEST, "InvalidParameter", "Name required");
            }
            match svc(state).create_topic(name).await {
                Ok(arn) => xml_ok(wrap_xml(
                    "CreateTopic", NS,
                    &format!("<CreateTopicResult><TopicArn>{}</TopicArn></CreateTopicResult>", xml_escape(&arn)),
                )),
                Err(e) => xml_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "ListTopics" => {
            match svc(state).list_topics().await {
                Ok(topics) => {
                    let members: String = topics.iter().map(|t| {
                        format!("<member><TopicArn>{}</TopicArn></member>", xml_escape(&t.arn))
                    }).collect();
                    xml_ok(wrap_xml(
                        "ListTopics", NS,
                        &format!("<ListTopicsResult><Topics>{members}</Topics></ListTopicsResult>"),
                    ))
                }
                Err(e) => xml_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "DeleteTopic" => {
            let arn = params.get("TopicArn").map(|s| s.as_str()).unwrap_or("");
            match svc(state).delete_topic(arn).await {
                Ok(_)  => xml_ok(wrap_xml("DeleteTopic", NS, "<DeleteTopicResult/>")),
                Err(e) => xml_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "Subscribe" => {
            let topic_arn = params.get("TopicArn").map(|s| s.as_str()).unwrap_or("");
            let protocol  = params.get("Protocol").map(|s| s.as_str()).unwrap_or("");
            let endpoint  = params.get("Endpoint").map(|s| s.as_str()).unwrap_or("");
            match svc(state).subscribe(topic_arn, protocol, endpoint).await {
                Ok(sub_arn) => xml_ok(wrap_xml(
                    "Subscribe", NS,
                    &format!("<SubscribeResult><SubscriptionArn>{}</SubscriptionArn></SubscribeResult>", xml_escape(&sub_arn)),
                )),
                Err(e) if e.to_string().contains("NotFound") =>
                    xml_error_response(StatusCode::NOT_FOUND, "NotFound", &e.to_string()),
                Err(e) => xml_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "ListSubscriptionsByTopic" => {
            let topic_arn = params.get("TopicArn").map(|s| s.as_str()).unwrap_or("");
            match svc(state).list_subscriptions_by_topic(topic_arn).await {
                Ok(subs) => {
                    let members: String = subs.iter().map(|s| format!(
                        "<member>\
<SubscriptionArn>{}</SubscriptionArn>\
<TopicArn>{}</TopicArn>\
<Protocol>{}</Protocol>\
<Endpoint>{}</Endpoint>\
<Owner>000000000000</Owner>\
</member>",
                        xml_escape(&s.arn), xml_escape(&s.topic_arn),
                        xml_escape(&s.protocol), xml_escape(&s.endpoint),
                    )).collect();
                    xml_ok(wrap_xml(
                        "ListSubscriptionsByTopic", NS,
                        &format!("<ListSubscriptionsByTopicResult><Subscriptions>{members}</Subscriptions></ListSubscriptionsByTopicResult>"),
                    ))
                }
                Err(e) => xml_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        "Publish" => {
            let topic_arn = params.get("TopicArn").map(|s| s.as_str()).unwrap_or("");
            let message   = params.get("Message").map(|s| s.as_str()).unwrap_or("");
            let subject   = params.get("Subject").map(|s| s.as_str());
            match svc(state).publish(topic_arn, message, subject).await {
                Ok(mid) => xml_ok(wrap_xml(
                    "Publish", NS,
                    &format!("<PublishResult><MessageId>{}</MessageId></PublishResult>", xml_escape(&mid)),
                )),
                Err(e) => xml_error_response(StatusCode::INTERNAL_SERVER_ERROR, "InternalError", &e.to_string()),
            }
        }
        _ => xml_error_response(StatusCode::BAD_REQUEST, "InvalidAction", &format!("Unknown SNS action: {action}")),
    }
}
