use crate::policy::rules::PolicyRule;
use crate::policy::types::{
    Decision, EngineReason, EvaluationContext, PolicyVerdict, RuleOutcome,
    TransactionRequest,
};

// TODO (Cyndie): Filter rules by action type. Exposure and spend rules
// only apply to additive actions (Supply, Stake). Withdraw and
// Transfer reduce or move risk and should skip these checks.

/// Evaluates all rules against a transaction request and context.
/// Returns a single PolicyVerdict with the aggregated decision.
///
/// Decision logic:
/// - No rules → Blocked (fail-closed, with EngineReason)
/// - Any Fail or Indeterminate → Blocked
/// - All Pass → Approved
pub fn evaluate(
    rules: &[PolicyRule],
    request: &TransactionRequest,
    context: &EvaluationContext,
) -> PolicyVerdict {
    if rules.is_empty() {
        return PolicyVerdict::blocked_by_engine(EngineReason::NoPoliciesConfigured);
    }

    let results: Vec<_> = rules
        .iter()
        .map(|rule| rule.evaluate(request, context))
        .collect();

    let blocked = results
        .iter()
        .any(|r| matches!(r.outcome(), RuleOutcome::Fail | RuleOutcome::Indeterminate));

    PolicyVerdict {
        decision: if blocked { Decision::Blocked } else { Decision::Approved },
        results,
        engine_reason: None,
    }
}