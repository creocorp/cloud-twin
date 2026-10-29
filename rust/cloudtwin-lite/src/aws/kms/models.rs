//! KMS domain records and their AWS JSON shapes.

use serde_json::{json, Value};

use crate::aws::{ACCOUNT_ID, REGION};

pub fn key_arn(id: &str) -> String {
    format!("arn:aws:kms:{REGION}:{ACCOUNT_ID}:key/{id}")
}

pub fn alias_arn(name: &str) -> String {
    format!("arn:aws:kms:{REGION}:{ACCOUNT_ID}:{name}")
}

pub fn default_policy() -> String {
    json!({
        "Version": "2012-10-17",
        "Id": "key-default-1",
        "Statement": [{
            "Sid": "Enable IAM User Permissions",
            "Effect": "Allow",
            "Principal": {"AWS": format!("arn:aws:iam::{ACCOUNT_ID}:root")},
            "Action": "kms:*",
            "Resource": "*"
        }]
    })
    .to_string()
}

#[derive(Debug, Clone)]
pub struct KmsKey {
    pub id: String,
    pub description: String,
    pub key_usage: String,
    pub key_spec: String,
    pub origin: String,
    pub key_state: String,
    pub created_at: f64,
    pub deletion_date: Option<f64>,
    pub pending_window_days: Option<i64>,
    pub material: Option<Vec<u8>>,
    pub rotation_enabled: bool,
    pub rotation_period_days: i64,
    pub policy: String,
    pub tags: Vec<(String, String)>,
    pub multi_region: bool,
}

impl KmsKey {
    pub fn arn(&self) -> String {
        key_arn(&self.id)
    }

    pub fn is_symmetric(&self) -> bool {
        self.key_spec == "SYMMETRIC_DEFAULT"
    }

    pub fn tags_json(&self) -> Value {
        Value::Array(
            self.tags
                .iter()
                .map(|(k, v)| json!({"TagKey": k, "TagValue": v}))
                .collect(),
        )
    }

    /// The `KeyMetadata` structure returned by CreateKey / DescribeKey.
    pub fn metadata(&self) -> Value {
        let mut m = json!({
            "AWSAccountId": ACCOUNT_ID,
            "KeyId": self.id,
            "Arn": self.arn(),
            "CreationDate": self.created_at,
            "Enabled": self.key_state == "Enabled",
            "Description": self.description,
            "KeyUsage": self.key_usage,
            "KeyState": self.key_state,
            "Origin": self.origin,
            "KeyManager": "CUSTOMER",
            "CustomerMasterKeySpec": self.key_spec,
            "KeySpec": self.key_spec,
            "MultiRegion": self.multi_region,
        });
        let obj = m.as_object_mut().expect("object");
        if let Some(d) = self.deletion_date {
            obj.insert("DeletionDate".into(), json!(d));
        }
        if let Some(w) = self.pending_window_days {
            obj.insert("PendingDeletionWindowInDays".into(), json!(w));
        }
        let spec = self.key_spec.as_str();
        match self.key_usage.as_str() {
            "ENCRYPT_DECRYPT" if spec == "SYMMETRIC_DEFAULT" => {
                obj.insert("EncryptionAlgorithms".into(), json!(["SYMMETRIC_DEFAULT"]));
            }
            "ENCRYPT_DECRYPT" if spec.starts_with("RSA_") => {
                obj.insert(
                    "EncryptionAlgorithms".into(),
                    json!(["RSAES_OAEP_SHA_1", "RSAES_OAEP_SHA_256"]),
                );
            }
            "ENCRYPT_DECRYPT" if spec == "SM2" => {
                obj.insert("EncryptionAlgorithms".into(), json!(["SM2PKE"]));
            }
            "SIGN_VERIFY" => {
                let algs = match spec {
                    s if s.starts_with("RSA_") => json!([
                        "RSASSA_PSS_SHA_256",
                        "RSASSA_PSS_SHA_384",
                        "RSASSA_PSS_SHA_512",
                        "RSASSA_PKCS1_V1_5_SHA_256",
                        "RSASSA_PKCS1_V1_5_SHA_384",
                        "RSASSA_PKCS1_V1_5_SHA_512"
                    ]),
                    "ECC_NIST_P384" => json!(["ECDSA_SHA_384"]),
                    "ECC_NIST_P521" => json!(["ECDSA_SHA_512"]),
                    "SM2" => json!(["SM2DSA"]),
                    _ => json!(["ECDSA_SHA_256"]),
                };
                obj.insert("SigningAlgorithms".into(), algs);
            }
            "GENERATE_VERIFY_MAC" => {
                let alg = format!("HMAC_SHA_{}", spec.trim_start_matches("HMAC_"));
                obj.insert("MacAlgorithms".into(), json!([alg]));
            }
            "KEY_AGREEMENT" => {
                obj.insert("KeyAgreementAlgorithms".into(), json!(["ECDH"]));
            }
            _ => {}
        }
        m
    }
}

#[derive(Debug, Clone)]
pub struct KmsAlias {
    pub name: String,
    pub target_key_id: String,
    pub created_at: f64,
    pub updated_at: f64,
}

impl KmsAlias {
    pub fn to_json(&self) -> Value {
        json!({
            "AliasName": self.name,
            "AliasArn": alias_arn(&self.name),
            "TargetKeyId": self.target_key_id,
            "CreationDate": self.created_at,
            "LastUpdatedDate": self.updated_at,
        })
    }
}
