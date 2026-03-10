use rust_decimal_macros::dec;
use uuid::Uuid;

use crate::policy::presets::{balanced, best_yields, safety_first};
use crate::policy::rules::PolicyRule;
use crate::policy::types::{
    Action, BasisPoints, Money, RiskScore, RuleId, AssetSymbol, Decision, EvaluationContext, ProtocolId, TransactionRequest, UserId,
};
use crate::policy::engine::evaluate;

// ── Helpers for extracting thresholds from rule vecs ─────────

fn find_max_tx_amount(rules: &[PolicyRule]) -> Money {
    rules.iter().find_map(|r| match r {
        PolicyRule::MaxTransactionAmount(m) => Some(*m),
        _ => None,
    }).expect("preset missing MaxTransactionAmount")
}

fn find_max_daily_spend(rules: &[PolicyRule]) -> Money {
    rules.iter().find_map(|r| match r {
        PolicyRule::MaxDailySpend(m) => Some(*m),
        _ => None,
    }).expect("preset missing MaxDailySpend")
}

fn find_max_percent_per_protocol(rules: &[PolicyRule]) -> BasisPoints {
    rules.iter().find_map(|r| match r {
        PolicyRule::MaxPercentPerProtocol(bps) => Some(*bps),
        _ => None,
    }).expect("preset missing MaxPercentPerProtocol")
}

fn find_max_percent_per_asset(rules: &[PolicyRule]) -> BasisPoints {
    rules.iter().find_map(|r| match r {
        PolicyRule::MaxPercentPerAsset(bps) => Some(*bps),
        _ => None,
    }).expect("preset missing MaxPercentPerAsset")
}

fn find_blocked_actions(rules: &[PolicyRule]) -> Vec<Action> {
    rules.iter().find_map(|r| match r {
        PolicyRule::BlockedActions(actions) => Some(actions.clone()),
        _ => None,
    }).expect("preset missing BlockedActions")
}

fn find_min_risk_score(rules: &[PolicyRule]) -> RiskScore {
    rules.iter().find_map(|r| match r {
        PolicyRule::MinRiskScore(s) => Some(*s),
        _ => None,
    }).expect("preset missing MinRiskScore")
}

fn find_max_protocol_utilization(rules: &[PolicyRule]) -> BasisPoints {
    rules.iter().find_map(|r| match r {
        PolicyRule::MaxProtocolUtilization(bps) => Some(*bps),
        _ => None,
    }).expect("preset missing MaxProtocolUtilization")
}

fn find_min_protocol_tvl(rules: &[PolicyRule]) -> Money {
    rules.iter().find_map(|r| match r {
        PolicyRule::MinProtocolTvl(m) => Some(*m),
        _ => None,
    }).expect("preset missing MinProtocolTvl")
}

fn has_only_audited_protocols(rules: &[PolicyRule]) -> bool {
    rules.iter().any(|r| matches!(r, PolicyRule::OnlyAuditedProtocols))
}

fn collect_rule_ids(rules: &[PolicyRule]) -> Vec<RuleId> {
    rules.iter().map(|r| r.id()).collect()
}


/// Builds a context where all optional fields are populated with safe values.
/// Caller should set portfolio/exposure/spend values appropriate to the preset.
fn safe_context(portfolio: Money, protocol_exposure: Money, asset_exposure: Money, daily_spend: Money) -> EvaluationContext {
    EvaluationContext {
        portfolio_total_usd: portfolio,
        current_protocol_exposure_usd: protocol_exposure,
        current_asset_exposure_usd: asset_exposure,
        daily_spend_usd: daily_spend,
        audited_protocols: vec![ProtocolId::new("aave-v3"), ProtocolId::new("moonwell")],
        protocol_risk_score: Some(RiskScore::try_new(dec!(0.90)).unwrap()),
        protocol_utilization: Some(BasisPoints::from_percent(50).unwrap()),
        protocol_tvl: Some(Money::try_new(dec!(500_000_000)).unwrap()),
    }
}

// ── 1. safety_first returns expected rule types ─────────────

#[test]
fn safety_first_returns_expected_rule_types() {
    let rules = safety_first();

    assert_eq!(find_max_tx_amount(&rules), Money::try_new(dec!(500)).unwrap());
    assert_eq!(find_max_daily_spend(&rules), Money::try_new(dec!(1000)).unwrap());
    assert_eq!(find_max_percent_per_protocol(&rules), BasisPoints::from_percent(10).unwrap());
    assert_eq!(find_max_percent_per_asset(&rules), BasisPoints::from_percent(30).unwrap());
    assert!(has_only_audited_protocols(&rules));
    assert_eq!(find_min_risk_score(&rules), RiskScore::try_new(dec!(0.80)).unwrap());
    assert_eq!(find_max_protocol_utilization(&rules), BasisPoints::from_percent(80).unwrap());
    assert_eq!(find_min_protocol_tvl(&rules), Money::try_new(dec!(50_000_000)).unwrap());

    let blocked = find_blocked_actions(&rules);
    assert!(blocked.contains(&Action::Borrow));
    assert!(blocked.contains(&Action::Swap));
    assert!(blocked.contains(&Action::Transfer));
}

// ── 2. best_yields returns expected rule types ──────────────

#[test]
fn best_yields_returns_expected_rule_types() {
    let rules = best_yields();

    assert_eq!(find_max_tx_amount(&rules), Money::try_new(dec!(5000)).unwrap());
    assert_eq!(find_max_daily_spend(&rules), Money::try_new(dec!(10_000)).unwrap());
    assert_eq!(find_max_percent_per_protocol(&rules), BasisPoints::from_percent(40).unwrap());
    assert_eq!(find_max_percent_per_asset(&rules), BasisPoints::from_percent(70).unwrap());
    assert!(has_only_audited_protocols(&rules));
    assert_eq!(find_min_risk_score(&rules), RiskScore::try_new(dec!(0.50)).unwrap());
    assert_eq!(find_max_protocol_utilization(&rules), BasisPoints::from_percent(95).unwrap());
    assert_eq!(find_min_protocol_tvl(&rules), Money::try_new(dec!(5_000_000)).unwrap());

    let blocked = find_blocked_actions(&rules);
    assert_eq!(blocked.len(), 1);
    assert!(blocked.contains(&Action::Transfer));
}

// ── 3. balanced returns expected rule types ──────────────────

#[test]
fn balanced_returns_expected_rule_types() {
    let rules = balanced();

    assert_eq!(find_max_tx_amount(&rules), Money::try_new(dec!(2000)).unwrap());
    assert_eq!(find_max_daily_spend(&rules), Money::try_new(dec!(5000)).unwrap());
    assert_eq!(find_max_percent_per_protocol(&rules), BasisPoints::from_percent(25).unwrap());
    assert_eq!(find_max_percent_per_asset(&rules), BasisPoints::from_percent(50).unwrap());
    assert!(has_only_audited_protocols(&rules));
    assert_eq!(find_min_risk_score(&rules), RiskScore::try_new(dec!(0.65)).unwrap());
    assert_eq!(find_max_protocol_utilization(&rules), BasisPoints::from_percent(90).unwrap());
    assert_eq!(find_min_protocol_tvl(&rules), Money::try_new(dec!(10_000_000)).unwrap());

    let blocked = find_blocked_actions(&rules);
    assert!(blocked.contains(&Action::Borrow));
    assert!(blocked.contains(&Action::Transfer));
    assert!(!blocked.contains(&Action::Swap));
}

// ── 4. Presets are monotonically ordered ────────────────────
// safety_first is strictest, best_yields is most permissive,
// balanced is in between. This catches threshold drift.

#[test]
fn presets_are_monotonically_ordered() {
    let safe = safety_first();
    let bal = balanced();
    let best = best_yields();

    // Higher limits = more permissive
    assert!(find_max_tx_amount(&safe) < find_max_tx_amount(&bal));
    assert!(find_max_tx_amount(&bal) < find_max_tx_amount(&best));

    assert!(find_max_daily_spend(&safe) < find_max_daily_spend(&bal));
    assert!(find_max_daily_spend(&bal) < find_max_daily_spend(&best));

    // Higher percent caps = more permissive
    assert!(find_max_percent_per_protocol(&safe) < find_max_percent_per_protocol(&bal));
    assert!(find_max_percent_per_protocol(&bal) < find_max_percent_per_protocol(&best));

    assert!(find_max_percent_per_asset(&safe) < find_max_percent_per_asset(&bal));
    assert!(find_max_percent_per_asset(&bal) < find_max_percent_per_asset(&best));

    // Lower risk score floor = more permissive
    assert!(find_min_risk_score(&safe) > find_min_risk_score(&bal));
    assert!(find_min_risk_score(&bal) > find_min_risk_score(&best));

    // Higher utilization cap = more permissive
    assert!(find_max_protocol_utilization(&safe) < find_max_protocol_utilization(&bal));
    assert!(find_max_protocol_utilization(&bal) < find_max_protocol_utilization(&best));

    // Lower TVL floor = more permissive
    assert!(find_min_protocol_tvl(&safe) > find_min_protocol_tvl(&bal));
    assert!(find_min_protocol_tvl(&bal) > find_min_protocol_tvl(&best));

    // More blocked actions = more restrictive
    assert!(find_blocked_actions(&safe).len() > find_blocked_actions(&bal).len());
    assert!(find_blocked_actions(&bal).len() > find_blocked_actions(&best).len());
}

// ── 5. No preset has duplicate rule IDs ─────────────────────

#[test]
fn no_preset_has_duplicate_rule_ids() {
    let presets: Vec<(&str, Vec<PolicyRule>)> = vec![
        ("safety_first", safety_first()),
        ("balanced", balanced()),
        ("best_yields", best_yields()),
    ];
    for (name, rules) in presets {
        let ids = collect_rule_ids(&rules);
        let mut seen = std::collections::HashSet::new();
        for id in &ids {
            assert!(
                seen.insert(id),
                "preset {name} has duplicate rule: {id}",
            );
        }
    }
}


// ── 6. safety_first blocks large transaction ────────────────

#[test]
fn safety_first_blocks_large_transaction() {
    let rules = safety_first();
    // $600 exceeds safety_first's $500 limit
    let req = TransactionRequest {
        user_id: UserId::from(Uuid::nil()),
        target_protocol: ProtocolId::new("aave-v3"),
        action: Action::Supply,
        asset: AssetSymbol::new("USDC"),
        amount_usd: Money::try_new(dec!(600)).unwrap(),
        target_address: "0x1234".into(),
    };
    // Large portfolio so percentage rules don't interfere
    let ctx = safe_context(
        Money::try_new(dec!(100_000)).unwrap(),
        Money::zero(),
        Money::zero(),
        Money::zero(),
    );

    let verdict = evaluate(&rules, &req, &ctx);

    assert_eq!(verdict.decision, Decision::Blocked);
    assert!(verdict.engine_reason.is_none());
    assert_eq!(verdict.failed_rules().len(), 1);
    let failed_ids: Vec<RuleId> = verdict.failed_rules().iter().map(|r| r.rule().clone()).collect();
    assert!(failed_ids.contains(&RuleId::MaxTransactionAmount));
}

// ── 7. safety_first approves small safe transaction ─────────

#[test]
fn safety_first_approves_small_safe_transaction() {
    let rules = safety_first();
    let req = TransactionRequest {
        user_id: UserId::from(Uuid::nil()),
        target_protocol: ProtocolId::new("aave-v3"),
        action: Action::Supply,
        asset: AssetSymbol::new("USDC"),
        amount_usd: Money::try_new(dec!(30)).unwrap(),
        target_address: "0x1234".into(),
    };
    // $30 on $10k portfolio = 0.3% exposure, well within all limits
    let ctx = safe_context(
        Money::try_new(dec!(10_000)).unwrap(),
        Money::zero(),
        Money::zero(),
        Money::zero(),
    );

    let verdict = evaluate(&rules, &req, &ctx);

    assert_eq!(verdict.decision, Decision::Approved);
    assert!(verdict.failed_rules().is_empty());
    assert!(verdict.engine_reason.is_none());
    assert_eq!(verdict.results.len(), rules.len());
}

// ── 8. safety_first blocks unaudited protocol ───────────────

#[test]
fn safety_first_blocks_unaudited_protocol() {
    let rules = safety_first();
    let req = TransactionRequest {
        user_id: UserId::from(Uuid::nil()),
        target_protocol: ProtocolId::new("shady-yield"),
        action: Action::Supply,
        asset: AssetSymbol::new("USDC"),
        amount_usd: Money::try_new(dec!(10)).unwrap(),
        target_address: "0x1234".into(),
    };
    let ctx = safe_context(
        Money::try_new(dec!(10_000)).unwrap(),
        Money::zero(),
        Money::zero(),
        Money::zero(),
    );

    let verdict = evaluate(&rules, &req, &ctx);

    assert_eq!(verdict.decision, Decision::Blocked);
    assert!(verdict.engine_reason.is_none());
    let failed_ids: Vec<RuleId> = verdict.failed_rules().iter().map(|r| r.rule().clone()).collect();
    assert!(failed_ids.contains(&RuleId::OnlyAuditedProtocols));
}

// ── 9. safety_first blocks swap action ──────────────────────

#[test]
fn safety_first_blocks_swap_action() {
    let rules = safety_first();
    let req = TransactionRequest {
        user_id: UserId::from(Uuid::nil()),
        target_protocol: ProtocolId::new("aave-v3"),
        action: Action::Swap,
        asset: AssetSymbol::new("USDC"),
        amount_usd: Money::try_new(dec!(10)).unwrap(),
        target_address: "0x1234".into(),
    };
    let ctx = safe_context(
        Money::try_new(dec!(10_000)).unwrap(),
        Money::zero(),
        Money::zero(),
        Money::zero(),
    );

    let verdict = evaluate(&rules, &req, &ctx);

    assert_eq!(verdict.decision, Decision::Blocked);
    assert!(verdict.engine_reason.is_none());
    assert_eq!(verdict.failed_rules().len(), 1);
    let failed_ids: Vec<RuleId> = verdict.failed_rules().iter().map(|r| r.rule().clone()).collect();
    assert!(failed_ids.contains(&RuleId::BlockedActions));
}

// ── 10. best_yields approves what safety_first blocks ───────

#[test]
fn best_yields_approves_larger_transaction() {
    let safe_rules = safety_first();
    let best_rules = best_yields();

    // $2000 transaction — over safety_first's $500 but under best_yields' $5000
    let req = TransactionRequest {
        user_id: UserId::from(Uuid::nil()),
        target_protocol: ProtocolId::new("aave-v3"),
        action: Action::Supply,
        asset: AssetSymbol::new("USDC"),
        amount_usd: Money::try_new(dec!(2000)).unwrap(),
        target_address: "0x1234".into(),
    };
    let ctx = safe_context(
        Money::try_new(dec!(100_000)).unwrap(),
        Money::zero(),
        Money::zero(),
        Money::zero(),
    );

    let safe_verdict = evaluate(&safe_rules, &req, &ctx);
    let best_verdict = evaluate(&best_rules, &req, &ctx);

    assert_eq!(safe_verdict.decision, Decision::Blocked);
    assert_eq!(best_verdict.decision, Decision::Approved);
    assert!(safe_verdict.engine_reason.is_none());
    assert!(best_verdict.engine_reason.is_none());
}

// ── 11. balanced approves moderate transaction ───────────────

#[test]
fn balanced_approves_moderate_transaction() {
    let rules = balanced();
    // $1000 — under balanced's $2000 limit
    let req = TransactionRequest {
        user_id: UserId::from(Uuid::nil()),
        target_protocol: ProtocolId::new("aave-v3"),
        action: Action::Supply,
        asset: AssetSymbol::new("USDC"),
        amount_usd: Money::try_new(dec!(1000)).unwrap(),
        target_address: "0x1234".into(),
    };
    let ctx = safe_context(
        Money::try_new(dec!(100_000)).unwrap(),
        Money::zero(),
        Money::zero(),
        Money::zero(),
    );

    let verdict = evaluate(&rules, &req, &ctx);

    assert_eq!(verdict.decision, Decision::Approved);
    assert!(verdict.engine_reason.is_none());
    assert_eq!(verdict.results.len(), rules.len());
}

// ── 12. balanced blocks extreme transaction ─────────────────

#[test]
fn balanced_blocks_extreme_transaction() {
    let rules = balanced();
    // $3000 — over balanced's $2000 limit
    let req = TransactionRequest {
        user_id: UserId::from(Uuid::nil()),
        target_protocol: ProtocolId::new("aave-v3"),
        action: Action::Supply,
        asset: AssetSymbol::new("USDC"),
        amount_usd: Money::try_new(dec!(3000)).unwrap(),
        target_address: "0x1234".into(),
    };
    let ctx = safe_context(
        Money::try_new(dec!(100_000)).unwrap(),
        Money::zero(),
        Money::zero(),
        Money::zero(),
    );

    let verdict = evaluate(&rules, &req, &ctx);

    assert_eq!(verdict.decision, Decision::Blocked);
    assert!(verdict.engine_reason.is_none());
    assert_eq!(verdict.failed_rules().len(), 1);
    let failed_ids: Vec<RuleId> = verdict.failed_rules().iter().map(|r| r.rule().clone()).collect();
    assert!(failed_ids.contains(&RuleId::MaxTransactionAmount));
}