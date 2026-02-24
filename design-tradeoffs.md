#  Design Tradeoffs & Future Optimizations For Callipsos

> **Living document.** This doc is updated as we build. Each phase records what we chose, what we deferred, and why. When complexity grows, check here first before redesigning.

---

## Phase 1: Foundation + Policy Engine (23/02/2025)

Tradeoffs made during Phase 1 to keep scope tight. Revisit these in later phases.

### Deferred to Phase 2

| Tradeoff | What we did (Phase 1) | What to do later | Why we deferred |
|---|---|---|---|
| **`target_address` typing** | Plain `String` | Replace with alloy `Address` type with proper hex validation | alloy enters the crate in Phase 2. Hand-rolling a newtype now is throwaway work. |
| **Transaction calldata decoding** | `target_protocol` is declared intent from the agent, so we trust the request fields | Decode raw calldata with alloy `sol!` macro, verify target contract is actually the claimed protocol | Phase 2 adds alloy-rs. The policy engine doesn't change, only the validate route gets smarter about where `TransactionRequest` fields come from. |
| **`audited_protocols` as `HashSet`** | `Vec<ProtocolId>` with `.contains()` | Switch to `HashSet<ProtocolId>` for O(1) lookups | We have 3 protocols. O(n) on n=3 is not a bottleneck. Revisit when the allowlist grows past ~20. |
| **Transaction simulation** | No simulation. Policy engine approves/blocks based on rules only. | Add `eth_call` simulation via alloy provider on Base to preview transaction outcomes before execution. | Simulation requires an RPC connection and alloy. Not needed to prove the policy engine works. |

### Deferred to Phase 3+

| Tradeoff | What we did (Phase 1) | What to do later | Why we deferred |
|---|---|---|---|
| **`ChainId` on `TransactionRequest`** | Hardcoded to Base. No chain field. | Add `chain: ChainId(u64)` to `TransactionRequest`. Route allowlists and rule sets per chain. | Single-chain MVP. Adding a field we never read in any rule is dead code. Add when we actually support multiple chains. |
| **Time window rules** | Not implemented | Add `PolicyRule::TimeWindow { start_hour, end_hour, timezone }` — "Only allow transactions between 9am–9pm" | Low complexity, high trust value. Design is clean to add as a new enum variant. Not needed for hackathon demo. |
| **Cooldown / rate limit rules** | Not implemented | Add `PolicyRule::MaxTransactionsPerHour(u32)` — protects against compromised agent loops | Requires tracking tx count per time window in `EvaluationContext`. Easy to add, not needed for initial demo. |
| **Recipient allowlist/blocklist** | Not implemented | Add `PolicyRule::AllowedRecipients(Vec<Address>)` / `BlockedRecipients(Vec<Address>)` | Big for "agent goes rogue" narrative. Needs typed addresses (Phase 2). |
| **NLP → Policy mapping** | Policies set via presets only (safety_first, best_yields, balanced) | Claude function calling extracts structured `PolicyRule` from natural language via Rig | Phase 3 adds Rig + Claude. The policy engine and `rules_json` schema already support this, so only the input method changes. |
| **`primary_reason` on `PolicyVerdict`** | `failed_rules()` helper filters non-passing results | Add severity ranking to rules so verdict can surface the highest-priority violation | Implies a severity system between rules. Not needed when all rules are equally weighted. Add when UI needs "most important reason." |

### Decisions we're keeping

| Decision | Why it's right |
|---|---|
| **Policy engine is purely offchain** | Chain-agnostic, fast iteration, no audit/deploy/gas overhead. Signed verdicts provide on-chain verifiability without on-chain execution. Don't let a partner dictate architecture. |
| **`Money` as `rust_decimal::Decimal`, not `f64`** | Float boundary bugs in financial logic are unacceptable. `0.1 + 0.2 != 0.3` energy. Judges feel it when money logic uses floats. |
| **`BasisPoints(u32)` for percentages** | 10% = 1000 bps. Avoids float precision issues in percentage comparisons. |
| **Structured `Violation` enum over plain strings** | Machine-readable failures enable analytics, UI rendering, and signed attestations. |
| **`RuleResult` constructors enforce invariants** | Impossible to create a Pass with a Violation or a Fail without one. Type system prevents bugs. |
| **`RuleOutcome::Indeterminate` exists** | Most hackathon projects pretend uncertainty doesn't exist. We explicitly handle "can't evaluate" (e.g., portfolio total is zero) and default to blocked. |
| **`OnlyAuditedProtocols` reads from `EvaluationContext`, not hardcoded** | Keeps rules pure and testable. Allowlist can be updated without code changes in the future. |
| **Evaluate all rules, don't short-circuit** | Aggregated results show full breakdown: "Failed 2 rules: daily limit + protocol not audited." Better for trust-building and demos. |

---

## Phase 2: Validation Pipeline + Signing

_To be filled as we build Phase 2._

| Tradeoff | What we chose | What to revisit | Why |
|---|---|---|---|

---

## Phase 3: AI Layer + Conversational Interface

_To be filled as we build Phase 3._

| Tradeoff | What we chose | What to revisit | Why |
|---|---|---|---|

---

## Post-MVP: Scaling & Production

_Tradeoffs that only matter at scale. Don't touch these until product-market fit hypothesis is validated._

| Tradeoff | What to revisit | Trigger |
|---|---|---|
| `Vec` → `HashSet` for protocol lookups (`audited_protocols: Vec<ProtocolId>`) | Allowlist exceeds ~20 entries | Protocol count grows |
| Single-crate Rust → workspace with sub-crates | Module boundaries get painful | Codebase exceeds ~5k lines |
| PostgreSQL → read replicas or caching layer | DB becomes bottleneck on validate endpoint | Sustained >1k req/s |
| Hardcoded yield sources → general yield aggregator | Users want protocols beyond Aave + Moonwell | User feedback demands it |