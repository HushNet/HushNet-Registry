// src/types.rs
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize)]
pub struct ChallengeReq { pub pubkey_b64: String }

#[derive(Serialize)]
pub struct ChallengeRes { pub nonce: String, pub expires_at: String }

#[derive(Deserialize)]
pub struct RegisterReq {
    pub payload: Value,
    pub nonce: String,
    pub signature_b64: String,
    pub pubkey_b64: String,
}

#[derive(Deserialize)]
pub struct HeartbeatReq {
    pub host: String,
    pub nonce: String,
    pub signature_b64: String,
    pub pubkey_b64: String,
}
