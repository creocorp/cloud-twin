//! Cognito domain records and their AWS JSON shapes.

use rusqlite::Row;
use serde_json::{json, Map, Value};

use crate::aws::{ACCOUNT_ID, REGION};

pub fn now_f() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

pub fn now_i() -> i64 {
    chrono::Utc::now().timestamp()
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct Pool {
    pub id: String,
    pub name: String,
    pub data: Value,
    pub domain: Option<String>,
    pub created: f64,
    pub modified: f64,
}

pub struct PasswordPolicy {
    pub min_length: usize,
    pub upper: bool,
    pub lower: bool,
    pub numbers: bool,
    pub symbols: bool,
    pub temp_validity_days: i64,
}

impl Pool {
    pub const COLS: &'static str = "id, name, data, domain, created, modified";

    pub fn from_row(r: &Row) -> rusqlite::Result<Self> {
        let data: String = r.get(2)?;
        Ok(Pool {
            id: r.get(0)?,
            name: r.get(1)?,
            data: serde_json::from_str(&data).unwrap_or_else(|_| json!({})),
            domain: r.get(3)?,
            created: r.get(4)?,
            modified: r.get(5)?,
        })
    }

    pub fn arn(&self) -> String {
        format!("arn:aws:cognito-idp:{REGION}:{ACCOUNT_ID}:userpool/{}", self.id)
    }

    pub fn username_attributes(&self) -> Vec<String> {
        str_list(self.data.get("UsernameAttributes"))
    }

    pub fn alias_attributes(&self) -> Vec<String> {
        str_list(self.data.get("AliasAttributes"))
    }

    pub fn auto_verified(&self) -> Vec<String> {
        str_list(self.data.get("AutoVerifiedAttributes"))
    }

    pub fn email_is_username(&self) -> bool {
        self.username_attributes().iter().any(|a| a == "email")
    }

    pub fn case_sensitive(&self) -> bool {
        self.data
            .pointer("/UsernameConfiguration/CaseSensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn admin_create_only(&self) -> bool {
        self.data
            .pointer("/AdminCreateUserConfig/AllowAdminCreateUserOnly")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn password_policy(&self) -> PasswordPolicy {
        match self.data.pointer("/Policies/PasswordPolicy") {
            Some(p) => PasswordPolicy {
                min_length: p.get("MinimumLength").and_then(|v| v.as_u64()).unwrap_or(8) as usize,
                upper: p.get("RequireUppercase").and_then(|v| v.as_bool()).unwrap_or(false),
                lower: p.get("RequireLowercase").and_then(|v| v.as_bool()).unwrap_or(false),
                numbers: p.get("RequireNumbers").and_then(|v| v.as_bool()).unwrap_or(false),
                symbols: p.get("RequireSymbols").and_then(|v| v.as_bool()).unwrap_or(false),
                temp_validity_days: p
                    .get("TemporaryPasswordValidityDays")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(7),
            },
            None => PasswordPolicy {
                min_length: 8,
                upper: true,
                lower: true,
                numbers: true,
                symbols: true,
                temp_validity_days: 7,
            },
        }
    }

    fn policies_json(&self) -> Value {
        let p = self.password_policy();
        let mut policies = self.data.get("Policies").cloned().unwrap_or_else(|| json!({}));
        policies["PasswordPolicy"] = json!({
            "MinimumLength": p.min_length,
            "RequireUppercase": p.upper,
            "RequireLowercase": p.lower,
            "RequireNumbers": p.numbers,
            "RequireSymbols": p.symbols,
            "TemporaryPasswordValidityDays": p.temp_validity_days,
        });
        policies
    }

    pub fn summary(&self) -> Value {
        json!({
            "Id": self.id,
            "Name": self.name,
            "LambdaConfig": {},
            "Status": "Enabled",
            "LastModifiedDate": self.modified,
            "CreationDate": self.created,
        })
    }

    pub fn describe(&self, estimated_users: i64) -> Value {
        let mut out = match self.data.clone() {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        out.insert("Id".into(), json!(self.id));
        out.insert("Name".into(), json!(self.name));
        out.insert("Arn".into(), json!(self.arn()));
        out.insert("Policies".into(), self.policies_json());
        out.entry("LambdaConfig").or_insert_with(|| json!({}));
        out.entry("MfaConfiguration").or_insert_with(|| json!("OFF"));
        out.entry("DeletionProtection").or_insert_with(|| json!("INACTIVE"));
        out.entry("SchemaAttributes").or_insert_with(default_schema);
        out.entry("AdminCreateUserConfig").or_insert_with(|| {
            json!({"AllowAdminCreateUserOnly": false, "UnusedAccountValidityDays": 7})
        });
        out.entry("VerificationMessageTemplate")
            .or_insert_with(|| json!({"DefaultEmailOption": "CONFIRM_WITH_CODE"}));
        out.entry("EmailConfiguration")
            .or_insert_with(|| json!({"EmailSendingAccount": "COGNITO_DEFAULT"}));
        out.entry("UserPoolTags").or_insert_with(|| json!({}));
        out.insert("EstimatedNumberOfUsers".into(), json!(estimated_users));
        out.insert("LastModifiedDate".into(), json!(self.modified));
        out.insert("CreationDate".into(), json!(self.created));
        if let Some(d) = &self.domain {
            out.insert("Domain".into(), json!(d));
        }
        Value::Object(out)
    }
}

fn default_schema() -> Value {
    let s = |name: &str, ty: &str, mutable: bool, required: bool| {
        let mut v = json!({
            "Name": name,
            "AttributeDataType": ty,
            "DeveloperOnlyAttribute": false,
            "Mutable": mutable,
            "Required": required,
        });
        if ty == "String" {
            v["StringAttributeConstraints"] = json!({"MinLength": "0", "MaxLength": "2048"});
        }
        v
    };
    json!([
        s("sub", "String", false, true),
        s("name", "String", true, false),
        s("given_name", "String", true, false),
        s("family_name", "String", true, false),
        s("middle_name", "String", true, false),
        s("nickname", "String", true, false),
        s("preferred_username", "String", true, false),
        s("profile", "String", true, false),
        s("picture", "String", true, false),
        s("website", "String", true, false),
        s("email", "String", true, false),
        s("email_verified", "Boolean", true, false),
        s("gender", "String", true, false),
        s("birthdate", "String", true, false),
        s("zoneinfo", "String", true, false),
        s("locale", "String", true, false),
        s("phone_number", "String", true, false),
        s("phone_number_verified", "Boolean", true, false),
        s("address", "String", true, false),
        s("updated_at", "Number", true, false),
    ])
}

#[derive(Debug, Clone)]
pub struct Client {
    pub client_id: String,
    pub pool_id: String,
    pub name: String,
    pub secret: Option<String>,
    pub data: Value,
    pub created: f64,
    pub modified: f64,
}

/// Token kinds with independently configurable lifetimes.
#[derive(Clone, Copy)]
pub enum TokenKind {
    Id,
    Access,
    Refresh,
}

impl Client {
    pub const COLS: &'static str = "client_id, pool_id, name, secret, data, created, modified";

    pub fn from_row(r: &Row) -> rusqlite::Result<Self> {
        let data: String = r.get(4)?;
        Ok(Client {
            client_id: r.get(0)?,
            pool_id: r.get(1)?,
            name: r.get(2)?,
            secret: r.get(3)?,
            data: serde_json::from_str(&data).unwrap_or_else(|_| json!({})),
            created: r.get(5)?,
            modified: r.get(6)?,
        })
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        str_list(self.data.get(key))
    }

    pub fn oauth_enabled(&self) -> bool {
        self.data
            .get("AllowedOAuthFlowsUserPoolClient")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn prevent_user_existence_errors(&self) -> bool {
        self.data.get("PreventUserExistenceErrors").and_then(|v| v.as_str()) == Some("ENABLED")
    }

    /// Whether an `ExplicitAuthFlows` entry (new `ALLOW_*` or legacy name) is permitted.
    /// Clients created without explicit flows accept every password flow.
    pub fn allows_flow(&self, allow_name: &str, legacy: &str) -> bool {
        let flows = self.list("ExplicitAuthFlows");
        flows.is_empty() || flows.iter().any(|f| f == allow_name || f == legacy)
    }

    pub fn auth_session_minutes(&self) -> i64 {
        self.data
            .get("AuthSessionValidity")
            .and_then(|v| v.as_i64())
            .unwrap_or(3)
    }

    /// Token lifetime in seconds: client setting, then config override, then Cognito default.
    pub fn ttl(&self, kind: TokenKind, cfg: &super::CognitoConfig) -> i64 {
        let (field, unit_field, default_unit, cfg_val, default) = match kind {
            TokenKind::Id => ("IdTokenValidity", "IdToken", "hours", cfg.id_token_ttl_seconds, 3600),
            TokenKind::Access => (
                "AccessTokenValidity",
                "AccessToken",
                "hours",
                cfg.access_token_ttl_seconds,
                3600,
            ),
            TokenKind::Refresh => (
                "RefreshTokenValidity",
                "RefreshToken",
                "days",
                cfg.refresh_token_ttl_seconds,
                30 * 86400,
            ),
        };
        if let Some(v) = self.data.get(field).and_then(|v| v.as_i64()) {
            let unit = self
                .data
                .pointer(&format!("/TokenValidityUnits/{unit_field}"))
                .and_then(|v| v.as_str())
                .unwrap_or(default_unit);
            let mult = match unit {
                "seconds" => 1,
                "minutes" => 60,
                "hours" => 3600,
                _ => 86400,
            };
            return v * mult;
        }
        cfg_val.map(|v| v as i64).unwrap_or(default)
    }

    pub fn summary(&self) -> Value {
        json!({"ClientId": self.client_id, "UserPoolId": self.pool_id, "ClientName": self.name})
    }

    pub fn describe(&self) -> Value {
        let mut out = match self.data.clone() {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        out.insert("UserPoolId".into(), json!(self.pool_id));
        out.insert("ClientName".into(), json!(self.name));
        out.insert("ClientId".into(), json!(self.client_id));
        if let Some(s) = &self.secret {
            out.insert("ClientSecret".into(), json!(s));
        }
        out.entry("RefreshTokenValidity").or_insert_with(|| json!(30));
        out.entry("AllowedOAuthFlowsUserPoolClient").or_insert_with(|| json!(false));
        out.entry("EnableTokenRevocation").or_insert_with(|| json!(true));
        out.entry("EnablePropagateAdditionalUserContextData")
            .or_insert_with(|| json!(false));
        out.entry("AuthSessionValidity").or_insert_with(|| json!(3));
        out.insert("LastModifiedDate".into(), json!(self.modified));
        out.insert("CreationDate".into(), json!(self.created));
        Value::Object(out)
    }
}

#[derive(Debug, Clone)]
pub struct User {
    pub pool_id: String,
    pub username: String,
    pub sub: String,
    pub attributes: Map<String, Value>,
    pub password_hash: Option<String>,
    pub password_salt: Option<String>,
    pub status: String,
    pub enabled: bool,
    pub created: f64,
    pub modified: f64,
    pub tokens_valid_after: f64,
}

impl User {
    pub const COLS: &'static str = "pool_id, username, sub, attributes, password_hash, password_salt, \
         status, enabled, created, modified, tokens_valid_after";

    pub fn from_row(r: &Row) -> rusqlite::Result<Self> {
        let attrs: String = r.get(3)?;
        Ok(User {
            pool_id: r.get(0)?,
            username: r.get(1)?,
            sub: r.get(2)?,
            attributes: match serde_json::from_str(&attrs) {
                Ok(Value::Object(m)) => m,
                _ => Map::new(),
            },
            password_hash: r.get(4)?,
            password_salt: r.get(5)?,
            status: r.get(6)?,
            enabled: r.get::<_, i64>(7)? != 0,
            created: r.get(8)?,
            modified: r.get(9)?,
            tokens_valid_after: r.get(10)?,
        })
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        if name == "sub" {
            return Some(&self.sub);
        }
        self.attributes.get(name).and_then(|v| v.as_str())
    }

    /// `[{Name, Value}]` with `sub` first, optionally restricted to `only`.
    pub fn attribute_list(&self, only: Option<&[String]>) -> Vec<Value> {
        let keep = |n: &str| only.is_none_or(|o| o.iter().any(|x| x == n));
        let mut out = Vec::new();
        if keep("sub") {
            out.push(json!({"Name": "sub", "Value": self.sub}));
        }
        for (k, v) in &self.attributes {
            if keep(k) {
                out.push(json!({"Name": k, "Value": v.as_str().unwrap_or_default()}));
            }
        }
        out
    }

    /// Shape used by AdminCreateUser and ListUsers (`Attributes`).
    pub fn list_shape(&self, only: Option<&[String]>) -> Value {
        json!({
            "Username": self.username,
            "Attributes": self.attribute_list(only),
            "UserCreateDate": self.created,
            "UserLastModifiedDate": self.modified,
            "Enabled": self.enabled,
            "UserStatus": self.status,
        })
    }

    /// Shape used by AdminGetUser (`UserAttributes`).
    pub fn get_shape(&self) -> Value {
        json!({
            "Username": self.username,
            "UserAttributes": self.attribute_list(None),
            "UserCreateDate": self.created,
            "UserLastModifiedDate": self.modified,
            "Enabled": self.enabled,
            "UserStatus": self.status,
            "MFAOptions": [],
            "UserMFASettingList": [],
        })
    }
}
