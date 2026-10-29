//! SQS error values carrying both the modern JSON (`__type`) error name and the
//! legacy query-protocol code (`x-amzn-query-error`).

use std::fmt;

#[derive(Debug, Clone)]
pub struct SqsError {
    /// Shape name used in `__type` (`com.amazonaws.sqs#<code>`).
    pub code: &'static str,
    /// Legacy AWS Query error code (sent in `x-amzn-query-error`).
    pub query_code: &'static str,
    pub message: String,
    pub sender_fault: bool,
}

pub type Result<T> = std::result::Result<T, SqsError>;

impl SqsError {
    fn new(code: &'static str, query_code: &'static str, message: impl Into<String>) -> Self {
        SqsError {
            code,
            query_code,
            message: message.into(),
            sender_fault: true,
        }
    }

    pub fn queue_does_not_exist() -> Self {
        Self::new(
            "QueueDoesNotExist",
            "AWS.SimpleQueueService.NonExistentQueue",
            "The specified queue does not exist.",
        )
    }
    pub fn queue_name_exists(msg: impl Into<String>) -> Self {
        Self::new("QueueNameExists", "QueueAlreadyExists", msg)
    }
    pub fn receipt_handle_invalid(rh: &str) -> Self {
        Self::new(
            "ReceiptHandleIsInvalid",
            "ReceiptHandleIsInvalid",
            format!("The input receipt handle \"{rh}\" is not a valid receipt handle."),
        )
    }
    pub fn message_not_inflight() -> Self {
        Self::new(
            "MessageNotInflight",
            "AWS.SimpleQueueService.MessageNotInflight",
            "The message referred to isn't in flight.",
        )
    }
    pub fn invalid_attribute_name(msg: impl Into<String>) -> Self {
        Self::new("InvalidAttributeName", "InvalidAttributeName", msg)
    }
    pub fn invalid_attribute_value(msg: impl Into<String>) -> Self {
        Self::new("InvalidAttributeValue", "InvalidAttributeValue", msg)
    }
    pub fn invalid_parameter(msg: impl Into<String>) -> Self {
        Self::new("InvalidParameterValue", "InvalidParameterValue", msg)
    }
    pub fn missing_parameter(name: &str) -> Self {
        Self::new(
            "MissingParameter",
            "MissingParameter",
            format!("The request must contain the parameter {name}."),
        )
    }
    pub fn invalid_message_contents(msg: impl Into<String>) -> Self {
        Self::new("InvalidMessageContents", "InvalidMessageContents", msg)
    }
    pub fn empty_batch() -> Self {
        Self::new(
            "EmptyBatchRequest",
            "AWS.SimpleQueueService.EmptyBatchRequest",
            "There should be at least one SendMessageBatchRequestEntry in the request.",
        )
    }
    pub fn too_many_entries(n: usize) -> Self {
        Self::new(
            "TooManyEntriesInBatchRequest",
            "AWS.SimpleQueueService.TooManyEntriesInBatchRequest",
            format!("Maximum number of entries per request are 10. You have sent {n}."),
        )
    }
    pub fn ids_not_distinct(id: &str) -> Self {
        Self::new(
            "BatchEntryIdsNotDistinct",
            "AWS.SimpleQueueService.BatchEntryIdsNotDistinct",
            format!("Id {id} repeated."),
        )
    }
    pub fn invalid_batch_entry_id(id: &str) -> Self {
        Self::new(
            "InvalidBatchEntryId",
            "AWS.SimpleQueueService.InvalidBatchEntryId",
            format!(
                "A batch entry id can only contain alphanumeric characters, hyphens and underscores. It can be at most 80 letters long. Id: {id}"
            ),
        )
    }
    pub fn batch_too_long(limit: i64) -> Self {
        Self::new(
            "BatchRequestTooLong",
            "AWS.SimpleQueueService.BatchRequestTooLong",
            format!("Batch requests cannot be longer than {limit} bytes."),
        )
    }
    pub fn unsupported(msg: impl Into<String>) -> Self {
        Self::new(
            "UnsupportedOperation",
            "AWS.SimpleQueueService.UnsupportedOperation",
            msg,
        )
    }
    pub fn internal(e: impl fmt::Display) -> Self {
        SqsError {
            code: "InternalError",
            query_code: "InternalError",
            message: e.to_string(),
            sender_fault: false,
        }
    }

    pub fn status(&self) -> u16 {
        if self.sender_fault {
            400
        } else {
            500
        }
    }
}

impl fmt::Display for SqsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SqsError {}

impl From<rusqlite::Error> for SqsError {
    fn from(e: rusqlite::Error) -> Self {
        SqsError::internal(e)
    }
}

impl From<serde_json::Error> for SqsError {
    fn from(e: serde_json::Error) -> Self {
        SqsError::internal(e)
    }
}
