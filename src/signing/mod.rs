pub mod lit;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::policy::types::PolicyVerdict;

// ── SigningResult ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningResult {
    pub signed: bool,
    pub signature: Option<String>,
    pub signer_address: Option<String>,
    pub reason: Option<String>,
}

// ── SigningError ─────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("Signing service unavailable: {0}")]
    Unavailable(String),

    #[error("Signing rejected: {0}")]
    Rejected(String),

    #[error("Internal signing error: {0}")]
    Internal(String),
}

// ── SigningProvider trait ────────────────────────────────────

#[async_trait]
pub trait SigningProvider: Send + Sync {
    async fn sign_verdict(
        &self,
        verdict: &PolicyVerdict,
        tx_hash: &str,
    ) -> Result<SigningResult, SigningError>;
}