use serde::{Deserialize, Serialize};

use crate::policy::types::{
    Action, BasisPoints, CannotEvaluateReason, EvaluationContext, Money,
    ProtocolId, RiskScore, RuleId, RuleResult, TransactionRequest, Violation,
};

// ── PolicyRule ──────────────────────────────────────────────

/// Each variant carries its threshold. The engine iterates a `Vec<PolicyRule>`
/// and calls `evaluate()` on each one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyRule {
    MaxTransactionAmount(Money),
    MaxPercentPerProtocol(BasisPoints),
    MaxPercentPerAsset(BasisPoints),
    OnlyAuditedProtocols,
    AllowedProtocols(Vec<ProtocolId>),
    BlockedActions(Vec<Action>),
    MaxDailySpend(Money),
    MinRiskScore(RiskScore),
    MaxProtocolUtilization(BasisPoints),
    MinProtocolTvl(Money),
}

impl PolicyRule {
    /// Evaluate this single rule against a transaction request and its context.
    /// Returns a `RuleResult` indicating pass, fail, or indeterminate.
    pub fn evaluate(
        &self,
        request: &TransactionRequest,
        context: &EvaluationContext,
    ) -> RuleResult {
        match self {
            // ── 1. MaxTransactionAmount ──────────────────────
            PolicyRule::MaxTransactionAmount(max) => {
                if request.amount_usd > *max {
                    RuleResult::fail(
                        RuleId::MaxTransactionAmount,
                        Violation::TxAmountTooHigh {
                            requested: request.amount_usd,
                            max: *max,
                        },
                        format!("amount {} exceeds {} limit", request.amount_usd, max),
                    )
                } else {
                    RuleResult::pass(
                        RuleId::MaxTransactionAmount,
                        format!("amount {} within {} limit", request.amount_usd, max),
                    )
                }
            }

            // ── 2. MaxPercentPerProtocol ─────────────────────
            PolicyRule::MaxPercentPerProtocol(max_bps) => {
                if context.portfolio_total_usd.is_zero() {
                    return RuleResult::indeterminate(
                        RuleId::MaxPercentPerProtocol,
                        Violation::CannotEvaluate(CannotEvaluateReason::PortfolioTotalZero),
                        "cannot evaluate protocol exposure: portfolio total is zero",
                    );
                }

                let exposure_after = context.current_protocol_exposure_usd + request.amount_usd;
                let ratio = exposure_after.inner() / context.portfolio_total_usd.inner();
                let cap = max_bps.as_decimal();

                if ratio > cap {
                    RuleResult::fail(
                        RuleId::MaxPercentPerProtocol,
                        Violation::ProtocolExposureTooHigh {
                            current_plus_requested: exposure_after,
                            max_percent: *max_bps,
                            portfolio_total: context.portfolio_total_usd,
                        },
                        format!(
                            "protocol exposure {:.1}% exceeds {} cap",
                            ratio * rust_decimal::Decimal::ONE_HUNDRED,
                            max_bps,
                        ),
                    )
                } else {
                    RuleResult::pass(
                        RuleId::MaxPercentPerProtocol,
                        format!(
                            "protocol exposure {:.1}% within {} cap",
                            ratio * rust_decimal::Decimal::ONE_HUNDRED,
                            max_bps,
                        ),
                    )
                }
            }

            // ── 3. MaxPercentPerAsset ────────────────────────
            PolicyRule::MaxPercentPerAsset(max_bps) => {
                if context.portfolio_total_usd.is_zero() {
                    return RuleResult::indeterminate(
                        RuleId::MaxPercentPerAsset,
                        Violation::CannotEvaluate(CannotEvaluateReason::PortfolioTotalZero),
                        "cannot evaluate asset concentration: portfolio total is zero",
                    );
                }

                let exposure_after = context.current_asset_exposure_usd + request.amount_usd;
                let ratio = exposure_after.inner() / context.portfolio_total_usd.inner();
                let cap = max_bps.as_decimal();

                if ratio > cap {
                    RuleResult::fail(
                        RuleId::MaxPercentPerAsset,
                        Violation::AssetConcentrationTooHigh {
                            asset: request.asset.clone(),
                            current_plus_requested: exposure_after,
                            max_percent: *max_bps,
                            portfolio_total: context.portfolio_total_usd,
                        },
                        format!(
                            "asset {} exposure {:.1}% exceeds {} cap",
                            request.asset,
                            ratio * rust_decimal::Decimal::ONE_HUNDRED,
                            max_bps,
                        ),
                    )
                } else {
                    RuleResult::pass(
                        RuleId::MaxPercentPerAsset,
                        format!(
                            "asset {} exposure {:.1}% within {} cap",
                            request.asset,
                            ratio * rust_decimal::Decimal::ONE_HUNDRED,
                            max_bps,
                        ),
                    )
                }
            }

            // ── 4. OnlyAuditedProtocols ──────────────────────
            PolicyRule::OnlyAuditedProtocols => {
                if context.audited_protocols.contains(&request.target_protocol) {
                    RuleResult::pass(
                        RuleId::OnlyAuditedProtocols,
                        format!("protocol {} is audited", request.target_protocol),
                    )
                } else {
                    RuleResult::fail(
                        RuleId::OnlyAuditedProtocols,
                        Violation::ProtocolNotAudited {
                            protocol: request.target_protocol.clone(),
                        },
                        format!("protocol {} is not in audited list", request.target_protocol),
                    )
                }
            }

            // ── 5. AllowedProtocols ──────────────────────────
            PolicyRule::AllowedProtocols(allowed) => {
                if allowed.contains(&request.target_protocol) {
                    RuleResult::pass(
                        RuleId::AllowedProtocols,
                        format!("protocol {} is in allowed list", request.target_protocol),
                    )
                } else {
                    RuleResult::fail(
                        RuleId::AllowedProtocols,
                        Violation::ProtocolNotAllowed {
                            protocol: request.target_protocol.clone(),
                        },
                        format!("protocol {} is not in allowed list", request.target_protocol),
                    )
                }
            }

            // ── 6. BlockedActions ────────────────────────────
            PolicyRule::BlockedActions(blocked) => {
                if blocked.contains(&request.action) {
                    RuleResult::fail(
                        RuleId::BlockedActions,
                        Violation::ActionBlocked {
                            action: request.action.clone(),
                            blocked: blocked.clone(),
                        },
                        format!("action {} is blocked", request.action),
                    )
                } else {
                    RuleResult::pass(
                        RuleId::BlockedActions,
                        format!("action {} is permitted", request.action),
                    )
                }
            }

            // ── 7. MaxDailySpend ─────────────────────────────
            PolicyRule::MaxDailySpend(max) => {
                let total_after = context.daily_spend_usd + request.amount_usd;

                if total_after > *max {
                    RuleResult::fail(
                        RuleId::MaxDailySpend,
                        Violation::DailySpendExceeded {
                            current_plus_requested: total_after,
                            max: *max,
                        },
                        format!("daily spend {} would exceed {} limit", total_after, max),
                    )
                } else {
                    RuleResult::pass(
                        RuleId::MaxDailySpend,
                        format!("daily spend {} within {} limit", total_after, max),
                    )
                }
            }

            // ── 8. MinRiskScore ──────────────────────────────
            PolicyRule::MinRiskScore(min) => match context.protocol_risk_score {
                None => RuleResult::indeterminate(
                    RuleId::MinRiskScore,
                    Violation::CannotEvaluate(CannotEvaluateReason::MissingContext(
                        "protocol_risk_score".into(),
                    )),
                    "cannot evaluate risk score: data not available",
                ),
                Some(score) if score < *min => RuleResult::fail(
                    RuleId::MinRiskScore,
                    Violation::RiskScoreTooLow {
                        protocol: request.target_protocol.clone(),
                        score,
                        min_required: *min,
                    },
                    format!(
                        "protocol {} risk score {} below minimum {}",
                        request.target_protocol, score, min,
                    ),
                ),
                Some(score) => RuleResult::pass(
                    RuleId::MinRiskScore,
                    format!(
                        "protocol {} risk score {} meets minimum {}",
                        request.target_protocol, score, min,
                    ),
                ),
            },

            // ── 9. MaxProtocolUtilization ────────────────────
            PolicyRule::MaxProtocolUtilization(max_bps) => match context.protocol_utilization {
                None => RuleResult::indeterminate(
                    RuleId::MaxProtocolUtilization,
                    Violation::CannotEvaluate(CannotEvaluateReason::MissingContext(
                        "protocol_utilization".into(),
                    )),
                    "cannot evaluate utilization: data not available",
                ),
                Some(current) if current > *max_bps => RuleResult::fail(
                    RuleId::MaxProtocolUtilization,
                    Violation::ProtocolUtilizationTooHigh {
                        protocol: request.target_protocol.clone(),
                        current_utilization: current,
                        max_utilization: *max_bps,
                    },
                    format!(
                        "protocol {} utilization {} exceeds {} cap",
                        request.target_protocol, current, max_bps,
                    ),
                ),
                Some(current) => RuleResult::pass(
                    RuleId::MaxProtocolUtilization,
                    format!(
                        "protocol {} utilization {} within {} cap",
                        request.target_protocol, current, max_bps,
                    ),
                ),
            },

            // ── 10. MinProtocolTvl ───────────────────────────
            PolicyRule::MinProtocolTvl(min) => match context.protocol_tvl {
                None => RuleResult::indeterminate(
                    RuleId::MinProtocolTvl,
                    Violation::CannotEvaluate(CannotEvaluateReason::MissingContext(
                        "protocol_tvl".into(),
                    )),
                    "cannot evaluate TVL: data not available",
                ),
                Some(tvl) if tvl < *min => RuleResult::fail(
                    RuleId::MinProtocolTvl,
                    Violation::ProtocolTvlTooLow {
                        protocol: request.target_protocol.clone(),
                        current_tvl: tvl,
                        min_tvl: *min,
                    },
                    format!(
                        "protocol {} TVL {} below {} minimum",
                        request.target_protocol, tvl, min,
                    ),
                ),
                Some(tvl) => RuleResult::pass(
                    RuleId::MinProtocolTvl,
                    format!(
                        "protocol {} TVL {} meets {} minimum",
                        request.target_protocol, tvl, min,
                    ),
                ),
            },
        }
    }

    pub fn id(&self) -> RuleId {
        match self {
            PolicyRule::MaxTransactionAmount(_) => RuleId::MaxTransactionAmount,
            PolicyRule::MaxPercentPerProtocol(_) => RuleId::MaxPercentPerProtocol,
            PolicyRule::MaxPercentPerAsset(_) => RuleId::MaxPercentPerAsset,
            PolicyRule::OnlyAuditedProtocols => RuleId::OnlyAuditedProtocols,
            PolicyRule::AllowedProtocols(_) => RuleId::AllowedProtocols,
            PolicyRule::BlockedActions(_) => RuleId::BlockedActions,
            PolicyRule::MaxDailySpend(_) => RuleId::MaxDailySpend,
            PolicyRule::MinRiskScore(_) => RuleId::MinRiskScore,
            PolicyRule::MaxProtocolUtilization(_) => RuleId::MaxProtocolUtilization,
            PolicyRule::MinProtocolTvl(_) => RuleId::MinProtocolTvl,
        }
    }
}