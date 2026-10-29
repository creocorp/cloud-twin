//! GCP Cloud Storage models.

use base64::Engine;
use chrono::{DateTime, SecondsFormat, Utc};
use md5::{Digest, Md5};

#[derive(Debug, Clone)]
pub struct GcsBucket {
    pub name: String,
    #[allow(dead_code)]
    pub project: String,
    pub location: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct GcsObject {
    pub bucket: String,
    pub name: String,
    pub content: Vec<u8>,
    pub content_type: String,
    pub size: i64,
    pub etag: String,
    /// Creation time of the current generation (overwrites reset it).
    pub created_at: String,
    /// Optional object properties (`metadata`, `cacheControl`, ...) as JSON.
    pub props: serde_json::Value,
}

/// Object properties that are stored verbatim from upload/patch metadata.
pub const OBJECT_PROPS: &[&str] = &[
    "metadata",
    "cacheControl",
    "contentEncoding",
    "contentDisposition",
    "contentLanguage",
    "customTime",
];

impl GcsObject {
    /// Generation number: the creation time of this generation in microseconds,
    /// which (like real GCS) changes every time the object is overwritten.
    pub fn generation(&self) -> i64 {
        DateTime::parse_from_rfc3339(&self.created_at)
            .map(|d| d.timestamp_micros())
            .unwrap_or(1)
    }

    pub fn md5_b64(&self) -> String {
        let mut h = Md5::new();
        h.update(&self.content);
        base64::engine::general_purpose::STANDARD.encode(h.finalize())
    }

    pub fn crc32c_b64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(crc32c(&self.content).to_be_bytes())
    }
}

/// Current time as an RFC 3339 UTC timestamp (`...Z`), the format GCS uses.
pub fn now_ts() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Normalise a stored timestamp to RFC 3339 with a `Z` suffix (the SDK's
/// parser rejects `+00:00` offsets).
pub fn rfc3339_z(ts: &str) -> String {
    DateTime::parse_from_rfc3339(ts)
        .map(|d| {
            d.with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        })
        .unwrap_or_else(|_| ts.to_string())
}

/// CRC-32C (Castagnoli), as used by GCS object checksums.
pub fn crc32c(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0x82F6_3B78
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        t
    });
    let mut crc = !0u32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32c_known_vector() {
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(b""), 0);
    }

    #[test]
    fn timestamps_use_z_suffix() {
        assert_eq!(
            rfc3339_z("2024-01-02T03:04:05.123456+00:00"),
            "2024-01-02T03:04:05.123Z"
        );
        assert!(now_ts().ends_with('Z'));
    }
}
