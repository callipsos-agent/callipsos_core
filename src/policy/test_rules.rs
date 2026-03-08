use rust_decimal_macros::dec;
use uuid::Uuid;

use crate::policy::rules::PolicyRule;
use crate::policy::types::{
    Action, AssetSymbol, BasisPoints, EvaluationContext, Money, ProtocolId,
    RuleId, RuleOutcome, TransactionRequest, UserId, RiskScore
};

// ── Test helpers ────────────────────────────────────────

/// Builds a basic TransactionRequest for testing.
/// Defaults: $50 supply of USDC to aave-v3.
fn test_request() -> TransactionRequest {
    TransactionRequest {
        user_id: UserId::from(Uuid::nil()),
        target_protocol: ProtocolId::new("aave-v3"),
        action: Action::Supply,
        asset: AssetSymbol::new("USDC"),
        amount_usd: Money::try_new(dec!(50)).unwrap(),
        target_address: "0x1234".into(),
    }
}

/// Builds a basic EvaluationContext for testing.
/// Defaults: $500 portfolio, $100 protocol exposure, $100 asset exposure,
/// $30 daily spend, aave-v3 audited, no risk/utilization/tvl data.
fn test_context() -> EvaluationContext {
    EvaluationContext {
        portfolio_total_usd: Money::try_new(dec!(500)).unwrap(),
        current_protocol_exposure_usd: Money::try_new(dec!(100)).unwrap(),
        current_asset_exposure_usd: Money::try_new(dec!(100)).unwrap(),
        daily_spend_usd: Money::try_new(dec!(30)).unwrap(),
        audited_protocols: vec![ProtocolId::new("aave-v3"), ProtocolId::new("moonwell")],
        protocol_risk_score: None,
        protocol_utilization: None,
        protocol_tvl: None,
    }
}    

// ── 1. MaxTransactionAmount ─────────────────────────────

#[test]
fn max_tx_amount_pass() {
    // $50 request, $500 limit → pass
    let rule = PolicyRule::MaxTransactionAmount(Money::try_new(dec!(500)).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
    assert_eq!(result.rule(), &RuleId::MaxTransactionAmount);
    assert!(result.violation().is_none());
}

#[test]
fn max_tx_amount_fail() {
    // $50 request, $25 limit → fail
    let rule = PolicyRule::MaxTransactionAmount(Money::try_new(dec!(25)).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Fail);
    assert_eq!(result.rule(), &RuleId::MaxTransactionAmount);
    assert!(result.violation().is_some());
}

#[test]
fn max_tx_amount_exact_boundary_passes() {
    // $50 request, $50 limit → pass (not strictly greater)
    let rule = PolicyRule::MaxTransactionAmount(Money::try_new(dec!(50)).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
}

// ── 2. MaxPercentPerProtocol ────────────────────────────

#[test]
fn max_percent_per_protocol_pass() {
    // $100 existing + $50 request = $150 on $500 portfolio = 30%
    // Cap at 40% → pass
    let rule = PolicyRule::MaxPercentPerProtocol(BasisPoints::from_percent(40).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
    assert_eq!(result.rule(), &RuleId::MaxPercentPerProtocol);
}

#[test]
fn max_percent_per_protocol_fail() {
    // $100 existing + $50 request = $150 on $500 portfolio = 30%
    // Cap at 20% → fail
    let rule = PolicyRule::MaxPercentPerProtocol(BasisPoints::from_percent(20).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Fail);
    assert_eq!(result.rule(), &RuleId::MaxPercentPerProtocol);
    assert!(result.violation().is_some());
}

#[test]
fn max_percent_per_protocol_portfolio_zero() {
    // Portfolio total is zero → indeterminate (can't divide by zero)
    let rule = PolicyRule::MaxPercentPerProtocol(BasisPoints::from_percent(40).unwrap());
    let mut ctx = test_context();
    ctx.portfolio_total_usd = Money::zero();

    let result = rule.evaluate(&test_request(), &ctx);

    assert_eq!(result.outcome(), &RuleOutcome::Indeterminate);
    assert_eq!(result.rule(), &RuleId::MaxPercentPerProtocol);
    assert!(result.violation().is_some());
}

#[test]
fn max_percent_per_protocol_exact_boundary_passes() {
    // $100 + $50 = $150 on $500 = exactly 30%, cap at 30% → pass
    let rule = PolicyRule::MaxPercentPerProtocol(BasisPoints::from_percent(30).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
}

// ── 3. MaxPercentPerAsset ───────────────────────────────

#[test]
fn max_percent_per_asset_portfolio_zero() {
    let rule = PolicyRule::MaxPercentPerAsset(BasisPoints::from_percent(40).unwrap());
    let mut ctx = test_context();
    ctx.portfolio_total_usd = Money::zero();

    let result = rule.evaluate(&test_request(), &ctx);

    assert_eq!(result.outcome(), &RuleOutcome::Indeterminate);
    assert_eq!(result.rule(), &RuleId::MaxPercentPerAsset);
    assert!(result.violation().is_some());
}

#[test]
fn max_percent_per_asset_pass() {
    // $100 existing asset exposure + $50 request = $150 on $500 portfolio = 30%
    // Cap at 40% → pass
    let rule = PolicyRule::MaxPercentPerAsset(BasisPoints::from_percent(40).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
    assert_eq!(result.rule(), &RuleId::MaxPercentPerAsset);
}

#[test]
fn max_percent_per_asset_fail() {
    // $100 existing + $50 request = $150 on $500 portfolio = 30%
    // Cap at 20% → fail
    let rule = PolicyRule::MaxPercentPerAsset(BasisPoints::from_percent(20).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Fail);
    assert_eq!(result.rule(), &RuleId::MaxPercentPerAsset);
    assert!(result.violation().is_some());
}

#[test]
fn max_percent_per_asset_exact_boundary_passes() {
    // $100 + $50 = $150 on $500 = exactly 30%, cap at 30% → pass
    let rule = PolicyRule::MaxPercentPerAsset(BasisPoints::from_percent(30).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
}

// ── 4. OnlyAuditedProtocols ─────────────────────────────

#[test]
fn only_audited_protocols_pass() {
    // aave-v3 is in audited list → pass
    let rule = PolicyRule::OnlyAuditedProtocols;
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
    assert_eq!(result.rule(), &RuleId::OnlyAuditedProtocols);
}

#[test]
fn only_audited_protocols_fail() {
    // shady-yield is not in audited list → fail
    let rule = PolicyRule::OnlyAuditedProtocols;
    let mut req = test_request();
    req.target_protocol = ProtocolId::new("shady-yield");

    let result = rule.evaluate(&req, &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Fail);
    assert_eq!(result.rule(), &RuleId::OnlyAuditedProtocols);
    assert!(result.violation().is_some());
}

// ── 5. AllowedProtocols ─────────────────────────────────

#[test]
fn allowed_protocols_pass() {
    // aave-v3 is in allowed list → pass
    let rule = PolicyRule::AllowedProtocols(vec![
        ProtocolId::new("aave-v3"),
        ProtocolId::new("moonwell"),
    ]);
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
    assert_eq!(result.rule(), &RuleId::AllowedProtocols);
}

#[test]
fn allowed_protocols_fail() {
    // aave-v3 is NOT in allowed list (only moonwell allowed) → fail
    let rule = PolicyRule::AllowedProtocols(vec![ProtocolId::new("moonwell")]);
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Fail);
    assert_eq!(result.rule(), &RuleId::AllowedProtocols);
    assert!(result.violation().is_some());
}

// ── 6. BlockedActions ─────────────────────────────────────

#[test]
fn blocked_actions_pass() {
    // Action is Supply, blocked list is [Borrow, Swap] → pass
    let rule = PolicyRule::BlockedActions(vec![Action::Borrow, Action::Swap]);
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
    assert_eq!(result.rule(), &RuleId::BlockedActions);
}

#[test]
fn blocked_actions_fail() {
    // Action is Supply, blocked list includes Supply → fail
    let rule = PolicyRule::BlockedActions(vec![Action::Supply, Action::Borrow]);
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Fail);
    assert_eq!(result.rule(), &RuleId::BlockedActions);
    assert!(result.violation().is_some());
}

// ── 7. MaxDailySpend ────────────────────────────────────

#[test]
fn max_daily_spend_pass() {
    // $30 already spent + $50 request = $80, limit $100 → pass
    let rule = PolicyRule::MaxDailySpend(Money::try_new(dec!(100)).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
    assert_eq!(result.rule(), &RuleId::MaxDailySpend);
}

#[test]
fn max_daily_spend_fail() {
    // $30 already spent + $50 request = $80, limit $60 → fail
    let rule = PolicyRule::MaxDailySpend(Money::try_new(dec!(60)).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Fail);
    assert_eq!(result.rule(), &RuleId::MaxDailySpend);
    assert!(result.violation().is_some());
}

#[test]
fn max_daily_spend_exact_boundary_passes() {
    // $30 already spent + $50 request = $80, limit $80 → pass
    let rule = PolicyRule::MaxDailySpend(Money::try_new(dec!(80)).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
}

// ── 8. MinRiskScore ─────────────────────────────────────

#[test]
fn min_risk_score_pass() {
    // Protocol risk score 0.85, minimum 0.70 → pass
    let rule = PolicyRule::MinRiskScore(RiskScore::try_new(dec!(0.70)).unwrap());
    let mut ctx = test_context();
    ctx.protocol_risk_score = Some(RiskScore::try_new(dec!(0.85)).unwrap());

    let result = rule.evaluate(&test_request(), &ctx);

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
    assert_eq!(result.rule(), &RuleId::MinRiskScore);
}

#[test]
fn min_risk_score_fail() {
    // Protocol risk score 0.40, minimum 0.70 → fail
    let rule = PolicyRule::MinRiskScore(RiskScore::try_new(dec!(0.70)).unwrap());
    let mut ctx = test_context();
    ctx.protocol_risk_score = Some(RiskScore::try_new(dec!(0.40)).unwrap());

    let result = rule.evaluate(&test_request(), &ctx);

    assert_eq!(result.outcome(), &RuleOutcome::Fail);
    assert_eq!(result.rule(), &RuleId::MinRiskScore);
    assert!(result.violation().is_some());
}

#[test]
fn min_risk_score_missing_data() {
    // No risk score data → indeterminate
    let rule = PolicyRule::MinRiskScore(RiskScore::try_new(dec!(0.70)).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Indeterminate);
    assert_eq!(result.rule(), &RuleId::MinRiskScore);
    assert!(result.violation().is_some());
}

// ── 9. MaxProtocolUtilization ───────────────────────────

#[test]
fn max_protocol_utilization_pass() {
    // Utilization 60%, cap 80% → pass
    let rule = PolicyRule::MaxProtocolUtilization(BasisPoints::from_percent(80).unwrap());
    let mut ctx = test_context();
    ctx.protocol_utilization = Some(BasisPoints::from_percent(60).unwrap());

    let result = rule.evaluate(&test_request(), &ctx);

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
    assert_eq!(result.rule(), &RuleId::MaxProtocolUtilization);
}

#[test]
fn max_protocol_utilization_fail() {
    // Utilization 90%, cap 80% → fail
    let rule = PolicyRule::MaxProtocolUtilization(BasisPoints::from_percent(80).unwrap());
    let mut ctx = test_context();
    ctx.protocol_utilization = Some(BasisPoints::from_percent(90).unwrap());

    let result = rule.evaluate(&test_request(), &ctx);

    assert_eq!(result.outcome(), &RuleOutcome::Fail);
    assert_eq!(result.rule(), &RuleId::MaxProtocolUtilization);
    assert!(result.violation().is_some());
}

#[test]
fn max_protocol_utilization_missing_data() {
    // No utilization data → indeterminate
    let rule = PolicyRule::MaxProtocolUtilization(BasisPoints::from_percent(80).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Indeterminate);
    assert_eq!(result.rule(), &RuleId::MaxProtocolUtilization);
    assert!(result.violation().is_some());
}

#[test]
fn max_protocol_utilization_exact_boundary_passes() {
    // Utilization exactly 80%, cap 80% → pass
    let rule = PolicyRule::MaxProtocolUtilization(BasisPoints::from_percent(80).unwrap());
    let mut ctx = test_context();
    ctx.protocol_utilization = Some(BasisPoints::from_percent(80).unwrap());

    let result = rule.evaluate(&test_request(), &ctx);

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
}

// ── 10. MinProtocolTvl ──────────────────────────────────

#[test]
fn min_protocol_tvl_pass() {
    // TVL $500M, minimum $100M → pass
    let rule = PolicyRule::MinProtocolTvl(Money::try_new(dec!(100_000_000)).unwrap());
    let mut ctx = test_context();
    ctx.protocol_tvl = Some(Money::try_new(dec!(500_000_000)).unwrap());

    let result = rule.evaluate(&test_request(), &ctx);

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
    assert_eq!(result.rule(), &RuleId::MinProtocolTvl);
}

#[test]
fn min_protocol_tvl_fail() {
    // TVL $50M, minimum $100M → fail
    let rule = PolicyRule::MinProtocolTvl(Money::try_new(dec!(100_000_000)).unwrap());
    let mut ctx = test_context();
    ctx.protocol_tvl = Some(Money::try_new(dec!(50_000_000)).unwrap());

    let result = rule.evaluate(&test_request(), &ctx);

    assert_eq!(result.outcome(), &RuleOutcome::Fail);
    assert_eq!(result.rule(), &RuleId::MinProtocolTvl);
    assert!(result.violation().is_some());
}

#[test]
fn min_protocol_tvl_missing_data() {
    // No TVL data → indeterminate
    let rule = PolicyRule::MinProtocolTvl(Money::try_new(dec!(100_000_000)).unwrap());
    let result = rule.evaluate(&test_request(), &test_context());

    assert_eq!(result.outcome(), &RuleOutcome::Indeterminate);
    assert_eq!(result.rule(), &RuleId::MinProtocolTvl);
    assert!(result.violation().is_some());
}

#[test]
fn min_protocol_tvl_exact_boundary_passes() {
    // TVL exactly $100M, floor $100M → pass
    let rule = PolicyRule::MinProtocolTvl(Money::try_new(dec!(100_000_000)).unwrap());
    let mut ctx = test_context();
    ctx.protocol_tvl = Some(Money::try_new(dec!(100_000_000)).unwrap());

    let result = rule.evaluate(&test_request(), &ctx);

    assert_eq!(result.outcome(), &RuleOutcome::Pass);
}








