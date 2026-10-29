//! Secrets Manager persistence models.
#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct Secret {
    pub name: String,
    pub arn: String,
    pub secret_string: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
