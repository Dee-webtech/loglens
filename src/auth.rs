use axum::http::{header, HeaderMap};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn hash_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

pub fn new_key() -> String {
    let mut b = [0u8; 24];
    rand::rngs::OsRng.fill_bytes(&mut b);
    format!("ll_{}", hex::encode(b))
}

pub fn bearer(h: &HeaderMap) -> Option<&str> {
    let v = h.get(header::AUTHORIZATION)?.to_str().ok()?;
    v.strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|k| !k.is_empty())
}
