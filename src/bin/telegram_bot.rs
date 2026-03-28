// src/bin/telegram_bot.rs
//
// Callipsos Telegram Bot — conversational DeFi safety agent.
// Runs as a separate binary alongside the main API server.
// The server must be running (cargo run --bin callipsos_core) for the bot to work.
//
// Required env vars:
//   TELOXIDE_TOKEN      — Telegram bot token from BotFather
//   ANTHROPIC_API_KEY   — Fallback LLM key (used for all users in MVP)
//   DATABASE_URL        — Same PostgreSQL instance as the API server
//   CALLIPSOS_API_URL   — API server base URL (default: http://127.0.0.1:3000)
//
// Run with: cargo run --bin telegram_bot

// ═══════════════════════════════════════════════════════════════
// Section 1: Imports
// ═══════════════════════════════════════════════════════════════

use std::sync::Arc;

use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;
use uuid::Uuid;
use rig::client::{ProviderClient, CompletionClient};
use rig::completion::Chat;

use callipsos_core::db;
use callipsos_core::db::conversation::{
    ConversationMessage, ConversationRow, MessageRole,
};
use callipsos_core::db::user::User;
use callipsos_core::policy::types::{Action, Decision, EngineReason, UserId};
use callipsos_core::signing::SigningResult;

type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

// ═══════════════════════════════════════════════════════════════
// Section 2: Bot commands
// ═══════════════════════════════════════════════════════════════

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
enum Command {
    #[command(description = "Start or restart Callipsos")]
    Start,
    #[command(description = "View your active safety policies")]
    Policy,
    #[command(description = "Reset conversation and start fresh")]
    Reset,
    #[command(description = "Show available commands")]
    Help,
}

// ═══════════════════════════════════════════════════════════════
// Section 3: Shared state
// ═══════════════════════════════════════════════════════════════

/// Shared across all handlers via Arc. Holds everything needed
/// to process any incoming message without additional lookups.
#[derive(Clone)]
struct BotState {
    /// Database pool — same instance as the API server.
    /// Used for user lookup, conversation persistence, onboarding state.
    db: PgPool,
    /// HTTP client for calling the Callipsos API (validate, policies, users).
    http_client: HttpClient,
    /// Base URL of the running Callipsos API server.
    api_url: String,
}

// ═══════════════════════════════════════════════════════════════
// Section 4: API types (shared with chaos_agent.rs)
// ═══════════════════════════════════════════════════════════════
// These mirror the API request/response shapes. Duplicated from
// chaos_agent.rs because binary crates can't share private types.
// If this grows, extract to a shared module in the library crate.

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

#[derive(Debug, Serialize)]
struct CreatePolicyRequest {
    user_id: Uuid,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rules: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ValidateResponse {
    decision: Decision,
    results: Vec<RuleResultResponse>,
    engine_reason: Option<EngineReason>,
    signing: Option<SigningResult>,
}

#[derive(Debug, Deserialize)]
struct RuleResultResponse {
    rule: String,
    outcome: String,
    violation: Option<serde_json::Value>,
    message: String,
}

#[derive(Debug, Deserialize)]
struct CreateUserResponse {
    id: Uuid,
}

// ═══════════════════════════════════════════════════════════════
// Section 5: Rig tool error type
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, thiserror::Error)]
enum AgentError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

// ═══════════════════════════════════════════════════════════════
// Section 6: Rig tools (ValidateTool + SetPolicyTool)
//
// Adapted from src/bin/chaos_agent.rs. Changes from the original:
//   - Error type: AgentError instead of ChaosAgentError
//   - No colour:: terminal prints (output goes to Telegram, not stdout)
//   - portfolio_total_usd is user-supplied (stored on the tool), not hardcoded
//   - tracing::debug instead of println for server-side observability
// ═══════════════════════════════════════════════════════════════

// ── ValidateTool ────────────────────────────────────────────

struct ValidateTool {
    api_url: String,
    user_id: Uuid,
    http_client: HttpClient,
    /// User's stated portfolio size. Set from their first message
    /// (e.g. "I have $1000 USDC"). Defaults to "1000.00" if not stated.
    portfolio_total_usd: String,
    /// Tracks cumulative daily spend across calls so context stays accurate.
    daily_spend_so_far: Arc<tokio::sync::Mutex<f64>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ValidateToolArgs {
    /// The target DeFi protocol (e.g. "aave-v3", "moonwell", "shady-yield", "uniswap")
    target_protocol: String,
    /// The action to perform: "supply", "borrow", "swap", "transfer", "withdraw", or "stake"
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

    type Error = AgentError;
    type Args = ValidateToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> rig::completion::request::ToolDefinition {
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

        let action: Action = serde_json::from_value(
            serde_json::Value::String(args.action.clone()),
        )
        .map_err(|_| AgentError::Other(format!("Invalid action '{}'", args.action)))?;

        let context = ValidateContext {
            portfolio_total_usd: self.portfolio_total_usd.clone(),
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

        tracing::debug!(
            "validate_transaction: {} {} {} to {}",
            args.amount_usd, args.asset, args.action, args.target_protocol
        );

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

        if verdict.decision == Decision::Approved {
            if let Ok(amount) = args.amount_usd.parse::<f64>() {
                *daily_spend += amount;
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

// ── SetPolicyTool ───────────────────────────────────────────

struct SetPolicyTool {
    api_url: String,
    user_id: Uuid,
    http_client: HttpClient,
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

    type Error = AgentError;
    type Args = SetPolicyToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> rig::completion::request::ToolDefinition {
        rig::completion::request::ToolDefinition {
            name: "set_policy".to_string(),
            description: "Set a safety policy for the user's wallet. \
                Translate the user's natural language safety preferences into specific policy rules. \
                For any rule the user explicitly mentions, use their stated value. \
                For rules the user does NOT mention, apply sensible safety-first defaults \
                scaled to their portfolio size: \
                - max_transaction_amount: ~10% of portfolio \
                - max_daily_spend: ~10-20% of portfolio \
                - max_percent_per_protocol: 25 \
                - max_percent_per_asset: 50 \
                - only_audited: true \
                - blocked_actions: [\"transfer\"] \
                - min_risk_score: 0.70 \
                - max_utilization: 85 \
                - min_tvl: 10000000 \
                ALWAYS fill in defaults for rules the user didn't mention. \
                After setting the policy, list which rules came from the user and which are defaults."
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

        tracing::debug!("set_policy: {} ({} rules)", args.name, rule_count);

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
            .map_err(AgentError::Http)?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return Ok(format!("POLICY ERROR ({}): {}", status, body_text));
        }

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

// ═══════════════════════════════════════════════════════════════
// Section 7: Command handlers
// ═══════════════════════════════════════════════════════════════

/// /start — Create or find user, start a fresh conversation, hand off to agent.
async fn handle_start(bot: Bot, msg: Message, state: BotState) -> HandlerResult {
    let telegram_id = msg.from.as_ref().map(|u| u.id.0 as i64).ok_or("No user ID")?;

    // Find existing user or create a new one
    let user = match User::find_by_telegram_id(&state.db, telegram_id).await? {
        Some(user) => {
            tracing::info!("Returning user: {} (telegram: {})", user.id, telegram_id);
            user
        }
        None => {
            let user = User::create(&state.db, Some(telegram_id)).await?;
            tracing::info!("New user created: {} (telegram: {})", user.id, telegram_id);
            user
        }
    };

    // Start a fresh conversation (deactivates any previous one)
    ConversationRow::reset(&state.db, user.id).await?;

    // Welcome message — the agent takes it from here
    let welcome = "\
    Hey! I'm your Callipsos agent — I help you earn yields on your crypto safely.\n\n\
    Think of me as a guard dog for your funds. I find opportunities, but nothing \
    moves without passing my safety checks first.\n\n\
    ⚠️ Demo mode: No real funds are used. Everything is simulated so you can \
    see how I protect your portfolio.\n\n\
    To get started, just tell me:\n\
    • How much you have (e.g. \"I have $1,000 USDC\")\n\
    • Your risk preferences (e.g. \"keep it safe\" or \"I want maximum yields\")\n\n\
    For example:\"I have $500 USDC. Only use safe, audited protocols. \
    Don't risk more than $100 per day.\"\n\n\
    I'll set up your safety policies and start scouting yields for you.";

    bot.send_message(msg.chat.id, welcome)
        // .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;

    // Store the welcome as the first assistant message in the conversation
    let welcome_msg = ConversationMessage {
        role: MessageRole::Assistant,
        content: welcome.to_string(),
        tool_calls: vec![],
    };
    ConversationRow::append_message(&state.db, user.id, &welcome_msg).await?;

    Ok(())
}

/// /policy — Show the user's active policies in plain language.
async fn handle_policy(bot: Bot, msg: Message, state: BotState) -> HandlerResult {
    let telegram_id = msg.from.as_ref().map(|u| u.id.0 as i64).ok_or("No user ID")?;

    let user = match User::find_by_telegram_id(&state.db, telegram_id).await? {
        Some(user) => user,
        None => {
            bot.send_message(msg.chat.id, "You haven't started yet. Send /start first.")
                .await?;
            return Ok(());
        }
    };

    let policies = callipsos_core::db::policy::PolicyRow::find_active_by_user(
        &state.db, user.id,
    )
    .await?;

    if policies.is_empty() {
        bot.send_message(
            msg.chat.id,
            "You don't have any active policies yet. Tell me your risk preferences and I'll set them up.",
        )
        .await?;
        return Ok(());
    }

    // Build a plain-language summary of all active rules
    let mut summary = String::from("🛡️ *Your active safety policies:*\n\n");

    for policy in &policies {
        summary.push_str(&format!("*{}*\n", policy.name));

        // Deserialize rules and describe each one
        if let Ok(rules) = serde_json::from_value::<Vec<serde_json::Value>>(
            policy.rules_json.clone(),
        ) {
            for rule in &rules {
                let description = describe_rule(rule);
                summary.push_str(&format!("  • {}\n", description));
            }
        }
        summary.push('\n');
    }

    bot.send_message(msg.chat.id, &summary)
        // .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;

    Ok(())
}

/// /reset — Clear conversation history and deactivate policies. Fresh start.
async fn handle_reset(bot: Bot, msg: Message, state: BotState) -> HandlerResult {
    let telegram_id = msg.from.as_ref().map(|u| u.id.0 as i64).ok_or("No user ID")?;

    let user = match User::find_by_telegram_id(&state.db, telegram_id).await? {
        Some(user) => user,
        None => {
            bot.send_message(msg.chat.id, "You haven't started yet. Send /start first.")
                .await?;
            return Ok(());
        }
    };

    // Deactivate all policies
    let policies = callipsos_core::db::policy::PolicyRow::find_active_by_user(
        &state.db, user.id,
    )
    .await?;
    for policy in &policies {
        callipsos_core::db::policy::PolicyRow::soft_delete(&state.db, policy.id).await?;
    }

    // Reset conversation (deactivates old, creates fresh)
    ConversationRow::reset(&state.db, user.id).await?;

    // Reset onboarded flag
    sqlx::query!(
        "UPDATE users SET onboarded = FALSE, updated_at = NOW() WHERE id = $1",
        user.id.0,
    )
    .execute(&state.db)
    .await?;

    bot.send_message(
        msg.chat.id,
        "Fresh start. Your policies and conversation history have been cleared.\n\nSend /start to begin again.",
    )
    .await?;

    Ok(())
}

/// /help — Show available commands.
async fn handle_help(bot: Bot, msg: Message, _state: BotState) -> HandlerResult {
    let help_text = "\
    Callipsos Agent — Commands\n\n\
    /start — Start or restart Callipsos\n\
    /policy — View your active safety policies\n\
    /reset — Clear everything and start fresh\n\
    /help — Show this message\n\n\
    Or just chat with me naturally. Tell me about your portfolio and risk \
    preferences, and I'll take care of the rest.\n\n\
    If you enjoy using Callipsos, consider supporting compute costs \
    to keep the agent running.";

    bot.send_message(msg.chat.id, help_text)
        // .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await?;

    Ok(())
}

// ── Helper: describe a policy rule in plain language ─────────

fn describe_rule(rule: &serde_json::Value) -> String {
    match rule {
        serde_json::Value::String(s) if s == "OnlyAuditedProtocols" => {
            "Only audited protocols allowed".to_string()
        }
        serde_json::Value::Object(map) => {
            if let Some(v) = map.get("MaxTransactionAmount") {
                format!("Max ${} per transaction", v.as_str().unwrap_or("?"))
            } else if let Some(v) = map.get("MaxDailySpend") {
                format!("Max ${} daily spend", v.as_str().unwrap_or("?"))
            } else if let Some(v) = map.get("MaxPercentPerProtocol") {
                let bps = v.as_u64().unwrap_or(0);
                format!("Max {}% in any single protocol", bps / 100)
            } else if let Some(v) = map.get("MaxPercentPerAsset") {
                let bps = v.as_u64().unwrap_or(0);
                format!("Max {}% in any single asset", bps / 100)
            } else if let Some(v) = map.get("BlockedActions") {
                let actions = v
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                format!("Blocked actions: {}", actions)
            } else if let Some(v) = map.get("MinRiskScore") {
                format!("Min risk score: {}", v.as_str().unwrap_or("?"))
            } else if let Some(v) = map.get("MaxProtocolUtilization") {
                let bps = v.as_u64().unwrap_or(0);
                format!("Max {}% protocol utilization", bps / 100)
            } else if let Some(v) = map.get("MinProtocolTvl") {
                format!("Min protocol TVL: ${}", v.as_str().unwrap_or("?"))
            } else if let Some(v) = map.get("AllowedProtocols") {
                let protocols = v
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                format!("Allowed protocols: {}", protocols)
            } else {
                format!("{}", rule)
            }
        }
        _ => format!("{}", rule),
    }
}

// ═══════════════════════════════════════════════════════════════
// Section 8: Free-text message handler (the agent)
//
// Every non-command text message flows through here. Instead of
// using Rig's built-in multi-turn .chat() which runs silently,
// we run the agent loop manually so we can send Telegram messages
// between each tool call. The user sees progress in real time.
// ═══════════════════════════════════════════════════════════════

use rig::completion::Completion;
use rig::message::{AssistantContent, UserContent};
use rig::one_or_many::OneOrMany;

async fn handle_message(bot: Bot, msg: Message, state: BotState) -> HandlerResult {
    let text = match msg.text() {
        Some(t) => t.to_string(),
        None => return Ok(()),
    };

    let telegram_id = msg.from.as_ref().map(|u| u.id.0 as i64).ok_or("No user ID")?;

    // 1. Find user
    let user = match User::find_by_telegram_id(&state.db, telegram_id).await? {
        Some(user) => user,
        None => {
            bot.send_message(msg.chat.id, "I don't know you yet. Send /start to get started.")
                .await?;
            return Ok(());
        }
    };

    // 2. Ensure active conversation exists
    if ConversationRow::find_active(&state.db, user.id).await?.is_none() {
        bot.send_message(msg.chat.id, "No active session. Send /start to begin.")
            .await?;
        return Ok(());
    }

    // 3. Typing indicator
    bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing)
        .await?;

    // 4. Store user message
    let user_msg = ConversationMessage {
        role: MessageRole::User,
        content: text.clone(),
        tool_calls: vec![],
    };
    ConversationRow::append_message(&state.db, user.id, &user_msg).await?;

    // 5. Load conversation history for Rig
    let conversation = ConversationRow::find_active(&state.db, user.id)
        .await?
        .ok_or("Conversation disappeared")?;

    let messages = conversation.messages()?;

    let history: Vec<rig::message::Message> = messages
        .iter()
        .take(messages.len().saturating_sub(1))
        .map(|m| match m.role {
            MessageRole::User => rig::message::Message::user(&m.content),
            MessageRole::Assistant => rig::message::Message::assistant(&m.content),
        })
        .collect();

    // 6. Build the Rig agent
    let anthropic_client = rig::providers::anthropic::Client::from_env();

    let daily_spend = Arc::new(tokio::sync::Mutex::new(0.0));

    let validate_tool = ValidateTool {
        api_url: state.api_url.clone(),
        user_id: *user.id,
        http_client: state.http_client.clone(),
        portfolio_total_usd: "1000.00".to_string(),
        daily_spend_so_far: daily_spend.clone(),
    };

    let set_policy_tool = SetPolicyTool {
        api_url: state.api_url.clone(),
        user_id: *user.id,
        http_client: state.http_client.clone(),
    };

    let agent = anthropic_client
        .agent("claude-sonnet-4-5-20250929")
        .preamble(AGENT_PREAMBLE)
        .max_tokens(4096)
        .tool(validate_tool)
        .tool(set_policy_tool)
        .build();

    // 7. Manual agent loop with Telegram updates between steps
    let max_turns = 20;
    let mut current_history = history;
    let mut current_prompt = text.clone();
    let mut final_response = String::new();

    for turn in 0..max_turns {
        // Refresh typing indicator each turn
        let _ = bot
            .send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing)
            .await;

        // Get completion for this turn
        let request_builder = match agent.completion(&current_prompt, current_history.clone()).await
        {
            Ok(builder) => builder,
            Err(e) => {
                tracing::error!("Agent completion error: {e}");
                final_response =
                    "Sorry, I ran into an issue. Please try again.".to_string();
                break;
            }
        };

        let response = match request_builder.send().await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("Agent send error: {e}");
                final_response =
                    "Sorry, I ran into an issue. Please try again.".to_string();
                break;
            }
        };

        // Check what the model returned
        let mut has_tool_calls = false;
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_results: Vec<(String, String, serde_json::Value, String)> = Vec::new();
        // (tool_call_id, tool_name, arguments, result_text)

        for content in response.choice.iter() {
            match content {
                AssistantContent::Text(t) => {
                    text_parts.push(t.text.clone());
                }
                AssistantContent::ToolCall(tc) => {
                    has_tool_calls = true;

                    let tool_name = &tc.function.name;
                    let tool_args = &tc.function.arguments;
                    let tool_id = &tc.id;

                    // Send progress update to Telegram
                    let progress = format_tool_progress(tool_name, tool_args);
                    bot.send_message(msg.chat.id, &progress).await?;

                    // Execute the tool
                    let result = execute_tool(
                        tool_name,
                        tool_args,
                        &state,
                        *user.id,
                        daily_spend.clone(),
                    )
                    .await;

                    // Send result update to Telegram
                    let result_msg = format_tool_result(tool_name, &result);
                    bot.send_message(msg.chat.id, &result_msg).await?;

                    tool_results.push((
                        tool_id.clone(),
                        tool_name.clone(),
                        tool_args.clone(),
                        result,
                    ));
                }
                _ => {} // Reasoning, Image, etc. — ignore for now
            }
        }

        // If the model produced text and no tool calls, we're done
        if !has_tool_calls {
            final_response = text_parts.join("\n");
            break;
        }

        // If there were tool calls, build the next turn's history:
        // 1. Add the assistant message (with tool calls) to history
        // 2. Add tool results as user messages
        // 3. Loop again with empty prompt (continuation)

        // Add assistant message to history
        current_history.push(rig::message::Message::Assistant {
            id: response.message_id.clone(),
            content: response.choice.clone(),
        });

        // Add tool results as user messages
        for (tool_id, _tool_name, _args, result_text) in &tool_results {
            current_history.push(rig::message::Message::User {
                content: OneOrMany::one(UserContent::tool_result(
                    tool_id.clone(),
                    OneOrMany::one(rig::message::ToolResultContent::text(result_text)),
                )),
            });
        }

        // Next turn: empty prompt since the context is in history
        current_prompt = "Continue.".to_string();

        tracing::debug!("Agent turn {}/{}: {} tool calls", turn + 1, max_turns, tool_results.len());

        // If we've hit max turns, the last text output is the response
        if turn == max_turns - 1 {
            final_response = if text_parts.is_empty() {
                "I've been working on your request but hit my limit. Here's what I've done so far — check /policy to see your current setup.".to_string()
            } else {
                text_parts.join("\n")
            };
        }
    }

    // 8. Store assistant response
    if !final_response.is_empty() {
        let assistant_msg = ConversationMessage {
            role: MessageRole::Assistant,
            content: final_response.clone(),
            tool_calls: vec![],
        };
        ConversationRow::append_message(&state.db, user.id, &assistant_msg).await?;
    }

    // 9. Trim if needed
    if let Some(conv) = ConversationRow::find_active(&state.db, user.id).await? {
        if conv.message_count() > 40 {
            ConversationRow::trim_to_recent(&state.db, user.id, 40).await?;
        }
    }

    // 10. Send final response
    if !final_response.is_empty() {
        for chunk in split_telegram_message(&final_response) {
            bot.send_message(msg.chat.id, &chunk).await?;
        }
    }

    Ok(())
}

// ── Tool execution dispatcher ───────────────────────────────

async fn execute_tool(
    tool_name: &str,
    args: &serde_json::Value,
    state: &BotState,
    user_id: Uuid,
    daily_spend: Arc<tokio::sync::Mutex<f64>>,
) -> String {
    match tool_name {
        "validate_transaction" => {
            let parsed: Result<ValidateToolArgs, _> = serde_json::from_value(args.clone());
            match parsed {
                Ok(tool_args) => {
                    tracing::info!(
                        "validate_transaction: {} {} {} to {}",
                        tool_args.amount_usd, tool_args.asset, tool_args.action, tool_args.target_protocol
                    );
                    let tool = ValidateTool {
                        api_url: state.api_url.clone(),
                        user_id,
                        http_client: state.http_client.clone(),
                        portfolio_total_usd: "1000.00".to_string(),
                        daily_spend_so_far: daily_spend,
                    };
                    match rig::tool::Tool::call(&tool, tool_args).await {
                        Ok(result) => {
                            tracing::info!("validate_transaction result:\n{}", result);
                            result
                        }
                        Err(e) => {
                            tracing::error!("validate_transaction error: {e}");
                            format!("Tool error: {e}")
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("validate_transaction invalid args: {e}");
                    format!("Invalid tool arguments: {e}")
                }
            }
        }
        "set_policy" => {
            let parsed: Result<SetPolicyToolArgs, _> = serde_json::from_value(args.clone());
            match parsed {
                Ok(tool_args) => {
                    tracing::info!("set_policy: {}", tool_args.name);
                    let tool = SetPolicyTool {
                        api_url: state.api_url.clone(),
                        user_id,
                        http_client: state.http_client.clone(),
                    };
                    match rig::tool::Tool::call(&tool, tool_args).await {
                        Ok(result) => {
                            tracing::info!("set_policy result:\n{}", result);
                            result
                        }
                        Err(e) => {
                            tracing::error!("set_policy error: {e}");
                            format!("Tool error: {e}")
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("set_policy invalid args: {e}");
                    format!("Invalid tool arguments: {e}")
                }
            }
        }
        _ => {
            tracing::warn!("Unknown tool called: {tool_name}");
            format!("Unknown tool: {tool_name}")
        }
    }
}
// ── Progress message formatters ─────────────────────────────

fn format_tool_progress(tool_name: &str, args: &serde_json::Value) -> String {
    match tool_name {
        "set_policy" => {
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("safety policy");
            format!("Setting up your safety policy: \"{}\"...", name)
        }
        "validate_transaction" => {
            let amount = args
                .get("amount_usd")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let asset = args
                .get("asset")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let protocol = args
                .get("target_protocol")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!(
                "Attempting: {} ${} {} on {}...",
                action, amount, asset, protocol
            )
        }
        _ => format!("Running {}...", tool_name),
    }
}

fn format_tool_result(tool_name: &str, result: &str) -> String {
    match tool_name {
        "set_policy" => {
            if result.starts_with("POLICY ERROR") {
                format!("Policy setup failed: {}", result)
            } else {
                "Policy created successfully.".to_string()
            }
        }
        "validate_transaction" => {
            if result.contains("DECISION: Approved") {
                let signed = if result.contains("SIGNED by Lit PKP") {
                    " (Signed by Lit Protocol)"
                } else {
                    ""
                };
                format!("APPROVED{}", signed)
            } else if result.contains("DECISION: Blocked") {
                // Extract the failed rules
                let failed: Vec<&str> = result
                    .lines()
                    .filter(|l| l.starts_with("✗"))
                    .collect();
                if failed.is_empty() {
                    "BLOCKED".to_string()
                } else {
                    let reasons = failed
                        .iter()
                        .map(|l| l.trim_start_matches("✗ "))
                        .collect::<Vec<_>>()
                        .join("\n  ");
                    format!("BLOCKED:\n  {}", reasons)
                }
            } else {
                result.lines().next().unwrap_or("Done").to_string()
            }
        }
        _ => "Done.".to_string(),
    }
}

// ── Agent preamble ──────────────────────────────────────────

const AGENT_PREAMBLE: &str = "\
You are the Callipsos DeFi agent, chatting with a user on Telegram. You help users \
safely earn yields on their crypto on Base.\n\n\
\
This is DEMO MODE — no real funds are used. The user states a portfolio amount \
and you simulate everything.\n\n\
\
Your workflow:\n\
1. FIRST: Read the user's message. If they state a portfolio amount and preferences, \
   proceed. If not, ask them to describe their portfolio and risk preferences.\n\
2. Show your reasoning BEFORE calling set_policy:\n\
   - You said \"[quote]\" -> setting [rule] to [value] because [reason]\n\
   - You didn't mention [X], so adding a safety default: [rule] = [value]\n\
   Show EVERY rule and why.\n\
3. For rules the user does NOT mention, apply safety-first defaults scaled to \
   their portfolio size:\n\
   - max_transaction_amount: ~10% of portfolio\n\
   - max_daily_spend: ~10-20% of portfolio\n\
   - max_percent_per_protocol: 25\n\
   - max_percent_per_asset: 50\n\
   - only_audited: true\n\
   - blocked_actions: [\"transfer\"]\n\
   - min_risk_score: 0.70\n\
   - max_utilization: 85\n\
   - min_tvl: 10000000\n\
   ALWAYS fill in defaults. After setting, tell the user which rules are theirs \
   and which are your safety defaults.\n\
4. After the policy is set, start finding yield opportunities.\n\
5. Use validate_transaction to attempt transactions. Try a mix — some that should \
   pass and some more aggressive ones to show the safety layer catching violations.\n\
   - Spread transactions across multiple smaller amounts to diversify.\n\
   - If the user said 'low risk', 'safe', or 'conservative', ALWAYS block \
     borrowing and transfers.\n\
6. When transactions are blocked, explain why in simple terms.\n\
7. Make at least 3-5 transaction attempts after setting the policy.\n\
8. End with a summary: what was approved, what was blocked, total yield.\n\n\
\
IMPORTANT: After each tool call, I will show the user the result immediately. \
Keep your text responses between tool calls SHORT — just explain what you're about \
to do next or what just happened. Save the detailed summary for the end.\n\n\
\
Available protocols:\n\
- Aave V3 (audited, ~4.2% APY on USDC supply)\n\
- Moonwell (audited, ~3.8% APY on USDC supply)\n\
- ShadyYield (UNAUDITED, 15% APY — use this to demo blocks)\n\
- Uniswap (DEX for swaps)\n\n\
\
Available actions: supply, borrow, swap, transfer, withdraw, stake\n\n\
\
Be friendly and conversational. Explain everything in simple terms, no DeFi jargon. \
You're a knowledgeable friend who guards their crypto, not a terminal.\n\
Do NOT ask unnecessary questions. Act on what the user provides.";

// ── Helper: split messages for Telegram's 4096 char limit ───

fn split_telegram_message(text: &str) -> Vec<String> {
    const MAX_LEN: usize = 4000;

    if text.len() <= MAX_LEN {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= MAX_LEN {
            chunks.push(remaining.to_string());
            break;
        }

        let split_at = remaining[..MAX_LEN].rfind('\n').unwrap_or(MAX_LEN);

        chunks.push(remaining[..split_at].to_string());
        remaining = &remaining[split_at..].trim_start();
    }

    chunks
}


// ═══════════════════════════════════════════════════════════════
// Section 9: Command router + main function
// ═══════════════════════════════════════════════════════════════

/// Routes /commands to their handlers.
async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    state: BotState,
) -> HandlerResult {
    match cmd {
        Command::Start => handle_start(bot, msg, state).await,
        Command::Policy => handle_policy(bot, msg, state).await,
        Command::Reset => handle_reset(bot, msg, state).await,
        Command::Help => handle_help(bot, msg, state).await,
    }
}

// ── Main ────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // ── Validate required env vars ──────────────────────────
    std::env::var("TELOXIDE_TOKEN")
        .expect("TELOXIDE_TOKEN must be set");
    std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY must be set");

    // ── Database (same instance as the API server) ──────────
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let pool = db::connect(&database_url).await?;
    db::migrate(&pool).await?;

    let api_url = std::env::var("CALLIPSOS_API_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());

    tracing::info!("Callipsos Telegram bot starting");
    tracing::info!("API server: {}", api_url);

    let state = BotState {
        db: pool,
        http_client: HttpClient::new(),
        api_url,
    };

    // ── Bot + dispatcher ────────────────────────────────────
    let bot = Bot::from_env();

    // Set bot commands in Telegram's menu
    bot.set_my_commands(Command::bot_commands()).await?;

    let handler = dptree::entry()
        .branch(
            Update::filter_message()
                .branch(
                    dptree::entry()
                        .filter_command::<Command>()
                        .endpoint(handle_command),
                )
                .branch(
                    dptree::entry()
                        .filter(|msg: Message| msg.text().is_some())
                        .endpoint(handle_message),
                ),
        );

    tracing::info!("Bot is live. Listening for messages...");

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .default_handler(|_| async {})
        .error_handler(Arc::new(|err| {
            Box::pin(async move {
                tracing::error!("Dispatcher error: {err}");
            })
        }))
        .build()
        .dispatch()
        .await;

    Ok(())
}