//! SNS topic and subscription records.

use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct SnsTopic {
    pub arn: String,
    pub attributes: BTreeMap<String, String>,
    pub tags: BTreeMap<String, String>,
}

impl SnsTopic {
    pub fn is_fifo(&self) -> bool {
        self.attributes
            .get("FifoTopic")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub struct SnsSubscription {
    pub arn: String,
    pub topic_arn: String,
    pub protocol: String,
    pub endpoint: String,
    pub attributes: BTreeMap<String, String>,
}

impl SnsSubscription {
    pub fn raw_delivery(&self) -> bool {
        self.attributes
            .get("RawMessageDelivery")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }
}
