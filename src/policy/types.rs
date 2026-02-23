use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    pub fn inner(&self) -> Decimal {
        self.0
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

#[derive(Debug, Clone, thiserror::Error)]
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

#[derive(Debug, Clone, thiserror::Error)]
pub enum BasisPointsError {
    #[error("Basis points out of range (0-10000): {0}")]
    OutOfRange(u32),
}