use async_trait::async_trait;
use serde_json::json;

use crate::policy::types::{Decision, PolicyVerdict};
use crate::signing::{SigningError, SigningProvider, SigningResult};

// ── LitSigningProvider (Chipotle REST API) ──────────────────
//
// Calls Lit Protocol's Chipotle REST API directly from Rust.
// No TypeScript sidecar needed — Chipotle replaced the SDK
// with a standard HTTP API authenticated via API key.
//
// Endpoint: POST /core/v1/lit_action
// Auth: X-Api-Key header
// Body: { "code": "<js string>", "js_params": { ... } }
//
// The Lit Action code is sent inline (not via IPFS CID) to avoid
// IPFS availability issues. The code is ~30 lines and deterministic.
//
// Setup (one-time via Chipotle Dashboard):
// 1. Create account → get account API key
// 2. Create usage API key
// 3. Create PKP wallet → note public key
// 4. Create group, add PKP to group
//
// Environment variables:
// - LIT_API_URL: Chipotle API base URL (e.g. https://api.dev.litprotocol.com)
// - LIT_API_KEY: Usage or account API key from Chipotle Dashboard
// - LIT_PKP_PUBLIC_KEY: PKP public key for signing

/// The Lit Action code that validates a Callipsos verdict and signs if approved.
/// Sent inline with each request to avoid IPFS dependency.
const LIT_ACTION_CODE: &str = r#"
(async () => {
  try {
    const parsedVerdict = typeof verdict === 'string'
      ? JSON.parse(verdict)
      : verdict;

    if (!parsedVerdict || parsedVerdict.decision !== 'approved') {
      Lit.Actions.setResponse({
        response: JSON.stringify({
          ok: false,
          reason: `Verdict decision is "${parsedVerdict?.decision || 'missing'}", not approved`,
        }),
      });
      return;
    }

    const failedRules = (parsedVerdict.results || []).filter(
      (r) => r.outcome !== 'pass'
    );
    if (failedRules.length > 0) {
      Lit.Actions.setResponse({
        response: JSON.stringify({
          ok: false,
          reason: `Verdict has ${failedRules.length} non-passing rules`,
        }),
      });
      return;
    }

    const toSign = ethers.utils.arrayify(txHash);

    const signature = await LitActions.signEcdsa({
      toSign,
      publicKey,
      sigName: 'transactionSignature',
    });

    Lit.Actions.setResponse({
      response: JSON.stringify({
        ok: true,
        signature,
        message: 'Transaction signed by Callipsos-gated PKP',
      }),
    });
  } catch (error) {
    Lit.Actions.setResponse({
      response: JSON.stringify({
        ok: false,
        reason: `Lit Action error: ${error.message}`,
      }),
    });
  }
})();
"#;

pub struct LitSigningProvider {
    api_url: String,
    api_key: String,
    pkp_public_key: String,
    client: reqwest::Client,
}

impl LitSigningProvider {
    pub fn new(
        api_url: String,
        api_key: String,
        pkp_public_key: String,
    ) -> Self {
        Self {
            api_url,
            api_key,
            pkp_public_key,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl SigningProvider for LitSigningProvider {
    async fn sign_verdict(
        &self,
        verdict: &PolicyVerdict,
        tx_hash: &str,
    ) -> Result<SigningResult, SigningError> {
        // Don't call Lit if verdict is not approved
        if verdict.decision != Decision::Approved {
            return Ok(SigningResult {
                signed: false,
                signature: None,
                signer_address: None,
                reason: Some("Verdict not approved".into()),
            });
        }

        // Serialize verdict for the Lit Action
        let verdict_json = serde_json::to_string(verdict)
            .map_err(|e| SigningError::Internal(format!("Failed to serialize verdict: {e}")))?;

        // Build js_params — these become top-level globals in the Lit Action
        let js_params = json!({
            "verdict": verdict_json,
            "txHash": tx_hash,
            "publicKey": self.pkp_public_key,
        });

        // Build the request body matching the Chipotle SDK:
        // POST /core/v1/lit_action { code, js_params }
        let body = json!({
            "code": LIT_ACTION_CODE,
            "js_params": js_params,
        });

        // POST to Chipotle API
        let response = self
            .client
            .post(format!("{}/core/v1/lit_action", self.api_url))
            .header("X-Api-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| SigningError::Unavailable(format!("Failed to reach Lit Chipotle API: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read response body".into());
            return Err(SigningError::Unavailable(format!(
                "Lit Chipotle API returned {status}: {body_text}"
            )));
        }

        // Parse Chipotle response: { signatures, response, logs, has_error }
        let resp_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| SigningError::Internal(format!("Failed to parse Lit response: {e}")))?;

        // Check has_error flag
        if resp_json["has_error"].as_bool() == Some(true) {
            let logs = resp_json["logs"].as_str().unwrap_or("no logs");
            return Err(SigningError::Rejected(format!("Lit Action error: {logs}")));
        }

        // The Lit Action sets its response via Lit.Actions.setResponse()
        // Chipotle returns it in the "response" field as a JSON string
        let action_response_str = resp_json["response"]
            .as_str()
            .ok_or_else(|| SigningError::Internal("Missing 'response' field in Lit result".into()))?;

        let action_response: serde_json::Value = serde_json::from_str(action_response_str)
            .map_err(|e| SigningError::Internal(format!("Failed to parse Lit Action response: {e}")))?;

        // Check if the Lit Action reported an error
        if action_response["ok"].as_bool() != Some(true) {
            let reason = action_response["reason"]
                .as_str()
                .unwrap_or("Lit Action returned ok=false")
                .to_string();
            return Err(SigningError::Rejected(reason));
        }

        // Map response fields to SigningResult
        Ok(SigningResult {
            signed: true,
            signature: action_response["signature"].as_str().map(|s| s.to_string()),
            signer_address: None, // PKP address can be derived from public key if needed
            reason: action_response["message"].as_str().map(|s| s.to_string()),
        })
    }
}