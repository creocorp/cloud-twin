//! S3 error type shared by the service and protocol layers.

/// An S3 error with its AWS error code. `status()` maps the code to HTTP.
#[derive(Debug, Clone)]
pub struct S3Error {
    pub code: &'static str,
    pub message: String,
    /// Extra `<Error>` children (BucketName, Key, UploadId, ...).
    pub fields: Vec<(&'static str, String)>,
    /// Extra response headers (e.g. `x-amz-delete-marker`).
    pub headers: Vec<(String, String)>,
}

pub type S3Result<T> = Result<T, S3Error>;

impl S3Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        S3Error {
            code,
            message: message.into(),
            fields: Vec::new(),
            headers: Vec::new(),
        }
    }

    pub fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.fields.push((name, value.into()));
        self
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_string(), value.into()));
        self
    }

    pub fn internal(e: impl std::fmt::Display) -> Self {
        S3Error::new("InternalError", e.to_string())
    }

    pub fn no_such_bucket(bucket: &str) -> Self {
        S3Error::new("NoSuchBucket", "The specified bucket does not exist")
            .with("BucketName", bucket)
    }

    pub fn no_such_key(key: &str) -> Self {
        S3Error::new("NoSuchKey", "The specified key does not exist.").with("Key", key)
    }

    pub fn no_such_upload(upload_id: &str) -> Self {
        S3Error::new(
            "NoSuchUpload",
            "The specified upload does not exist. The upload ID may be invalid, or the upload may have been aborted or completed.",
        )
        .with("UploadId", upload_id)
    }

    pub fn malformed_xml() -> Self {
        S3Error::new(
            "MalformedXML",
            "The XML you provided was not well-formed or did not validate against our published schema",
        )
    }

    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        S3Error::new("InvalidArgument", msg)
    }

    pub fn status(&self) -> u16 {
        match self.code {
            "NoSuchBucket"
            | "NoSuchKey"
            | "NoSuchUpload"
            | "NoSuchVersion"
            | "NoSuchTagSet"
            | "NoSuchCORSConfiguration"
            | "NoSuchLifecycleConfiguration"
            | "NoSuchBucketPolicy"
            | "NoSuchPublicAccessBlockConfiguration"
            | "ServerSideEncryptionConfigurationNotFoundError"
            | "OwnershipControlsNotFoundError"
            | "NoSuchWebsiteConfiguration"
            | "ReplicationConfigurationNotFoundError"
            | "ObjectLockConfigurationNotFoundError" => 404,
            "BucketNotEmpty"
            | "BucketAlreadyExists"
            | "BucketAlreadyOwnedByYou"
            | "OperationAborted" => 409,
            "AccessDenied" | "CORSResponse" | "SignatureDoesNotMatch" => 403,
            "MethodNotAllowed" => 405,
            "PreconditionFailed" => 412,
            "InvalidRange" => 416,
            "NotImplemented" => 501,
            "InternalError" => 500,
            _ => 400,
        }
    }
}

impl std::fmt::Display for S3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for S3Error {}

impl From<rusqlite::Error> for S3Error {
    fn from(e: rusqlite::Error) -> Self {
        S3Error::internal(e)
    }
}

impl From<tokio_rusqlite::Error> for S3Error {
    fn from(e: tokio_rusqlite::Error) -> Self {
        S3Error::internal(e)
    }
}
