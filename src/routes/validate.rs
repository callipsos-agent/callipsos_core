use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::db::policy::PolicyRow;
use crate::db::transaction_log::TransactionLogRow;
use crate::db::user::User;
use crate::error::AppError;
use crate::policy::engine;
use crate::policy::rules::PolicyRule;
use crate::policy::types::{
    Action, AssetSymbol, BasisPoints, Decision, EvaluationContext, Money,
    PolicyVerdict, ProtocolId, RiskScore, TransactionRequest, UserId,
};
use crate::routes::AppState;
use crate::signing::SigningResult;

// ── Request structs (API shape) ─────────────────────────────

#[derive(Deserialize)]
pub struct ValidateRequest {
    pub user_id: Uuid,
    pub target_protocol: String,
    pub action: Action,
    pub asset: String,
    pub amount_usd: String,
    pub target_address: String,
    pub context: ValidateContextRequest,
}

#[derive(Deserialize)]
pub struct ValidateContextRequest {
    pub portfolio_total_usd: String,
    pub current_protocol_exposure_usd: String,
    pub current_asset_exposure_usd: String,
    pub daily_spend_usd: String,
    pub audited_protocols: Vec<String>,
    pub protocol_risk_score: Option<f64>,
    pub protocol_utilization_pct: Option<f64>,
    pub protocol_tvl_usd: Option<String>,
}

// ── Response struct ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ValidateResponse {
    #[serde(flatten)]
    pub verdict: PolicyVerdict,
    pub signing: Option<SigningResult>,
}

// ── Conversion helpers ──────────────────────────────────────

fn parse_money(field: &str, value: &str) -> Result<Money, AppError> {
    let d = Decimal::from_str(value)
        .map_err(|_| AppError::BadRequest(format!("Invalid decimal for {field}: '{value}'")))?;
    Money::try_new(d)
        .map_err(|e| AppError::BadRequest(format!("Invalid {field}: {e}")))
}

fn convert_request(req: &ValidateRequest) -> Result<(TransactionRequest, EvaluationContext), AppError> {
    let amount_usd = parse_money("amount_usd", &req.amount_usd)?;
    let portfolio_total_usd = parse_money("portfolio_total_usd", &req.context.portfolio_total_usd)?;
    let current_protocol_exposure_usd = parse_money(
        "current_protocol_exposure_usd",
        &req.context.current_protocol_exposure_usd,
    )?;
    let current_asset_exposure_usd = parse_money(
        "current_asset_exposure_usd",
        &req.context.current_asset_exposure_usd,
    )?;
    let daily_spend_usd = parse_money("daily_spend_usd", &req.context.daily_spend_usd)?;

    let protocol_risk_score = req
        .context
        .protocol_risk_score
        .map(|v| {
            let d = Decimal::from_f64_retain(v)
                .ok_or_else(|| AppError::BadRequest(format!("Invalid protocol_risk_score: {v}")))?;
            RiskScore::try_new(d)
                .map_err(|e| AppError::BadRequest(format!("Invalid protocol_risk_score: {e}")))
        })
        .transpose()?;

    let protocol_utilization = req
        .context
        .protocol_utilization_pct
        .map(|v| {
            let bps = (v * 10_000.0) as u32;
            BasisPoints::new_checked(bps)
                .map_err(|e| AppError::BadRequest(format!("Invalid protocol_utilization_pct: {e}")))
        })
        .transpose()?;

    let protocol_tvl = req
        .context
        .protocol_tvl_usd
        .as_ref()
        .map(|v| parse_money("protocol_tvl_usd", v))
        .transpose()?;

    let tx_request = TransactionRequest {
        user_id: UserId::from(req.user_id),
        target_protocol: ProtocolId::new(&req.target_protocol),
        action: req.action.clone(),
        asset: AssetSymbol::new(&req.asset),
        amount_usd,
        target_address: req.target_address.clone(),
    };

    let eval_context = EvaluationContext {
        portfolio_total_usd,
        current_protocol_exposure_usd,
        current_asset_exposure_usd,
        daily_spend_usd,
        audited_protocols: req
            .context
            .audited_protocols
            .iter()
            .map(|s| ProtocolId::new(s))
            .collect(),
        protocol_risk_score,
        protocol_utilization,
        protocol_tvl,
    };

    Ok((tx_request, eval_context))
}

// ── Handler ─────────────────────────────────────────────────

pub async fn validate(
    State(state): State<AppState>,
    Json(req): Json<ValidateRequest>,
) -> Result<(StatusCode, Json<ValidateResponse>), AppError> {
    // 1-2. Parse and convert
    let user_id = UserId::from(req.user_id);
    let (tx_request, eval_context) = convert_request(&req)?;

    // 3. Verify user exists
    let user = User::find_by_id(&state.db, user_id).await?;
    if user.is_none() {
        return Err(AppError::NotFound(format!("User {} not found", req.user_id)));
    }

    // 4. Load active policies
    let policy_rows = PolicyRow::find_active_by_user(&state.db, user_id).await?;

    // 5. Deserialize and flatten all rules
    let all_rules: Vec<PolicyRule> = policy_rows
        .iter()
        .map(|row| {
            serde_json::from_value::<Vec<PolicyRule>>(row.rules_json.clone())
                .map_err(|e| AppError::Internal(format!(
                    "Failed to deserialize rules for policy {}: {e}",
                    row.id
                )))
        })
        .collect::<Result<Vec<Vec<PolicyRule>>, AppError>>()?
        .into_iter()
        .flatten()
        .collect();

    // 6. Evaluate
    let verdict = engine::evaluate(&all_rules, &tx_request, &eval_context);

    // 7. Attempt signing if approved and provider is configured
    let signing = if verdict.decision == Decision::Approved {
        if let Some(ref provider) = state.signing_provider {
            // Generate a placeholder tx hash until we have real on-chain txs
            let tx_hash = format!("0x{}", Uuid::new_v4().simple());

            match provider.sign_verdict(&verdict, &tx_hash).await {
                Ok(result) => Some(result),
                Err(e) => {
                    tracing::warn!("Signing failed (verdict still valid): {e}");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // 8. Log to transaction_log
    let verdict_str = match verdict.decision {
        Decision::Approved => "approved",
        Decision::Blocked => "blocked",
    };
    let request_json = serde_json::to_value(&tx_request)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let reasons_json = serde_json::to_value(&verdict.results)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    TransactionLogRow::create(
        &state.db,
        user_id,
        None, // multi-policy, no single policy_id
        request_json,
        verdict_str,
        reasons_json,
    )
    .await?;

    // 9. Return
    let response = ValidateResponse {
        verdict,
        signing,
    };

    Ok((StatusCode::OK, Json(response)))
}