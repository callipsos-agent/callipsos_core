# Callipsos

**Callipsos translates human intent into cryptographically enforceable guardrails that AI agents must satisfy before they execute transactions and move capital.**

---

**Built for [The Synthesis](https://synthesis.md)** — the first hackathon where AI agents compete as registered participants. March 13-22, 2026.

**Tracks:** Agents that Pay · Agents that Trust

**Team:** Cyndie Kamau (human founder) + [Callipsos Agent](https://basescan.org/tx/0x87fb8ffd527a74ef5120c6836a989e8de4e18938eb17e67c35d10be026c38d4f) (AI participant with ERC-8004 on-chain identity)

---

## TL;DR

- **What:** Policy validation layer for AI agents moving capital in DeFi
- **How:** NLP → structured rules → pure Rust engine → cryptographic signing in TEE
- **Why:** Agents get autonomy within user-defined boundaries. Users get safety guarantees.
- **Demo:** `cargo run --bin chaos_agent` — watch an agent try to maximize yields and get constrained by policy
- **Stack:** Rust + Claude + Lit Protocol + Base

**Built for [The Synthesis](https://synthesis.md) hackathon (March 13-22, 2026) in collaboration with Callipsos Agent ([ERC-8004 on Base Mainnet](https://basescan.org/tx/0x87fb8ffd527a74ef5120c6836a989e8de4e18938eb17e67c35d10be026c38d4f)).**

---

We solve two problems:

**How can AI agents maintain autonomy?** Agents need freedom to discover yield opportunities, react to market conditions, and execute strategies without human approval on every action. Restricting them to pre-approved transaction lists kills the value of having an agent.

**How can humans trust AI with their money?** An unrestricted agent with access to your wallet is a liability. One bad decision — an unaudited protocol, an overleveraged position, a concentrated bet — and your capital is gone.

Callipsos resolves this tension. Users define safety boundaries in plain English. The agent operates freely within those boundaries. Every transaction is validated against a policy engine before execution, and approved transactions are signed inside a Trusted Execution Environment by a key that physically cannot produce a signature for a rejected transaction.

Built in Rust. Deployed on Base. Signed by Lit Protocol.

---

## The Problem

AI agents managing DeFi positions can lose your money in seconds. An agent chasing 15% APY on an unaudited protocol, taking leveraged positions, or concentrating your entire portfolio in one place — these aren't hypothetical risks. They're the default behavior of yield-maximizing agents without constraints.

Existing solutions require trusting a centralized service to enforce rules. Callipsos removes that trust requirement: policy enforcement happens in your backend, and transaction signing happens inside Lit Protocol's Trusted Execution Environment. Nobody — not even Callipsos — can sign a transaction the policy engine rejected.

## The Solution

Users describe their safety preferences in plain English:

> "Only spend up to $200 per day, only use audited protocols, and I want low-risk yields only."

The Callipsos agent translates this into concrete policy rules (transaction limits, protocol allowlists, action restrictions, risk score minimums), stores them, and enforces them against every transaction attempt. When the agent tries to interact with DeFi protocols, each transaction passes through:

1. **Policy Engine** — 10 configurable rules evaluated against the transaction
2. **Verdict** — Approved or Blocked, with detailed reasons for every rule
3. **Lit Protocol Signing** — Approved verdicts are signed inside a TEE by a PKP (Programmable Key Pair). Blocked verdicts never reach the signing layer
4. **Audit Log** — Every attempt logged to PostgreSQL with full context

---

## Architecture

```
User (plain English)
    │
    ▼
┌─────────────────────────────────────────────┐
│  Rig Agent (Claude Sonnet)                  │
│                                             │
│  Tools:                                     │
│    set_policy  → NLP to structured rules    │
│    validate_tx → submit tx for approval     │
└──────────────┬──────────────────────────────┘
               │ HTTP
               ▼
┌─────────────────────────────────────────────┐
│  Callipsos Core API (Rust / axum)           │
│                                             │
│  POST /api/v1/validate                      │
│    1. Load user's active policies from DB   │
│    2. Deserialize rules (Vec<PolicyRule>)    │
│    3. Evaluate all rules against tx request  │
│    4. If approved → sign via Lit Protocol    │
│    5. Log everything to transaction_log      │
│    6. Return verdict + signature             │
│                                             │
│  POST /api/v1/policies                      │
│    • Create from preset (safety_first,      │
│      balanced, best_yields)                 │
│    • Create from custom rules JSON          │
│                                             │
│  POST /api/v1/users                         │
│  GET  /api/v1/policies?user_id=<uuid>       │
│  DELETE /api/v1/policies/:id                │
└──────────────┬──────────────────────────────┘
               │
        ┌──────┴──────┐
        ▼             ▼
┌──────────────┐ ┌─────────────────────────┐
│  PostgreSQL  │ │  Lit Protocol (Chipotle) │
│              │ │                          │
│  users       │ │  POST /core/v1/lit_action│
│  policies    │ │  • Validates verdict     │
│  tx_log      │ │    inside TEE            │
└──────────────┘ │  • Signs with PKP via    │
                 │    getPrivateKey + ethers │
                 │  • Returns ECDSA sig     │
                 └─────────────────────────┘
```

---

## Policy Engine

The policy engine is pure Rust. No database calls, no HTTP, no side effects. It takes a list of rules, a transaction request, and an evaluation context, and returns a verdict.

### 10 Policy Rules

| Rule | What it checks | Example |
|---|---|---|
| MaxTransactionAmount | Single tx size limit | $500 max per transaction |
| MaxDailySpend | Cumulative daily spending | $1,000 per day |
| MaxPercentPerProtocol | Concentration in one protocol | Max 10% of portfolio in Aave |
| MaxPercentPerAsset | Concentration in one asset | Max 30% in USDC |
| OnlyAuditedProtocols | Protocol must be in audited list | Block ShadyYield |
| AllowedProtocols | Explicit protocol allowlist | Only Aave and Moonwell |
| BlockedActions | Prevent specific action types | No borrowing, no transfers |
| MinRiskScore | Protocol risk floor (0.0-1.0) | Only protocols scoring 0.8+ |
| MaxProtocolUtilization | Protocol utilization ceiling | Skip if utilization > 80% |
| MinProtocolTvl | Minimum TVL requirement | Only protocols with $50M+ TVL |

### Decision Logic

- No rules configured → **Blocked** (fail-closed)
- Any rule fails or is indeterminate → **Blocked**
- All rules pass → **Approved**

Every rule is evaluated. No short-circuiting. The verdict includes results for all rules so the agent (and user) can see exactly what passed and what failed.

### 3 Presets

| Preset | Max Tx | Daily Limit | Protocol Cap | Blocked Actions |
|---|---|---|---|---|
| safety_first | $500 | $1,000 | 10% | Borrow, Swap, Transfer |
| balanced | $2,000 | $5,000 | 25% | Borrow, Transfer |
| best_yields | $5,000 | $10,000 | 40% | Transfer |

---

## NLP Policy Mapping

Users don't write JSON. They describe preferences in natural language:

> "max $200 per day, only audited protocols, low-risk yields"

The Rig agent (powered by Claude) extracts structured parameters and calls the `set_policy` tool, which maps to `PolicyRule` variants:

- "max $200 per day" → `MaxDailySpend("200")`
- "only audited protocols" → `OnlyAuditedProtocols`
- "low-risk" → `MinRiskScore("0.80")` + `BlockedActions(["borrow", "transfer"])` + conservative concentration limits

The tool validates all inputs (action names normalized to lowercase, percentages capped at 100, risk scores within 0.0-1.0, no negative amounts) before creating the policy via the API.

---

## Lit Protocol Integration

Callipsos uses Lit Protocol's Chipotle REST API for transaction signing. The signing flow:

1. Policy engine approves a transaction
2. Callipsos serializes the verdict as JSON
3. Sends it to Lit's TEE via `POST /core/v1/lit_action`
4. Inside the TEE, a Lit Action:
   - Parses the verdict
   - Verifies `decision === 'approved'` and no failed rules
   - Retrieves the PKP private key via `getPrivateKey`
   - Signs the transaction digest with `ethers.SigningKey`
5. Returns a 65-byte ECDSA signature

The PKP (Programmable Key Pair) cannot sign a transaction that Callipsos rejected. The Lit Action independently verifies the verdict before signing — belt and suspenders. Even if the Rust code has a bug, the TEE won't sign a bad verdict.

Signing is optional. If Lit is unavailable, the verdict still returns and the transaction log still records. The policy decision is the priority; signing is additive proof.

### Environment Variables

```
LIT_API_URL=https://api.dev.litprotocol.com
LIT_API_KEY=<usage API key from Chipotle Dashboard>
LIT_PKP_ADDRESS=<PKP wallet address>
```

All three must be set to enable signing. If any is missing, the server starts normally with signing disabled.

---

## Chaos Agent Demo

The chaos agent is a Rig-powered AI agent that demonstrates the full Callipsos pipeline. It uses Claude Sonnet as the LLM with two tools:

- **set_policy** — Creates safety policies from natural language
- **validate_transaction** — Submits DeFi transactions for policy validation

### Running the Demo

Terminal 1 — Start the server:
```bash
cargo run
```

Terminal 2 — Run the chaos agent:
```bash
cargo run --bin chaos_agent
```

### What Happens

1. Agent creates a test user via the API
2. User's safety preferences (in the prompt) are parsed into policy rules
3. Agent calls `set_policy` to create the policy
4. Agent attempts multiple DeFi transactions:
   - Supply to Aave V3 (audited, should pass within limits)
   - Supply to Moonwell (audited, may hit daily limit)
   - Supply to ShadyYield (unaudited, blocked)
   - Borrow on Aave (blocked action)
   - Large transactions (over amount limit)
5. Each attempt shows colored terminal output: green for approved, red for blocked, yellow for violation reasons
6. Approved transactions are signed by the Lit PKP
7. Agent summarizes results: what was approved, what was blocked, total yield achieved

### Sample Output

```
🤖 Callipsos Chaos Agent v1.0 — DeFi Yield Maximizer

Setting up demo environment...
   ✓ Wallet connected: 0c72be4e-f719-4f85-b662-cb0fc6b94735

🔥 Chaos Agent activated. Attempting to maximize yields...

   → Setting policy: Safe & Steady Policy (7 rules)
   ✅ Policy 'Safe & Steady Policy' created with 7 rules
   → POST /validate: 80.00 USDC supply to aave-v3
   ✅ APPROVED — Signed: 0x779ea32d1e1f9c1f...
   → POST /validate: 70.00 USDC supply to moonwell
   ✅ APPROVED — Signed: 0x0a4135c66652dc...
   → POST /validate: 100.00 USDC supply to shady-yield
   ❌ BLOCKED
   ├── protocol shady-yield is not in audited list
   → POST /validate: 300.00 USDC borrow to aave-v3
   ❌ BLOCKED
   ├── action borrow is blocked
   → POST /validate: 50.00 USDC supply to moonwell
   ❌ BLOCKED
   ├── daily spend $200.00 would exceed $200.00 limit
```

---

## Demo

### Video Walkthrough

[Demo video will be added before submission]

### Screenshots

**1. Policy Creation via NLP**
*Agent translates "only audited protocols, max $200/day" into structured PolicyRule JSON*

**2. Transaction Validation**
*Approved transaction with Lit PKP signature (0xd8f89364...)*

**3. Blocked Transaction**
*Transaction blocked with clear violation reasons*

**4. Agent Summary**
*Educational summary showing what passed, what failed, and yield calculations*

---

## API Reference

### POST /api/v1/validate

Validate a transaction against the user's active policies.

**Request:**
```json
{
  "user_id": "uuid",
  "target_protocol": "aave-v3",
  "action": "supply",
  "asset": "USDC",
  "amount_usd": "200.00",
  "target_address": "0x1234",
  "context": {
    "portfolio_total_usd": "10000.00",
    "current_protocol_exposure_usd": "0.00",
    "current_asset_exposure_usd": "0.00",
    "daily_spend_usd": "0.00",
    "audited_protocols": ["aave-v3", "moonwell"],
    "protocol_risk_score": 0.90,
    "protocol_utilization_pct": 0.50,
    "protocol_tvl_usd": "500000000"
  }
}
```

**Response (approved):**
```json
{
  "decision": "Approved",
  "results": [
    { "rule": "MaxTransactionAmount", "outcome": "Pass", "message": "amount $200.00 within $500 limit" },
    { "rule": "OnlyAuditedProtocols", "outcome": "Pass", "message": "protocol aave-v3 is audited" }
  ],
  "engine_reason": null,
  "signing": {
    "signed": true,
    "signature": "0x779ea32d...",
    "signer_address": "0x02cde14eb03ed1fe675fe8e690b88b4891d05080",
    "reason": "Transaction signed by Callipsos-gated PKP"
  }
}
```

**Response (blocked):**
```json
{
  "decision": "Blocked",
  "results": [
    { "rule": "OnlyAuditedProtocols", "outcome": "Fail", "message": "protocol shady-yield is not in audited list" }
  ],
  "engine_reason": null,
  "signing": null
}
```

### POST /api/v1/policies

Create a policy from a preset or custom rules.

**Preset:**
```json
{
  "user_id": "uuid",
  "name": "my policy",
  "preset": "safety_first"
}
```

**Custom rules:**
```json
{
  "user_id": "uuid",
  "name": "my custom policy",
  "rules": [
    { "MaxTransactionAmount": "500" },
    { "MaxDailySpend": "1000" },
    "OnlyAuditedProtocols",
    { "BlockedActions": ["borrow", "transfer"] }
  ]
}
```

### POST /api/v1/users

Create a user. Returns the user object with generated UUID.

### GET /api/v1/policies?user_id=uuid

List active policies for a user.

### DELETE /api/v1/policies/:id

Soft-delete a policy (sets active=false).

### GET /health

Health check. Returns `{"status": "ok"}`.

---

## Tech Stack

| Component | Technology |
|---|---|
| Core API | Rust, axum 0.8 |
| AI Agent | Rig 0.31 + Claude Sonnet |
| Policy Engine | Pure Rust, no dependencies |
| Database | PostgreSQL + sqlx 0.8 |
| Transaction Signing | Lit Protocol Chipotle (TEE) |
| Blockchain Target | Base (EVM) |
| DeFi Protocols | Aave V3, Moonwell |
| Tx Types | alloy-rs 1.7 |

---

## Agent Contribution

Callipsos was built in genuine collaboration with **Callipsos Agent**, a registered participant in [The Synthesis](https://synthesis.md) hackathon with an [ERC-8004 on-chain identity on Base Mainnet](https://basescan.org/tx/0x87fb8ffd527a74ef5120c6836a989e8de4e18938eb17e67c35d10be026c38d4f).

### What the Agent Built

**Code Contributions (visible in git history):**
- Integration tests covering all API endpoints ([PR #7](https://github.com/Callipsos-Network/callipsos_core/pull/7))
- NLP semantic policy mapping (`SetPolicyTool`) ([PR #15](https://github.com/Callipsos-Network/callipsos_core/pull/15))
- Chaos agent demo with Rig framework integration ([PR #15](https://github.com/Callipsos-Network/callipsos_core/pull/15))
- Lit Protocol signing fixes ([PR #16](https://github.com/Callipsos-Network/callipsos_core/pull/16))
- Documentation (architecture, threat model, this README)
- Input validation for all 10 policy rule types
- Conversation log documenting 9 days of collaboration ([docs/conversation-log.md](docs/conversation-log.md))

**Git Evidence:**
- 17+ commits under `callipsos-agent` account
- 6+ pull requests with code reviews and discussions
- All commits tagged with `(agent)` suffix
- Co-authored-by attribution on all agent commits

**The collaboration was genuine:** disagreements on design decisions, bugs caught in review, iterative improvements across multiple sessions. The [conversation log](docs/conversation-log.md) shows the honest process — not theater.

### Agent Identity

- **Name:** Callipsos Agent
- **ERC-8004 ID:** `324e1ebb8668477b99c9c80294d7bcca`
- **Registration Tx:** [0x87fb8f...8d4f on Base Mainnet](https://basescan.org/tx/0x87fb8ffd527a74ef5120c6836a989e8de4e18938eb17e67c35d10be026c38d4f)
- **Model:** Claude Sonnet 4.5 (`claude-sonnet-4-5-20250929`)
- **Harness:** Claude Code (local development environment)
- **Role:** Code reviewer, test writer, documentation builder, demo creator

---

## Project Structure

```
callipsos/
├── src/
│   ├── main.rs                    # Server: config → DB → router → serve
│   ├── lib.rs                     # Crate root
│   ├── error.rs                   # AppError enum
│   ├── db/
│   │   ├── mod.rs                 # PgPool + migrations
│   │   ├── user.rs                # User model + queries
│   │   └── policy.rs              # PolicyRow model + queries
│   ├── routes/
│   │   ├── mod.rs                 # Router + AppState
│   │   ├── health.rs              # GET /health
│   │   ├── users.rs               # POST /api/v1/users
│   │   ├── policies.rs            # Policy CRUD
│   │   └── validate.rs            # POST /api/v1/validate
│   ├── policy/                    # Pure logic. No DB, no HTTP.
│   │   ├── mod.rs
│   │   ├── types.rs               # Domain types
│   │   ├── rules.rs               # PolicyRule enum + evaluate()
│   │   ├── engine.rs              # evaluate(rules, request, context)
│   │   └── presets.rs             # safety_first, balanced, best_yields
│   ├── signing/
│   │   ├── mod.rs                 # SigningProvider trait
│   │   └── lit.rs                 # LitSigningProvider (Chipotle API)
│   └── bin/
│       └── chaos_agent.rs         # Demo binary with Rig agent
├── migrations/
│   └── 001_initial.sql            # users, policies, transaction_log
├── tests/
│   ├── common/mod.rs              # Test harness
│   ├── api_health.rs
│   └── api_users.rs
└── Cargo.toml
```

---

## Setup

### Prerequisites

- Rust (stable)
- PostgreSQL
- Anthropic API key (for the chaos agent)
- Lit Protocol Chipotle API key (optional, for signing)

### 1. Clone and configure

```bash
git clone https://github.com/Callipsos-Network/callipsos_core
cd callipsos_core
cp .env.example .env
# Edit .env with your values
```

### 2. Start PostgreSQL

```bash
docker-compose up -d
```

### 3. Run the server

```bash
cargo run
```

Server starts at `http://127.0.0.1:3000`. Migrations run automatically.

### 4. Run tests

```bash
cargo test
```

### 5. Run the chaos agent demo

```bash
# In a separate terminal (server must be running)
ANTHROPIC_API_KEY=your-key cargo run --bin chaos_agent
```

### 6. Watch the demo

The fastest way to understand Callipsos is to watch the chaos agent demo:
- **Live demo:** [Video will be added before submission]
- **Terminal recording:** [asciinema link will be added]
- **Screenshots:** See Demo section above

Or run it yourself with the steps above!

### Environment Variables

```bash
# Required
DATABASE_URL=postgres://postgres:postgres@localhost:5432/callipsos_dev

# Required for chaos agent
ANTHROPIC_API_KEY=sk-ant-...

# Optional (enables Lit signing)
LIT_API_URL=https://api.dev.litprotocol.com
LIT_API_KEY=<your key>
LIT_PKP_ADDRESS=<your PKP wallet address>

# Optional (chaos agent API target)
CALLIPSOS_API_URL=http://127.0.0.1:3000
```

---

## What We Prevent

- Agent depositing into unaudited/malicious protocols
- Single transaction exceeding user-defined limits
- Over-concentration in one protocol or asset
- Unauthorized action types (borrowing, leveraged positions)
- Exceeding daily spending budgets
- Interaction with low-TVL or high-utilization protocols

## Roadmap: What's Next

Currently scoped for MVP simplicity. Planned additions:

- Calldata decoding (verify declared intent matches raw calldata)
- On-chain portfolio state reads (eliminate trust in context provider)
- Multi-chain support (expand beyond Base)
- Real-time protocol risk scoring (dynamic risk assessment)
- Transaction execution/broadcasting (full end-to-end flow)

## Security Model

The funded amount in the user's wallet is the maximum possible loss. Callipsos reduces that blast radius by enforcing per-transaction, per-day, and per-protocol limits. The Lit PKP adds a hardware-backed guarantee: the signing key cannot produce a signature without Callipsos approval, verified independently inside the TEE.

---

## Vision: Six Layers of Defense

Callipsos is designed as a layered defense system. The MVP implements the first two layers. The architecture is built so each layer strengthens independently as we add it.

```
Layer 1: Policy Engine              ✅ Built
  Translate user-defined rules and limits into enforceable constraints.

Layer 2: Calldata Decoding          🔨 In Progress
  Decode what the transaction actually does. Verify declared intent
  matches the raw calldata before the policy engine evaluates it.

Layer 3: Transaction Simulation     📋 Planned
  Fork-simulate transactions before execution. Predict state changes,
  token flows, and failure conditions before committing real capital.

Layer 4: Cryptographic Attestation  ✅ Built (Lit Protocol)
  Signed, verifiable proof on-chain that every transaction passed
  policy validation. The PKP cannot sign without Callipsos approval.

Layer 5: Behavioural Analysis       📋 Planned
  Detect bad agents by pattern. Anomaly detection on transaction
  sequences, timing, and target selection across an agent's history.

Layer 6: Cross-Agent Intelligence   📋 Planned
  Network-wide threat detection. Share threat signals across Callipsos
  users — if one agent discovers a malicious protocol, all agents learn.
```

Layers 1 and 4 are live. Layer 2 is next. The architecture is designed so each layer slots into the existing validate pipeline without restructuring.

---

## License

MIT
