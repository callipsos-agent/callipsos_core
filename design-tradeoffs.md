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
| **`ReallocationDeltaTooSmall` in `policy/rules`** | TODO: | Add as a policy rule for rate chasing logic | Will come in handy when designing the DeFi agents to prevent agent from churning.
| **`Money` arithmetic in `policy/types`** | Can add basic arithmetic ops for the engine | We'll design the tests first, then add ops when the test demands it | Currently not needed will check back.
|**Action-aware rule filtering** | All rules run for all actions. Math assumes additive (Supply)| Engine filters which rules apply by action type. Withdraw/Transfer skip exposure and spend rules | MVP only supports Supply on Aave/Moonwell. Other actions exist in the enum for forward-compatibility
|**Single-asset `TransactionRequest`** | One `asset:AssetSymbol` field. Works for Supply/Withdraw/Stake.| Add `asset_in` and `asset_out` for `Action::Swap`. `MaxPercentPerAsset` evaluates both sides.| Swaps aren't in MVP scope. Calldata decoding in Phase 2 is when swap fields become meaningful.


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

## Phase 2: Validation Pipeline + Signing (13/03/2026)

Tradeoffs made during Phase 2 for Lit Protocol integration and API completion.

### Deferred to Phase 3+

| Tradeoff | What we did (Phase 2) | What to do later | Why we deferred |
|---|---|---|---|
| **Lit Action code inline vs IPFS** | Send Lit Action JS code inline with every `/core/v1/lit_action` request | Pin to IPFS and reference by CID for immutability guarantees. Register CID in Chipotle group for tighter scoping. | Inline is simpler and avoids IPFS availability dependency. For production, pinned CID proves to users the signing logic hasn't changed. |
| **Placeholder tx hash** | Generate `0x{uuid}` as stand-in tx hash for signing | Sign actual transaction calldata hash once alloy-rs calldata decoding lands | No real on-chain transactions yet. The signing flow works the same — real hash is just a different input. |
| **`signer_address` not populated** | `SigningResult.signer_address` is always `None` | Derive PKP address from public key and include in response | Address derivation requires keccak256 of the uncompressed public key. Not needed for demo — the signature itself proves the PKP signed. |
| **Signing failure is silent** | If Lit API fails, log a warning and return verdict without signature. `signing: null` in response. | Surface signing errors to caller via a `signing_error` field or separate status | For MVP, the policy decision is the priority. Signing is additive. Don't let Lit downtime break the validate endpoint. |
| **No retry on Lit API failure** | Single attempt, fail-open (verdict still returned) | Add retry with backoff for transient Lit API errors | Complexity not justified for MVP. Chipotle dev network may have occasional downtime. |
| **Risk score float precision** | `protocol_risk_score` arrives as f64, converted via `Decimal::from_f64_retain` which produces long decimals (e.g. `0.4000000000000000222044604924`) | Accept risk score as string (like money fields) or round after conversion | Display is correct (rounds to 2dp), only the raw serialized violation shows the noise. Cosmetic issue, not a correctness issue. |
| **Naga → Chipotle migration** | Built directly on Chipotle (Lit v3) REST API. No Naga code exists. | Move to Chipotle production when it launches (~March 25) | Naga is sunsetting April 1. Chipotle dev is live and working. Swap `LIT_API_URL` to production endpoint when available. |
| **No IPFS CID scoping in Chipotle group** | Group has "all actions permitted" flag for simplicity | Register specific IPFS CID in group, scope usage API key to only that action | Tighter security for production. MVP uses inline code so CID scoping doesn't apply yet. |
| **Express sidecar eliminated** | Call Chipotle REST API directly from Rust via reqwest. No `lit-signer/` TS service. | N/A — this is the final architecture | Chipotle's REST API made the sidecar unnecessary. Fewer moving parts, one language, one process. |

### Decisions we're keeping

| Decision | Why it's right |
|---|---|
| **`SigningProvider` trait abstraction** | `LitSigningProvider` today, could swap to any other signing backend (ZeroDev, Ika, local HSM) without touching the validate route. Trait takes `&PolicyVerdict` + tx hash, returns `SigningResult`. |
| **Signing is optional (`Option<Arc<dyn SigningProvider>>`)** | Server starts and works without Lit configured. All Phase 1 tests pass with `signing_provider: None`. No env vars required for development. |
| **Signing only on approved verdicts** | Blocked verdicts never touch the Lit API. The PKP physically cannot sign a transaction that Callipsos rejected. This is the core security guarantee. |
| **Lit Action double-checks the verdict** | The Lit Action independently verifies `decision === 'approved'` and no failed rules before signing. Belt-and-suspenders — even if the Rust code has a bug, the TEE won't sign a bad verdict. |
| **`ValidateResponse` uses `#[serde(flatten)]` on `PolicyVerdict`** | Keeps the existing `decision`, `results`, `engine_reason` fields at the top level. Adding `signing` alongside them is non-breaking — Phase 1 consumers see the same shape plus a new nullable field. |
| **Inline Lit Action code over IPFS** | Matches how Chipotle's own SDK (`litAction` method) sends code. Avoids IPFS pinning setup, gateway availability issues, and extra dashboard config. Code is ~30 lines and deterministic. |

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
| Add `MaxPositionsExceeded` Violation in `policy/types`  → A cap on simultaneous positions a user can have | Users want Vaults and LPs | User feedback demands it |