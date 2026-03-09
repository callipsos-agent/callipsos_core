use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt::{self};
use std::ops::Deref;
use uuid::Uuid;

// ── UserId (UUID wrapped) ──────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize, Deserialize)]
#[sqlx(transparent)]
pub struct UserId(pub Uuid);

impl Deref for UserId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for UserId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

// ── ProtocolId ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProtocolId(String);

impl ProtocolId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into().to_lowercase())
    }
}

impl Deref for ProtocolId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for ProtocolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

// ── AssetSymbol ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetSymbol(String);

impl AssetSymbol {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into().to_uppercase())
    }
}

impl Deref for AssetSymbol {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for AssetSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

// ── Action ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Supply,
    Borrow,
    Swap,
    Transfer,
    Withdraw,
    Stake,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Action::Supply => "supply",
            Action::Borrow => "borrow",
            Action::Swap => "swap",
            Action::Transfer => "transfer",
            Action::Withdraw => "withdraw",
            Action::Stake => "stake",
        };
        write!(f, "{s}")
    }
}

// ── Money ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(Decimal);

impl Money {
    pub fn try_new(amount: Decimal) -> Result<Self, MoneyError> {
        if amount < Decimal::ZERO {
            return Err(MoneyError::Negative(amount));
        }
        Ok(Self(amount))
    }

    pub fn zero() -> Self {
        Self(Decimal::ZERO)
    }

    pub fn is_zero(&self) -> bool {
    self.0.is_zero()
    }

    pub fn inner(&self) -> Decimal {
        self.0
    }
}

impl std::ops::Add for Money {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        // Safe: two non-negative values always sum to non-negative
        Self(self.0 + rhs.0)
    }
}

impl Deref for Money {
    type Target = Decimal;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}", self.0.round_dp(2))
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum MoneyError {
    #[error("Money cannot be negative: {0}")]
    Negative(Decimal),
}

// ── BasisPoints ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BasisPoints(u32);

impl BasisPoints {
    /// Creates BasisPoints validated to 0..=10000 (0% to 100%).
    pub fn new_checked(value: u32) -> Result<Self, BasisPointsError> {
        if value > 10_000 {
            return Err(BasisPointsError::OutOfRange(value));
        }
        Ok(Self(value))
    }
    /// Convenience constructor from a whole percentage.
    /// `BasisPoints::from_percent(10)` → 1000 bps (10%)
    pub fn from_percent(pct: u32) -> Result<Self, BasisPointsError> {
        Self::new_checked(pct * 100)
    }

    pub fn new_unchecked(value: u32) -> Self {
        Self(value)
    }

    pub fn as_decimal(&self) -> Decimal {
        Decimal::from(self.0) / Decimal::from(10_000)
    }

    pub fn inner(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for BasisPoints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pct = Decimal::from(self.0) / Decimal::from(100);
        write!(f, "{pct}%")
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum BasisPointsError {
    #[error("Basis points out of range (0-10000): {0}")]
    OutOfRange(u32),
}

// ── RiskScore ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RiskScore(Decimal);

impl RiskScore {
    /// Risk score clamped to [0.0, 1.0]
    pub fn try_new(value: Decimal) -> Result<Self, RiskScoreError> {
        if value < Decimal::ZERO || value > Decimal::ONE {
            return Err(RiskScoreError::OutOfRange(value));
        }
        Ok(Self(value))
    }

    pub fn inner(&self) -> Decimal {
        self.0
    }
}

impl Deref for RiskScore {
    type Target = Decimal;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for RiskScore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.round_dp(2))
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum RiskScoreError {
    #[error("Risk score must be between 0.0 and 1.0: {0}")]
    OutOfRange(Decimal),
}

// ── RuleId ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RuleId {
    MaxTransactionAmount,
    MaxPercentPerProtocol,
    MaxPercentPerAsset,
    OnlyAuditedProtocols,
    AllowedProtocols,
    BlockedActions,
    MaxDailySpend,
    MinRiskScore,
    MaxProtocolUtilization,
    MinProtocolTvl,

}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            RuleId::MaxTransactionAmount => "max_transaction_amount",
            RuleId::MaxPercentPerProtocol => "max_percent_per_protocol",
            RuleId::MaxPercentPerAsset => "max_percent_per_asset",
            RuleId::OnlyAuditedProtocols => "only_audited_protocols",
            RuleId::AllowedProtocols => "allowed_protocols",
            RuleId::BlockedActions => "blocked_actions",
            RuleId::MaxDailySpend => "max_daily_spend",
            RuleId::MinRiskScore => "min_risk_score",
            RuleId::MaxProtocolUtilization => "max_protocol_utilization",
            RuleId::MinProtocolTvl => "min_protocol_tvl"
        };
        write!(f, "{s}")
    }
}

// ── Violations (Cannot Evaluate Reason newtype) ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CannotEvaluateReason {
    PortfolioTotalZero,
    MissingContext(String),
}

impl fmt::Display for CannotEvaluateReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CannotEvaluateReason::PortfolioTotalZero => write!(f, "portfolio total is zero"),
            CannotEvaluateReason::MissingContext(ctx) => write!(f, "missing context: {ctx}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Violation {
    TxAmountTooHigh {
        requested: Money,
        max: Money,
    },
    ProtocolExposureTooHigh {
        current_plus_requested: Money,
        max_percent: BasisPoints,
        portfolio_total: Money,
    },
    AssetConcentrationTooHigh {
    asset: AssetSymbol,
    current_plus_requested: Money,
    max_percent: BasisPoints,
    portfolio_total: Money,
    },
    RiskScoreTooLow {
    protocol: ProtocolId,
    score: RiskScore,
    min_required: RiskScore,
    },
    ProtocolNotAudited {
        protocol: ProtocolId,
    },
    ProtocolNotAllowed {
        protocol: ProtocolId,
    },
    ProtocolUtilizationTooHigh {
    protocol: ProtocolId,
    current_utilization: BasisPoints,
    max_utilization: BasisPoints,
    },
    ProtocolTvlTooLow {
    protocol: ProtocolId,
    current_tvl: Money,
    min_tvl: Money,
    },
    ActionBlocked {
        action: Action,
        blocked: Vec<Action>,
    },
    DailySpendExceeded {
        current_plus_requested: Money,
        max: Money,
    },
    CannotEvaluate(CannotEvaluateReason),
    // TODO (Cyndie): Add MaxPositionsExceeded, a cap on how many simulatenous positions a user can have. Will come in handy when we add Vaults and LP yield strategies post MVP
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::TxAmountTooHigh { requested, max } => {
                write!(f, "transaction amount {requested} exceeds max {max}")
            }
            Violation::ProtocolExposureTooHigh { current_plus_requested, max_percent, portfolio_total } => {
                write!(f, "protocol exposure {current_plus_requested} exceeds {max_percent} of portfolio {portfolio_total}")
            }
            Violation::AssetConcentrationTooHigh { asset, current_plus_requested, max_percent, portfolio_total } => {
                write!(f, "asset {asset} concentration {current_plus_requested} exceeds {max_percent} of portfolio {portfolio_total}")
            }
            Violation::RiskScoreTooLow { protocol, score, min_required } => {
                write!(f, "protocol {protocol} risk score {score} is below minimum {min_required}")
            }
            Violation::ProtocolNotAudited { protocol } => {
                write!(f, "protocol {protocol} is not audited")
            }
            Violation::ProtocolNotAllowed { protocol } => {
                write!(f, "protocol {protocol} is not in allowed list")
            }
            Violation::ProtocolUtilizationTooHigh { protocol, current_utilization, max_utilization } => {
                write!(f, "protocol {protocol} utilization {current_utilization} exceeds max {max_utilization}")
            }
            Violation::ProtocolTvlTooLow { protocol, current_tvl, min_tvl } => {
                write!(f, "protocol {protocol} TVL {current_tvl} is below minimum {min_tvl}")
            }
            Violation::ActionBlocked { action, .. } => {
                write!(f, "action {action} is blocked")
            }
            Violation::DailySpendExceeded { current_plus_requested, max } => {
                write!(f, "daily spend {current_plus_requested} exceeds max {max}")
            }
            Violation::CannotEvaluate(reason) => {
                write!(f, "cannot evaluate: {reason}")
            }
        }
    }
}

// ── RuleOutcome + RuleResult ────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleOutcome {
    Pass,
    Fail,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize,  Deserialize)]
pub struct RuleResult {
    rule: RuleId,
    outcome: RuleOutcome,
    violation: Option<Violation>,
    message: String,
}

impl RuleResult {
    pub fn pass(rule: RuleId, msg: impl Into<String>) -> Self {
        Self {
            rule,
            outcome: RuleOutcome::Pass,
            violation: None,
            message: msg.into(),
        }
    }

    pub fn fail(rule: RuleId, violation: Violation, msg: impl Into<String>) -> Self {
        Self {
            rule,
            outcome: RuleOutcome::Fail,
            violation: Some(violation),
            message: msg.into(),
        }
    }

    pub fn indeterminate(rule: RuleId, violation: Violation, msg: impl Into<String>) -> Self {
        Self {
            rule,
            outcome: RuleOutcome::Indeterminate,
            violation: Some(violation),
            message: msg.into(),
        }
    }

    pub fn rule(&self) -> &RuleId {
        &self.rule
    }

    pub fn outcome(&self) -> &RuleOutcome {
        &self.outcome
    }

    pub fn violation(&self) -> Option<&Violation> {
        self.violation.as_ref()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

// ── TransactionRequest ──────────────────────────────────────

// TODO (Cyndie): For Action::Swap, add asset_in and asset_out fields.
// Current single-asset model only works for Supply/Withdraw/Stake.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRequest {
    pub user_id: UserId,
    /// Declared intent. Will be verified against decoded calldata in Phase 2.
    pub target_protocol: ProtocolId,
    pub action: Action,
    pub asset: AssetSymbol,
    pub amount_usd: Money,
    pub target_address: String,
}

// ── EvaluationContext ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationContext {
    pub portfolio_total_usd: Money,
    pub current_protocol_exposure_usd: Money,
    pub current_asset_exposure_usd: Money,
    pub daily_spend_usd: Money,
    pub audited_protocols: Vec<ProtocolId>,
    pub protocol_risk_score: Option<RiskScore>,
    pub protocol_utilization: Option<BasisPoints>,
    pub protocol_tvl: Option<Money>,
}

// ── EngineReason ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineReason {
    NoPoliciesConfigured,
}

impl fmt::Display for EngineReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineReason::NoPoliciesConfigured => {
                write!(f, "no policies configured — set policies before transacting")
            }
        }
    }
}

// ── Decision + PolicyVerdict ────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Approved,
    Blocked,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyVerdict {
    pub decision: Decision,
    pub results: Vec<RuleResult>,
    pub engine_reason: Option<EngineReason>,
}

impl PolicyVerdict {
    pub fn blocked_by_engine(reason: EngineReason) -> Self {
        Self {
            decision: Decision::Blocked,
            results: vec![],
            engine_reason: Some(reason),
        }
    }
    /// Returns results where outcome is Fail or Indeterminate.
    /// Includes indeterminate results because the safe default is to treat
    /// inability to evaluate as a failure.
    pub fn failed_rules(&self) -> Vec<&RuleResult> {
        self.results
            .iter()
            .filter(|r| matches!(r.outcome, RuleOutcome::Fail | RuleOutcome::Indeterminate))
            .collect()
    }
}

