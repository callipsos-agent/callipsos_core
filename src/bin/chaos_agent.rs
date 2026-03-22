use std::env;

use colour::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use rig::completion::Prompt;
use rig::client::{ProviderClient, CompletionClient};

use callipsos_core::policy::types::{Action, Decision, EngineReason};
use callipsos_core::signing::SigningResult;



// ── Types for API communication ─────────────────────────────

// ── Request types (constructed manually, plain structs) ─────

/// What we send to POST /api/v1/validate
#[derive(Debug, Serialize)]
struct ValidateRequest {
    user_id: Uuid,
    target_protocol: String,
    action: Action,
    asset: String,
    amount_usd: String,
    target_address: String,
    context: ValidateContext,
}

/// Portfolio context sent with each validation request.
/// In production, this comes from on-chain data. For the demo, we hardcode it.
#[derive(Debug, Clone, Serialize)]
struct ValidateContext {
    portfolio_total_usd: String,
    current_protocol_exposure_usd: String,
    current_asset_exposure_usd: String,
    daily_spend_usd: String,
    audited_protocols: Vec<String>,
    protocol_risk_score: Option<f64>,
    protocol_utilization_pct: Option<f64>,
    protocol_tvl_usd: Option<String>,
}

/// Request body for POST /api/v1/policies
#[derive(Debug, Serialize)]
struct CreatePolicyRequest {
    user_id: Uuid,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rules: Option<serde_json::Value>,
}

// ── Response types (use real types for type safety) ─────────

/// Response from POST /api/v1/users (only need the id)
#[derive(Debug, Deserialize)]
struct CreateUserResponse {
    id: Uuid,
}

/// The response from POST /api/v1/validate
/// Mirrors the flattened PolicyVerdict + signing field.
#[derive(Debug, Deserialize)]
struct ValidateResponse {
    decision: Decision,
    results: Vec<RuleResultResponse>,
    engine_reason: Option<EngineReason>,
    signing: Option<SigningResult>,
}

/// Simplified RuleResult for display purposes.
/// Using a local struct because RuleResult has private fields
/// and we only need rule name, outcome, and message for printing.
#[derive(Debug, Deserialize)]
struct RuleResultResponse {
    rule: String,
    outcome: String,
    violation: Option<serde_json::Value>,
    message: String,
}

// ── Chaos Agent Error ───────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
enum ChaosAgentError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}


// ── Rig Tool: ValidateTransaction ───────────────────────────

/// The tool the chaos agent uses to submit transactions to Callipsos for validation.
/// It calls POST /api/v1/validate and returns a human-readable result.
struct ValidateTool {
    api_url: String,
    user_id: Uuid,
    http_client: Client,
    /// Tracks cumulative daily spend across calls so context stays accurate
    daily_spend_so_far: std::sync::Arc<tokio::sync::Mutex<f64>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ValidateToolArgs {
    /// The target DeFi protocol (e.g. "aave-v3", "moonwell", "shady-yield", "uniswap")
    target_protocol: String,
    /// The action to perform: "supply", "borrow"
    /// NOTE! "swap", "transfer", "withdraw", or "stake" TBA post mvp
    action: String,
    /// The asset symbol (e.g. "USDC", "ETH")
    asset: String,
    /// The amount in USD as a string (e.g. "50.00", "5000.00")
    amount_usd: String,
    /// The target contract address (use "0x1234" for demo purposes)
    target_address: String,
}

impl rig::tool::Tool for ValidateTool {
    const NAME: &'static str = "validate_transaction";

    type Error = ChaosAgentError;
    type Args = ValidateToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> rig::completion::request::ToolDefinition  {
        rig::completion::request::ToolDefinition {
            name: "validate_transaction".to_string(),
            description: "Submit a DeFi transaction to Callipsos for policy validation. \
                Returns whether the transaction was APPROVED or BLOCKED, with reasons for each rule check. \
                Use this to attempt yield strategies. If blocked, read the reasons and try a different approach."
                .to_string(),
            parameters: serde_json::to_value(schemars::schema_for!(ValidateToolArgs))
                .unwrap_or_default(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<String, Self::Error> {
        let mut daily_spend = self.daily_spend_so_far.lock().await;

        // Parse the action string into the Action enum
        let action: Action = serde_json::from_value(
            serde_json::Value::String(args.action.clone()),
        )
        .map_err(|_| ChaosAgentError::Other(format!("Invalid action '{}'", args.action)))?;

        // Build the context — portfolio state for the policy engine
        let context = ValidateContext {
            portfolio_total_usd: "10000.00".to_string(),
            current_protocol_exposure_usd: "0.00".to_string(),
            current_asset_exposure_usd: "0.00".to_string(),
            daily_spend_usd: format!("{:.2}", *daily_spend),
            audited_protocols: vec![
                "aave-v3".to_string(),
                "moonwell".to_string(),
            ],
            protocol_risk_score: Some(0.90),
            protocol_utilization_pct: Some(0.50),
            protocol_tvl_usd: Some("500000000".to_string()),
        };

        let request = ValidateRequest {
            user_id: self.user_id,
            target_protocol: args.target_protocol.clone(),
            action,
            asset: args.asset.clone(),
            amount_usd: args.amount_usd.clone(),
            target_address: args.target_address.clone(),
            context,
        };

        // Print the attempt
        dark_grey_ln!(
            "   → POST /validate: {} {} {} to {}",
            args.amount_usd, args.asset, args.action, args.target_protocol
        );

        // Call the API
        let response = self
            .http_client
            .post(format!("{}/api/v1/validate", self.api_url))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Ok(format!("API ERROR ({}): {}", status, body));
        }

        let verdict: ValidateResponse = response.json().await?;

        // Update daily spend if approved
        if verdict.decision == Decision::Approved {
            if let Ok(amount) = args.amount_usd.parse::<f64>() {
                *daily_spend += amount;
            }
        }

        // Collect failed rules for display
        let failed_rules: Vec<&RuleResultResponse> = verdict
            .results
            .iter()
            .filter(|r| r.outcome != "pass")
            .collect();

        // Print colored result
        if verdict.decision == Decision::Approved {
            let sig_info = verdict
                .signing
                .as_ref()
                .and_then(|s| s.signature.as_ref())
                .map(|sig| format!(" — Signed: {}", sig))
                .unwrap_or_default();
            green_ln_bold!("   ✅ APPROVED{}", sig_info);
        } else {
            red_ln_bold!("   ❌ BLOCKED");
            for rule in &failed_rules {
                yellow_ln!("   ├── {}", rule.message);
            }
        }

        // Build a string response for the agent to reason about
        let mut result = format!("DECISION: {:?}\n", verdict.decision);

        if let Some(ref reason) = verdict.engine_reason {
            result.push_str(&format!("ENGINE REASON: {}\n", reason));
        }

        for rule_result in &verdict.results {
            let icon = match rule_result.outcome.as_str() {
                "pass" => "✓",
                "fail" => "✗",
                _ => "?",
            };
            result.push_str(&format!(
                "{} [{}] {}\n",
                icon, rule_result.rule, rule_result.message
            ));
        }

        if let Some(ref signing) = verdict.signing {
            if signing.signed {
                result.push_str(&format!(
                    "\nSIGNED by Lit PKP: {}\n",
                    signing.signature.as_deref().unwrap_or("(no sig)")
                ));
            }
        }

        Ok(result)
    }
}

// ── Rig Tool: SetPolicy (NLP → Policy Rules) ───────────────

/// The tool the agent uses to set safety policies from natural language.
/// Claude extracts structured rule parameters from the user's preferences.
struct SetPolicyTool {
    api_url: String,
    user_id: Uuid,
    http_client: Client,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetPolicyToolArgs {
    /// Human-friendly name for this policy (e.g. "my safety rules")
    name: String,
    /// Max single transaction amount in USD. Omit to skip this rule.
    max_transaction_amount: Option<f64>,
    /// Max daily total spend in USD. Omit to skip this rule.
    max_daily_spend: Option<f64>,
    /// Max percentage of portfolio in one protocol (e.g. 10 for 10%). Omit to skip.
    max_percent_per_protocol: Option<u32>,
    /// Max percentage of portfolio in one asset (e.g. 30 for 30%). Omit to skip.
    max_percent_per_asset: Option<u32>,
    /// Only allow audited protocols. Set true to enable.
    only_audited: Option<bool>,
    /// Actions to block (valid values: "borrow", "swap", "transfer", "supply", "withdraw", "stake")
    blocked_actions: Option<Vec<String>>,
    /// Minimum protocol risk score from 0.0 to 1.0 (e.g. 0.80). Omit to skip.
    min_risk_score: Option<f64>,
    /// Max protocol utilization percentage (e.g. 80 for 80%). Omit to skip.
    max_utilization: Option<u32>,
    /// Minimum protocol TVL in USD (e.g. 50000000). Omit to skip.
    min_tvl: Option<f64>,
}

impl SetPolicyTool {
    /// Builds the rules JSON array from the tool args.
    /// Validates inputs and matches the exact serde serialization format of Vec<PolicyRule>.
    fn build_rules_json(args: &SetPolicyToolArgs) -> Result<serde_json::Value, String> {
        let mut rules = Vec::new();

        if let Some(amount) = args.max_transaction_amount {
            if amount < 0.0 {
                return Err("max_transaction_amount cannot be negative".into());
            }
            rules.push(json!({"MaxTransactionAmount": format!("{:.2}", amount)}));
        }
        if let Some(daily) = args.max_daily_spend {
            if daily < 0.0 {
                return Err("max_daily_spend cannot be negative".into());
            }
            rules.push(json!({"MaxDailySpend": format!("{:.2}", daily)}));
        }
        if let Some(pct) = args.max_percent_per_protocol {
            if pct > 100 {
                return Err(format!("max_percent_per_protocol cannot exceed 100%, got {}%", pct));
            }
            rules.push(json!({"MaxPercentPerProtocol": pct * 100}));
        }
        if let Some(pct) = args.max_percent_per_asset {
            if pct > 100 {
                return Err(format!("max_percent_per_asset cannot exceed 100%, got {}%", pct));
            }
            rules.push(json!({"MaxPercentPerAsset": pct * 100}));
        }
        if args.only_audited == Some(true) {
            rules.push(json!("OnlyAuditedProtocols"));
        }
        if let Some(ref actions) = args.blocked_actions {
            let valid_actions = ["supply", "borrow", "swap", "transfer", "withdraw", "stake"];
            let mut normalized = Vec::new();
            for action in actions {
                let lowercase = action.to_lowercase();
                if !valid_actions.contains(&lowercase.as_str()) {
                    return Err(format!(
                        "Invalid action '{}'. Valid actions: {}",
                        action,
                        valid_actions.join(", ")
                    ));
                }
                normalized.push(lowercase);
            }
            rules.push(json!({"BlockedActions": normalized}));
        }
        if let Some(score) = args.min_risk_score {
            if score < 0.0 || score > 1.0 {
                return Err(format!("min_risk_score must be between 0.0 and 1.0, got {}", score));
            }
            rules.push(json!({"MinRiskScore": format!("{:.2}", score)}));
        }
        if let Some(pct) = args.max_utilization {
            if pct > 100 {
                return Err(format!("max_utilization cannot exceed 100%, got {}%", pct));
            }
            rules.push(json!({"MaxProtocolUtilization": pct * 100}));
        }
        if let Some(tvl) = args.min_tvl {
            if tvl < 0.0 {
                return Err("min_tvl cannot be negative".into());
            }
            rules.push(json!({"MinProtocolTvl": format!("{:.2}", tvl)}));
        }

        Ok(json!(rules))
    }
}

impl rig::tool::Tool for SetPolicyTool {
    const NAME: &'static str = "set_policy";

    type Error = ChaosAgentError;
    type Args = SetPolicyToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> rig::completion::request::ToolDefinition {
        rig::completion::request::ToolDefinition {
            name: "set_policy".to_string(),
            description: "Set a safety policy for the user's wallet. \
                Translate the user's natural language safety preferences into specific policy rules. \
                Each parameter is optional — only include rules the user mentions. \
                Example: if user says 'max $200 per transaction and only audited protocols', \
                set max_transaction_amount=200 and only_audited=true, leave everything else null."
                .to_string(),
            parameters: serde_json::to_value(schemars::schema_for!(SetPolicyToolArgs))
                .unwrap_or_default(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<String, Self::Error> {
        let rules_json = match Self::build_rules_json(&args) {
            Ok(json) => json,
            Err(msg) => return Ok(format!("POLICY ERROR: {}", msg)),
        };

        let rule_count = rules_json.as_array().map(|a| a.len()).unwrap_or(0);

        dark_grey_ln!("   → Setting policy: {} ({} rules)", args.name, rule_count);

        let body = CreatePolicyRequest {
            user_id: self.user_id,
            name: args.name.clone(),
            preset: None,
            rules: Some(rules_json),
        };

        let response = self
            .http_client
            .post(format!("{}/api/v1/policies", self.api_url))
            .json(&body)
            .send()
            .await
            .map_err(ChaosAgentError::Http)?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return Ok(format!("POLICY ERROR ({}): {}", status, body_text));
        }

        green_ln_bold!("   ✅ Policy '{}' created with {} rules", args.name, rule_count);

        let mut result = format!("Policy '{}' created successfully with {} rules:\n", args.name, rule_count);
        if let Some(amt) = args.max_transaction_amount {
            result.push_str(&format!("  - Max transaction: ${}\n", amt));
        }
        if let Some(daily) = args.max_daily_spend {
            result.push_str(&format!("  - Max daily spend: ${}\n", daily));
        }
        if let Some(pct) = args.max_percent_per_protocol {
            result.push_str(&format!("  - Max {}% per protocol\n", pct));
        }
        if let Some(pct) = args.max_percent_per_asset {
            result.push_str(&format!("  - Max {}% per asset\n", pct));
        }
        if args.only_audited == Some(true) {
            result.push_str("  - Only audited protocols\n");
        }
        if let Some(ref actions) = args.blocked_actions {
            result.push_str(&format!("  - Blocked actions: {}\n", actions.join(", ")));
        }
        if let Some(score) = args.min_risk_score {
            result.push_str(&format!("  - Min risk score: {}\n", score));
        }
        if let Some(pct) = args.max_utilization {
            result.push_str(&format!("  - Max {}% utilization\n", pct));
        }
        if let Some(tvl) = args.min_tvl {
            result.push_str(&format!("  - Min TVL: ${}\n", tvl));
        }

        Ok(result)
    }
}

// ── Demo setup helpers ──────────────────────────────────────

/// Creates a test user via POST /api/v1/users
async fn create_user(client: &Client, api_url: &str) -> anyhow::Result<Uuid> {
    let response = client
        .post(format!("{}/api/v1/users", api_url))
        .json(&serde_json::json!({}))
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to create user: {}", response.status());
    }

    let user: CreateUserResponse = response.json().await?;
    Ok(user.id)
}

/// Creates the safety_first policy for a user via POST /api/v1/policies.
/// Uses the preset path — the server serializes the rules correctly.
async fn create_policy(
    client: &Client,
    api_url: &str,
    user_id: Uuid,
) -> anyhow::Result<()> {
    let body = CreatePolicyRequest {
        user_id,
        name: "safety_first".to_string(),
        preset: Some("safety_first".to_string()),
        rules: None,
    };

    let response = client
        .post(format!("{}/api/v1/policies", api_url))
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to create policy: {} — {}", status, body);
    }

    Ok(())
}

// ── Main ────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let api_url = env::var("CALLIPSOS_API_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());
    let anthropic_api_key = env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY must be set");

    // ── Banner ──────────────────────────────────────────────
    println!();
    cyan_ln_bold!(r#"
   ██████╗ █████╗ ██╗     ██╗     ██╗██████╗ ███████╗ ██████╗ ███████╗
  ██╔════╝██╔══██╗██║     ██║     ██║██╔══██╗██╔════╝██╔═══██╗██╔════╝
  ██║     ███████║██║     ██║     ██║██████╔╝███████╗██║   ██║███████╗
  ██║     ██╔══██║██║     ██║     ██║██╔═══╝ ╚════██║██║   ██║╚════██║
  ╚██████╗██║  ██║███████╗███████╗██║██║     ███████║╚██████╔╝███████║
   ╚═════╝╚═╝  ╚═╝╚══════╝╚══════╝╚═╝╚═╝     ╚══════╝ ╚═════╝ ╚══════╝
    "#);
    dark_grey_ln!("   Safety layer for autonomous AI agents in DeFi");
    dark_grey_ln!("{}", "━".repeat(72));
    print!("   ");
    print_bold!("Portfolio:");
    println!(" $1,000 USDC on Base");
    print!("   ");
    print_bold!("Agent:");
    println!(" Rig + Claude Sonnet");
    print!("   ");
    print_bold!("Signing:");
    println!(" Lit Protocol (Chipotle TEE)");
    dark_grey_ln!("{}", "━".repeat(72));

    // ── Setup ───────────────────────────────────────────────
    let http_client = Client::new();

    dark_grey_ln!("\nSetting up demo environment...");

    let user_id = create_user(&http_client, &api_url).await?;
    green_ln!("   ✓ User created: {}", user_id);

    // create_policy(&http_client, &api_url, user_id).await?;
    // green_ln!("   ✓ Policy applied: safety_first");

    dark_grey_ln!("{}", "━".repeat(60));

    // ── Build the Rig Agent ─────────────────────────────────
   let anthropic_client = rig::providers::anthropic::Client::from_env();

    let daily_spend = std::sync::Arc::new(tokio::sync::Mutex::new(0.0));

    let validate_tool = ValidateTool {
        api_url: api_url.clone(),
        user_id,
        http_client: http_client.clone(),
        daily_spend_so_far: daily_spend.clone(),
    };

    let set_policy_tool = SetPolicyTool {
        api_url: api_url.clone(),
        user_id,
        http_client: http_client.clone(),
    };

    let agent = anthropic_client
        .agent("claude-sonnet-4-5-20250929")
        .preamble(
            "You are the Callipsos DeFi agent. You help users safely invest in DeFi on Base.\n\n\
            The user has connected their wallet with $1,000 USDC.\n\n\
            Your workflow:\n\
            1. FIRST: Read the user's safety preferences carefully.\n\
            2. You MUST show your NLP reasoning BEFORE calling set_policy. This is mandatory.\n\
               Format it exactly like this:\n\n\
               **Interpreting your preferences:**\n\
               - You said \"[exact quote]\" → I'm setting [rule] to [value] because [reason]\n\
               - You said \"[exact quote]\" → I'm setting [rule] to [value] because [reason]\n\
               - You didn't mention [X], so I'm adding a sensible default: [rule] = [value]\n\n\
               Show EVERY rule you're about to create and why. This transparency is the whole point.\n\
            3. Then call set_policy with the extracted parameters.\n\
            4. After the policy is set, start finding yield opportunities.\n\
            5. Use validate_transaction to attempt transactions. Try a mix — some aggressive \
               ones that will get blocked and some conservative ones that should pass.\n\
               - BUDGET TIP: If the user has a daily spend limit, don't use it all on the first \
                 transaction. Spread it across multiple smaller transactions to diversify.\n\
                - If the user says 'low risk', 'safe', or 'conservative', ALWAYS block \
                borrowing and transfers in addition to other rules.\n\
            6. When transactions are blocked, briefly explain why in plain language that is understandable to the user.\n\
            7. Make at least 5 transaction attempts after setting the policy.\n\
            8. End with a clear summary that MUST include:\n\
               a. **How I interpreted your preferences** — show the exact mapping from \
                  your words to policy rules\n\
               b. What was approved and what was blocked\n\
               c. Total yield achieved\n\n\
            Available protocols:\n\
            - Aave V3 (audited, 4.2% APY on USDC supply)\n\
            - Moonwell (audited, 3.8% APY on USDC supply)\n\
            - ShadyYield (UNAUDITED, 15% APY)\n\
            - Uniswap (DEX for swaps)\n\n\
            Available actions: supply, borrow, swap, transfer, withdraw, stake\n\n\
            IMPORTANT: Do NOT ask questions. Act immediately on what the user provides.\
            IMPORTANT: You MUST print the NLP reasoning block before calling set_policy. No exceptions.
            Be conversational and very friendly! Explain what you're doing and why, in simple terms so the user understands, don't use complex DeFi terminologies.\
            "
        )
        .max_tokens(4096)
        .tool(validate_tool)
        .tool(set_policy_tool)
        .default_max_turns(20)
        .build();

    // ── Run the Agent ───────────────────────────────────────
    yellow_ln_bold!("\n🔥 Chaos Agent activated. Attempting to maximize yields...\n");

    let response = agent
        .prompt("Hi! I just connected my wallet with $1,000 USDC. Here are my rules: only spend up to $200 per day, only use audited protocols, and I want only low risk yields. Keep my money safe! I would rather have low yields than risk losing everything. Show me what you got!!")
        .await;

    match response {
        Ok(output) => {
            dark_grey_ln!("\n{}", "━".repeat(60));
            cyan_ln_bold!("🤖 Agent's Summary:");
            println!("{}", output);
        }
        Err(e) => {
            red_ln_bold!("\nAgent error: {}", e);
        }
    }

    // ── Final Summary ───────────────────────────────────────
    dark_grey_ln!("\n{}", "━".repeat(60));
    green_ln_bold!("📊 DEMO COMPLETE");
    dark_grey_ln!("   Callipsos validated every transaction against user-defined policy.");
    dark_grey_ln!("   The agent tried everything — the safety layer held.");
    cyan_ln!("   Always watching. Always protecting.");
    dark_grey_ln!("{}\n", "━".repeat(60));

    Ok(())
}