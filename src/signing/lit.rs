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
// The Lit Action code is sent inline with each request.
// Inside the TEE, it retrieves the PKP's private key via
// getPrivateKey({ pkpId }), then signs the digest directly
// with ethers.utils.SigningKey.
//
// signEcdsa is gone in Chipotle. The replacement is getPrivateKey —
// you get the raw key inside the TEE and sign locally with ethers.
//
// Setup (one-time via Chipotle Dashboard):
// 1. Create account → get account API key
// 2. Create usage API key
// 3. Create PKP wallet → note wallet address (this is the pkpId)
//
// Environment variables:
// - LIT_API_URL: Chipotle API base URL (e.g. https://api.dev.litprotocol.com)
// - LIT_API_KEY: Usage or account API key from Chipotle Dashboard
// - LIT_PKP_ADDRESS: PKP wallet address from dashboard (used as pkpId)

/// Lit Action code that validates a Callipsos verdict and signs with the PKP.
/// Uses the new Chipotle pattern: getPrivateKey + ethers signing inside TEE.
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

    // Get the PKP's private key inside the TEE
    const privateKey = await Lit.Actions.getPrivateKey({ pkpId: pkpAddress });
    const signingKey = new ethers.utils.SigningKey(privateKey);
    const digestBytes = ethers.utils.arrayify(txHash);
    const sig = signingKey.signDigest(digestBytes);
    const signature = ethers.utils.joinSignature(sig);

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
    pkp_address: String,
    client: reqwest::Client,
}

impl LitSigningProvider {
    pub fn new(
        api_url: String,
        api_key: String,
        pkp_address: String,
    ) -> Self {
        Self {
            api_url,
            api_key,
            pkp_address,
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::limited(5))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
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
            "pkpAddress": self.pkp_address,
        });

        // Build the request body
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

        // Parse Chipotle response: { response, logs, has_error }
        let resp_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| SigningError::Internal(format!("Failed to parse Lit response: {e}")))?;

        tracing::debug!("Lit Chipotle raw response: {}", resp_json);

        // Check has_error flag
        if resp_json["has_error"].as_bool() == Some(true) {
            let logs = resp_json["logs"].as_str().unwrap_or("no logs");
            return Err(SigningError::Rejected(format!("Lit Action error: {logs}")));
        }

        // The Lit Action sets its response via Lit.Actions.setResponse()
        // Chipotle may return it as a JSON string (old) or a nested object (new).
        let action_response: serde_json::Value = match &resp_json["response"] {
            serde_json::Value::String(s) => serde_json::from_str(s)
                .map_err(|e| SigningError::Internal(
                    format!("Failed to parse Lit Action response string: {e}")
                ))?,
            serde_json::Value::Object(_) => resp_json["response"].clone(),
            other => {
                return Err(SigningError::Internal(format!(
                    "Unexpected 'response' field type in Lit result: {other}"
                )));
            }
        };

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
            signer_address: Some(self.pkp_address.clone()),
            reason: action_response["message"].as_str().map(|s| s.to_string()),
        })
    }
}