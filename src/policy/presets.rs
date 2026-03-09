use rust_decimal_macros::dec;

use crate::policy::rules::PolicyRule;
use crate::policy::types::{Action, BasisPoints, Money, RiskScore};

/// Conservative defaults. Prioritizes capital preservation.
/// Blocks Borrow, Swap, Transfer — only allows Supply, Withdraw, Stake.
pub fn safety_first() -> Vec<PolicyRule> {
    vec![
        PolicyRule::MaxTransactionAmount(Money::try_new(dec!(500)).unwrap()),
        PolicyRule::MaxDailySpend(Money::try_new(dec!(1000)).unwrap()),
        PolicyRule::MaxPercentPerProtocol(BasisPoints::from_percent(10).unwrap()),
        PolicyRule::MaxPercentPerAsset(BasisPoints::from_percent(30).unwrap()),
        PolicyRule::OnlyAuditedProtocols,
        PolicyRule::BlockedActions(vec![Action::Borrow, Action::Swap, Action::Transfer]),
        PolicyRule::MinRiskScore(RiskScore::try_new(dec!(0.80)).unwrap()),
        PolicyRule::MaxProtocolUtilization(BasisPoints::from_percent(80).unwrap()),
        PolicyRule::MinProtocolTvl(Money::try_new(dec!(50_000_000)).unwrap()),
    ]
}

/// Middle ground. Allows Swap, blocks Borrow and Transfer.
pub fn balanced() -> Vec<PolicyRule> {
    vec![
        PolicyRule::MaxTransactionAmount(Money::try_new(dec!(2000)).unwrap()),
        PolicyRule::MaxDailySpend(Money::try_new(dec!(5000)).unwrap()),
        PolicyRule::MaxPercentPerProtocol(BasisPoints::from_percent(25).unwrap()),
        PolicyRule::MaxPercentPerAsset(BasisPoints::from_percent(50).unwrap()),
        PolicyRule::OnlyAuditedProtocols,
        PolicyRule::BlockedActions(vec![Action::Borrow, Action::Transfer]),
        PolicyRule::MinRiskScore(RiskScore::try_new(dec!(0.65)).unwrap()),
        PolicyRule::MaxProtocolUtilization(BasisPoints::from_percent(90).unwrap()),
        PolicyRule::MinProtocolTvl(Money::try_new(dec!(10_000_000)).unwrap()),
    ]
}

/// Aggressive. Maximizes yield exposure within safety bounds.
/// Still requires audited protocols and blocks Transfer.
pub fn best_yields() -> Vec<PolicyRule> {
    vec![
        PolicyRule::MaxTransactionAmount(Money::try_new(dec!(5000)).unwrap()),
        PolicyRule::MaxDailySpend(Money::try_new(dec!(10_000)).unwrap()),
        PolicyRule::MaxPercentPerProtocol(BasisPoints::from_percent(40).unwrap()),
        PolicyRule::MaxPercentPerAsset(BasisPoints::from_percent(70).unwrap()),
        PolicyRule::OnlyAuditedProtocols,
        PolicyRule::BlockedActions(vec![Action::Transfer]),
        PolicyRule::MinRiskScore(RiskScore::try_new(dec!(0.50)).unwrap()),
        PolicyRule::MaxProtocolUtilization(BasisPoints::from_percent(95).unwrap()),
        PolicyRule::MinProtocolTvl(Money::try_new(dec!(5_000_000)).unwrap()),
    ]
}