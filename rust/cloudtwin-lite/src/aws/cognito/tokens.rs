//! RS256 signing key management and hand-rolled JWT encode/verify.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::{Signature, SigningKey, VerifyingKey};
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rsa::signature::{SignatureEncoding, Signer, Verifier};
use rsa::traits::PublicKeyParts;
use rsa::RsaPrivateKey;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::AppState;

const META_KEY: &str = "cognito.signing_key";

pub struct SigningKeys {
    signer: SigningKey<Sha256>,
    verifier: VerifyingKey<Sha256>,
    pub kid: String,
    pub n: String,
    pub e: String,
}

fn cache() -> &'static Mutex<HashMap<[u8; 32], Arc<SigningKeys>>> {
    static CACHE: OnceLock<Mutex<HashMap<[u8; 32], Arc<SigningKeys>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn file_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn parse_pem(pem: &str) -> anyhow::Result<SigningKeys> {
    let key = RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(pem))
        .map_err(|e| anyhow::anyhow!("invalid Cognito signing key PEM: {e}"))?;
    let public = key.to_public_key();
    let n_bytes = public.n().to_bytes_be();
    let e_bytes = public.e().to_bytes_be();
    let mut h = Sha256::new();
    h.update(&n_bytes);
    h.update(&e_bytes);
    let kid = base64::engine::general_purpose::STANDARD.encode(h.finalize());
    Ok(SigningKeys {
        verifier: VerifyingKey::<Sha256>::new(public),
        signer: SigningKey::<Sha256>::new(key),
        kid,
        n: B64URL.encode(n_bytes),
        e: B64URL.encode(e_bytes),
    })
}

async fn load_pem(state: &AppState) -> anyhow::Result<String> {
    if let Some(path) = state.cfg.cognito.signing_key_path.clone() {
        if let Some(p) = file_cache().lock().unwrap().get(&path) {
            return Ok(p.clone());
        }
        let pem = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| anyhow::anyhow!("reading Cognito signing key {path}: {e}"))?;
        file_cache().lock().unwrap().insert(path, pem.clone());
        return Ok(pem);
    }
    if let Some(pem) = state.db.call(|c| crate::db::meta_get(c, META_KEY)).await? {
        return Ok(pem);
    }
    let generated = tokio::task::spawn_blocking(|| -> anyhow::Result<String> {
        let key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048)?;
        Ok(key.to_pkcs8_pem(LineEnding::LF)?.to_string())
    })
    .await??;
    // Another request may have generated a key concurrently; first writer wins.
    let pem = state
        .db
        .call(move |c| {
            if let Some(existing) = crate::db::meta_get(c, META_KEY)? {
                return Ok(existing);
            }
            crate::db::meta_set(c, META_KEY, &generated)?;
            Ok(generated)
        })
        .await?;
    Ok(pem)
}

/// The active signing key (generated and persisted on first use).
pub async fn keys(state: &AppState) -> anyhow::Result<Arc<SigningKeys>> {
    let pem = load_pem(state).await?;
    let hash: [u8; 32] = Sha256::digest(pem.as_bytes()).into();
    if let Some(k) = cache().lock().unwrap().get(&hash) {
        return Ok(k.clone());
    }
    let parsed = Arc::new(parse_pem(&pem)?);
    cache().lock().unwrap().insert(hash, parsed.clone());
    Ok(parsed)
}

impl SigningKeys {
    pub fn jwks(&self) -> Value {
        json!({"keys": [{
            "alg": "RS256",
            "e": self.e,
            "kid": self.kid,
            "kty": "RSA",
            "n": self.n,
            "use": "sig",
        }]})
    }

    pub fn sign(&self, claims: &Value) -> String {
        let header = json!({"kid": self.kid, "alg": "RS256"});
        let signing_input = format!(
            "{}.{}",
            B64URL.encode(header.to_string()),
            B64URL.encode(claims.to_string())
        );
        let sig = self.signer.sign(signing_input.as_bytes());
        format!("{signing_input}.{}", B64URL.encode(sig.to_bytes()))
    }

    /// Verify the signature and return the claims (expiry is checked by callers).
    pub fn verify(&self, token: &str) -> Option<Value> {
        let mut parts = token.split('.');
        let (h, p, s) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let header: Value = serde_json::from_slice(&B64URL.decode(h).ok()?).ok()?;
        if header.get("alg").and_then(|v| v.as_str()) != Some("RS256") {
            return None;
        }
        let sig = Signature::try_from(B64URL.decode(s).ok()?.as_slice()).ok()?;
        self.verifier
            .verify(format!("{h}.{p}").as_bytes(), &sig)
            .ok()?;
        serde_json::from_slice(&B64URL.decode(p).ok()?).ok()
    }
}
