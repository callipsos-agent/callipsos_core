

use rust_decimal_macros::dec;
use uuid::Uuid;

use crate::policy::engine::evaluate;
use crate::policy::rules::PolicyRule;
use crate::policy::types::{
    Action, AssetSymbol, BasisPoints, Decision, EvaluationContext, EngineReason,
    Money, ProtocolId, RiskScore, RuleId, RuleOutcome, TransactionRequest, UserId,
};

// ── Test helpers ────────────────────────────────────────────

/// $50 supply of USDC to aave-v3. Same shape as rule tests.
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

/// Full context with all Option fields populated.
/// Designed so that safety_first_rules() + test_request() all pass.
fn full_context() -> EvaluationContext {
    EvaluationContext {
        portfolio_total_usd: Money::try_new(dec!(500)).unwrap(),
        current_protocol_exposure_usd: Money::try_new(dec!(100)).unwrap(),
        current_asset_exposure_usd: Money::try_new(dec!(100)).unwrap(),
        daily_spend_usd: Money::try_new(dec!(30)).unwrap(),
        audited_protocols: vec![ProtocolId::new("aave-v3"), ProtocolId::new("moonwell")],
        protocol_risk_score: Some(RiskScore::try_new(dec!(0.85)).unwrap()),
        protocol_utilization: Some(BasisPoints::from_percent(60).unwrap()),
        protocol_tvl: Some(Money::try_new(dec!(500_000_000)).unwrap()),
    }
}

/// Strict safety-first rules. All 10 rules with thresholds that
/// test_request() + full_context() will pass.
fn safety_first_rules() -> Vec<PolicyRule> {
    vec![
        PolicyRule::MaxTransactionAmount(Money::try_new(dec!(500)).unwrap()),
        PolicyRule::MaxPercentPerProtocol(BasisPoints::from_percent(40).unwrap()),
        PolicyRule::MaxPercentPerAsset(BasisPoints::from_percent(70).unwrap()),
        PolicyRule::OnlyAuditedProtocols,
        PolicyRule::AllowedProtocols(vec![
            ProtocolId::new("aave-v3"),
            ProtocolId::new("moonwell"),
        ]),
        PolicyRule::BlockedActions(vec![Action::Borrow, Action::Swap]),
        PolicyRule::MaxDailySpend(Money::try_new(dec!(500)).unwrap()),
        PolicyRule::MinRiskScore(RiskScore::try_new(dec!(0.80)).unwrap()),
        PolicyRule::MaxProtocolUtilization(BasisPoints::from_percent(85).unwrap()),
        PolicyRule::MinProtocolTvl(Money::try_new(dec!(10_000_000)).unwrap()),
    ]
}

// ── 1. Empty rules → blocked by engine ──────────────────────

#[test]
fn empty_rules_returns_blocked() {
    let verdict = evaluate(&[], &test_request(), &full_context());

    assert_eq!(verdict.decision, Decision::Blocked);
    assert!(verdict.results.is_empty());
    assert_eq!(
        verdict.engine_reason,
        Some(EngineReason::NoPoliciesConfigured)
    );
}

// ── 2. All rules pass → approved ────────────────────────────

#[test]
fn all_rules_pass_returns_approved() {
    let rules = safety_first_rules();
    let verdict = evaluate(&rules, &test_request(), &full_context());

    assert_eq!(verdict.decision, Decision::Approved);
    assert_eq!(verdict.results.len(), rules.len());
    assert!(verdict.failed_rules().is_empty());
    assert!(verdict.engine_reason.is_none());

    for result in &verdict.results {
        assert_eq!(result.outcome(), &RuleOutcome::Pass);
    }
}

// ── 3. Single rule fails → blocked ─────────────────────────

#[test]
fn single_rule_fails_returns_blocked() {
    let mut rules = safety_first_rules();
    let idx = rules.iter().position(|r| r.id() == RuleId::MaxTransactionAmount).unwrap();
    rules[idx] = PolicyRule::MaxTransactionAmount(Money::try_new(dec!(25)).unwrap());

    let verdict = evaluate(&rules, &test_request(), &full_context());

    assert_eq!(verdict.decision, Decision::Blocked);
    assert_eq!(verdict.results.len(), rules.len());
    assert_eq!(verdict.failed_rules().len(), 1);
    assert_eq!(verdict.failed_rules()[0].rule(), &RuleId::MaxTransactionAmount);
    assert!(verdict.engine_reason.is_none())
}

// ── 4. Multiple rules fail → blocked ────────────────────────

#[test]
fn multiple_rules_fail_returns_blocked() {
    let rules = safety_first_rules();
    let mut req = test_request();
    req.target_protocol = ProtocolId::new("shady-yield");

    let verdict = evaluate(&rules, &req, &full_context());

    assert_eq!(verdict.decision, Decision::Blocked);
    assert_eq!(verdict.results.len(), rules.len());
    assert_eq!(verdict.failed_rules().len(), 2);
    assert!(verdict.engine_reason.is_none());

    let failed_ids: Vec<&RuleId> = verdict.failed_rules().iter().map(|r| r.rule()).collect();
    assert!(failed_ids.contains(&&RuleId::OnlyAuditedProtocols));
    assert!(failed_ids.contains(&&RuleId::AllowedProtocols));
}

// ── 5. Indeterminate treated as blocked ─────────────────────

#[test]
fn indeterminate_treated_as_blocked() {
    let rules = safety_first_rules();
    let mut ctx = full_context();
    ctx.portfolio_total_usd = Money::zero();

    let verdict = evaluate(&rules, &test_request(), &ctx);

    assert_eq!(verdict.decision, Decision::Blocked);
    assert!(verdict.engine_reason.is_none());

    let indeterminate_ids: Vec<&RuleId> = verdict
        .results
        .iter()
        .filter(|r| r.outcome() == &RuleOutcome::Indeterminate)
        .map(|r| r.rule())
        .collect();
    assert_eq!(indeterminate_ids.len(), 2);
    assert!(indeterminate_ids.contains(&&RuleId::MaxPercentPerProtocol));
    assert!(indeterminate_ids.contains(&&RuleId::MaxPercentPerAsset));

    let failed_ids: Vec<&RuleId> = verdict.failed_rules().iter().map(|r| r.rule()).collect();
    assert!(failed_ids.contains(&&RuleId::MaxPercentPerProtocol));
    assert!(failed_ids.contains(&&RuleId::MaxPercentPerAsset));
}

// ── 6. All rules evaluated, no short circuit ────────────────

#[test]
fn all_rules_evaluated_no_short_circuit() {
    let mut rules = safety_first_rules();
    let idx = rules.iter().position(|r| r.id() == RuleId::MaxTransactionAmount).unwrap();
    rules[idx] = PolicyRule::MaxTransactionAmount(Money::try_new(dec!(25)).unwrap());

    let verdict = evaluate(&rules, &test_request(), &full_context());

    assert!(verdict.engine_reason.is_none());

    assert_eq!(verdict.results.len(), rules.len());
    for (i, result) in verdict.results.iter().enumerate() {
        assert_eq!(result.rule(), &rules[i].id());
    }
}
// ── 7. Duplicate rules all evaluated ────────────────────────

#[test]
fn duplicate_rules_all_evaluated() {
    let rules = vec![
        PolicyRule::MaxTransactionAmount(Money::try_new(dec!(500)).unwrap()),
        PolicyRule::MaxTransactionAmount(Money::try_new(dec!(500)).unwrap()),
    ];
    let verdict = evaluate(&rules, &test_request(), &full_context());

    assert_eq!(verdict.results.len(), 2);
    for result in verdict.results.iter() {
        assert_eq!(result.outcome(), &RuleOutcome::Pass);
        assert_eq!(result.rule(), &RuleId::MaxTransactionAmount);
    }
}

// ── 8. Blocked verdict preserves pass results ───────────────

#[test]
fn blocked_verdict_preserves_pass_results() {
    let mut rules = safety_first_rules();
    let idx = rules.iter().position(|r| r.id() == RuleId::MaxTransactionAmount).unwrap();
    rules[idx] = PolicyRule::MaxTransactionAmount(Money::try_new(dec!(25)).unwrap());

    let verdict = evaluate(&rules, &test_request(), &full_context());

    assert_eq!(verdict.decision, Decision::Blocked);
    assert!(verdict.engine_reason.is_none());

    let pass_count = verdict
        .results
        .iter()
        .filter(|r| r.outcome() == &RuleOutcome::Pass)
        .count();
    assert_eq!(pass_count, rules.len() - 1);
}

// ── 9. Verdict results order matches rules order ────────────

#[test]
fn verdict_results_order_matches_rules_order() {
    let rules = safety_first_rules();
    let verdict = evaluate(&rules, &test_request(), &full_context());

    assert_eq!(verdict.results.len(), rules.len());
    assert!(verdict.engine_reason.is_none());
    for (i, result) in verdict.results.iter().enumerate() {
        assert_eq!(result.rule(), &rules[i].id());
    }
}

// ── 10. Only indeterminate rules, all missing data → blocked ─

#[test]
fn only_indeterminate_capable_rules_all_missing_data_returns_blocked() {
    // Three rules that need Option data, context has all None
    let rules = vec![
        PolicyRule::MinRiskScore(RiskScore::try_new(dec!(0.80)).unwrap()),
        PolicyRule::MaxProtocolUtilization(BasisPoints::from_percent(85).unwrap()),
        PolicyRule::MinProtocolTvl(Money::try_new(dec!(10_000_000)).unwrap()),
    ];
    // test_context() equivalent with all Option fields as None
    let ctx = EvaluationContext {
        portfolio_total_usd: Money::try_new(dec!(500)).unwrap(),
        current_protocol_exposure_usd: Money::try_new(dec!(100)).unwrap(),
        current_asset_exposure_usd: Money::try_new(dec!(100)).unwrap(),
        daily_spend_usd: Money::try_new(dec!(30)).unwrap(),
        audited_protocols: vec![ProtocolId::new("aave-v3")],
        protocol_risk_score: None,
        protocol_utilization: None,
        protocol_tvl: None,
    };

    let verdict = evaluate(&rules, &test_request(), &ctx);

    assert_eq!(verdict.decision, Decision::Blocked);
    assert_eq!(verdict.results.len(), 3);
    assert!(verdict.engine_reason.is_none());

    for result in verdict.results.iter() {
        assert_eq!(result.outcome(), &RuleOutcome::Indeterminate);
    }
    assert_eq!(verdict.failed_rules().len(), 3);
}

// ── 11. Indeterminate first, fail later, both captured ──────

#[test]
fn indeterminate_first_fail_later_both_captured() {
    let rules = vec![
        // Will be Indeterminate — no risk score in context
        PolicyRule::MinRiskScore(RiskScore::try_new(dec!(0.80)).unwrap()),
        // Will Fail — request is $50, limit is $10
        PolicyRule::MaxTransactionAmount(Money::try_new(dec!(10)).unwrap()),
    ];
    let ctx = EvaluationContext {
        portfolio_total_usd: Money::try_new(dec!(500)).unwrap(),
        current_protocol_exposure_usd: Money::try_new(dec!(100)).unwrap(),
        current_asset_exposure_usd: Money::try_new(dec!(100)).unwrap(),
        daily_spend_usd: Money::try_new(dec!(30)).unwrap(),
        audited_protocols: vec![ProtocolId::new("aave-v3")],
        protocol_risk_score: None,
        protocol_utilization: None,
        protocol_tvl: None,
    };

    let verdict = evaluate(&rules, &test_request(), &ctx);

    assert_eq!(verdict.decision, Decision::Blocked);
    assert_eq!(verdict.results.len(), 2);
    assert_eq!(verdict.failed_rules().len(), 2);
    assert!(verdict.engine_reason.is_none());

    assert_eq!(verdict.results[0].outcome(), &RuleOutcome::Indeterminate);
    assert_eq!(verdict.results[0].rule(), &RuleId::MinRiskScore);
    assert_eq!(verdict.results[1].outcome(), &RuleOutcome::Fail);
    assert_eq!(verdict.results[1].rule(), &RuleId::MaxTransactionAmount);
}

// ── 12. Total chaos — all 10 rules fail simultaneously ──────
// The chaos agent's worst case: a request that violates EVERY rule.
// Confirms the engine doesn't panic or truncate when everything fails.
// TODO: When action-aware rule filtering lands (Phase 2),
// switch to Action::Supply or update expected failure count.
#[test]
fn all_rules_fail_total_chaos() {
    let rules = safety_first_rules();

    // Request that violates everything:
    // - $5000 (exceeds $500 limit)
    // - Borrow (blocked action)
    // - "shady-yield" (not audited, not in allowed list)
    let req = TransactionRequest {
        user_id: UserId::from(Uuid::nil()),
        target_protocol: ProtocolId::new("shady-yield"),
        action: Action::Borrow,
        asset: AssetSymbol::new("USDC"),
        amount_usd: Money::try_new(dec!(5000)).unwrap(),
        target_address: "0xDEAD".into(),
    };

    // Context that maximizes failures:
    // - Portfolio $500, protocol exposure $490, asset exposure $490 (both near 100%)
    // - Daily spend $490 (adding $5000 blows past $500 limit)
    // - Risk score 0.30 (below 0.80 minimum)
    // - Utilization 95% (above 85% cap)
    // - TVL $1M (below $10M floor)
    let ctx = EvaluationContext {
        portfolio_total_usd: Money::try_new(dec!(500)).unwrap(),
        current_protocol_exposure_usd: Money::try_new(dec!(490)).unwrap(),
        current_asset_exposure_usd: Money::try_new(dec!(490)).unwrap(),
        daily_spend_usd: Money::try_new(dec!(490)).unwrap(),
        audited_protocols: vec![ProtocolId::new("aave-v3")],
        protocol_risk_score: Some(RiskScore::try_new(dec!(0.30)).unwrap()),
        protocol_utilization: Some(BasisPoints::from_percent(95).unwrap()),
        protocol_tvl: Some(Money::try_new(dec!(1_000_000)).unwrap()),
    };

    let verdict = evaluate(&rules, &req, &ctx);

    assert_eq!(verdict.decision, Decision::Blocked);
    assert_eq!(verdict.results.len(), rules.len());
    // All 10 rules should fail — no passes
    assert_eq!(verdict.failed_rules().len(), rules.len());
    assert!(verdict.engine_reason.is_none());

    for result in verdict.results.iter() {
        assert!(
            result.outcome() == &RuleOutcome::Fail,
            "expected Fail for {:?}, got {:?}",
            result.rule(),
            result.outcome(),
        );
    }
}