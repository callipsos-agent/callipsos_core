#!/bin/bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Callipsos Demo Script
# Run with: ./demo.sh
# Requires: server running on localhost:3000, jq installed
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

API="http://127.0.0.1:3000"
BOLD='\033[1m'
CYAN='\033[36m'
GREEN='\033[32m'
RED='\033[31m'
YELLOW='\033[33m'
DIM='\033[2m'
RESET='\033[0m'

pause() {
    echo ""
    echo -e "${DIM}Press Enter to continue...${RESET}"
    read -r
}

echo ""
echo -e "${CYAN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo -e "${CYAN}${BOLD}  CALLIPSOS — Policy Engine Demo${RESET}"
echo -e "${CYAN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"

# ── Step 1: Create User ──────────────────────────────────────
pause
echo -e "${BOLD}Step 1: Create a user${RESET}"
echo -e "${DIM}POST /api/v1/users${RESET}"
echo ""

USER_RESPONSE=$(curl -s -X POST "$API/api/v1/users" \
    -H "Content-Type: application/json" \
    -d '{}')

echo "$USER_RESPONSE" | jq .

USER_ID=$(echo "$USER_RESPONSE" | jq -r '.id')
echo ""
echo -e "${GREEN}✓ User created: ${USER_ID}${RESET}"

# ── Step 2: Create Policy ────────────────────────────────────
pause
echo -e "${BOLD}Step 2: Apply safety_first policy${RESET}"
echo -e "${DIM}POST /api/v1/policies (preset: safety_first)${RESET}"
echo ""

curl -s -X POST "$API/api/v1/policies" \
    -H "Content-Type: application/json" \
    -d "{
        \"user_id\": \"$USER_ID\",
        \"name\": \"safety_first\",
        \"preset\": \"safety_first\"
    }" | jq .

echo ""
echo -e "${GREEN}✓ Policy applied: safety_first${RESET}"
echo -e "${DIM}  Rules: max \$500/tx, max \$1000/day, 10% per protocol,${RESET}"
echo -e "${DIM}  audited only, no borrow/swap/transfer, risk score ≥ 0.80${RESET}"

# ── Step 3: Approved Transaction ─────────────────────────────
pause
echo -e "${CYAN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo -e "${BOLD}Step 3: Submit a SAFE transaction${RESET}"
echo ""
echo -e "${GREEN}  Transaction details:${RESET}"
echo -e "  ├── Action:   ${BOLD}supply${RESET} (deposit to earn yield)"
echo -e "  ├── Amount:   ${BOLD}\$30 USDC${RESET} (well under \$500 limit)"
echo -e "  ├── Protocol: ${BOLD}Aave V3${RESET} (audited, \$500M TVL, 4.2% APY)"
echo -e "  ├── Risk:     ${BOLD}0.90${RESET} (above 0.80 minimum)"
echo -e "  └── Daily:    ${BOLD}\$0 spent so far${RESET} (under \$1000 limit)"
echo ""
echo -e "${DIM}This transaction should pass every rule. Sending...${RESET}"
echo ""

curl -s -X POST "$API/api/v1/validate" \
    -H "Content-Type: application/json" \
    -d "{
        \"user_id\": \"$USER_ID\",
        \"target_protocol\": \"aave-v3\",
        \"action\": \"supply\",
        \"asset\": \"USDC\",
        \"amount_usd\": \"30\",
        \"target_address\": \"0x1234\",
        \"context\": {
            \"portfolio_total_usd\": \"10000\",
            \"current_protocol_exposure_usd\": \"0\",
            \"current_asset_exposure_usd\": \"0\",
            \"daily_spend_usd\": \"0\",
            \"audited_protocols\": [\"aave-v3\", \"moonwell\"],
            \"protocol_risk_score\": 0.90,
            \"protocol_utilization_pct\": 0.50,
            \"protocol_tvl_usd\": \"500000000\"
        }
    }" | jq .

echo ""
echo -e "${GREEN}${BOLD}✅ APPROVED — All 9 rules passed. Signed by Lit PKP.${RESET}"

# ── Step 4: Blocked Transaction ──────────────────────────────
pause
echo -e "${CYAN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo -e "${BOLD}Step 4: Submit a DANGEROUS transaction${RESET}"
echo ""
echo -e "${RED}  Transaction details:${RESET}"
echo -e "  ├── Action:      ${BOLD}borrow${RESET} (leverage — blocked by policy)"
echo -e "  ├── Amount:      ${BOLD}\$5,000 USDC${RESET} (10x over \$500 limit)"
echo -e "  ├── Protocol:    ${BOLD}ShadyYield${RESET} (UNAUDITED, not in allowed list)"
echo -e "  ├── Risk score:  ${BOLD}0.30${RESET} (way below 0.80 minimum)"
echo -e "  ├── Utilization: ${BOLD}95%${RESET} (above 80% cap)"
echo -e "  └── TVL:         ${BOLD}\$1M${RESET} (below \$50M minimum)"
echo ""
echo -e "${DIM}This transaction violates almost every rule. Sending...${RESET}"
echo ""

curl -s -X POST "$API/api/v1/validate" \
    -H "Content-Type: application/json" \
    -d "{
        \"user_id\": \"$USER_ID\",
        \"target_protocol\": \"shady-yield\",
        \"action\": \"borrow\",
        \"asset\": \"USDC\",
        \"amount_usd\": \"5000\",
        \"target_address\": \"0xDEAD\",
        \"context\": {
            \"portfolio_total_usd\": \"10000\",
            \"current_protocol_exposure_usd\": \"0\",
            \"current_asset_exposure_usd\": \"0\",
            \"daily_spend_usd\": \"0\",
            \"audited_protocols\": [\"aave-v3\", \"moonwell\"],
            \"protocol_risk_score\": 0.30,
            \"protocol_utilization_pct\": 0.95,
            \"protocol_tvl_usd\": \"1000000\"
        }
    }" | jq .

echo ""
echo -e "${RED}${BOLD}❌ BLOCKED — Multiple violations detected:${RESET}"
echo -e "${YELLOW}   ├── Amount \$5000 exceeds \$500 limit${RESET}"
echo -e "${YELLOW}   ├── Protocol shady-yield is not audited${RESET}"
echo -e "${YELLOW}   ├── Action borrow is blocked${RESET}"
echo -e "${YELLOW}   ├── Risk score 0.30 below 0.80 minimum${RESET}"
echo -e "${YELLOW}   ├── Utilization 95% exceeds 80% cap${RESET}"
echo -e "${YELLOW}   └── TVL \$1M below \$50M minimum${RESET}"
echo ""
echo -e "${DIM}Signing: null — the PKP was never asked to sign.${RESET}"
echo -e "${DIM}No signature. No execution. Funds safe.${RESET}"

# ── Done ─────────────────────────────────────────────────────
pause
echo -e "${CYAN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo -e "${GREEN}${BOLD}  Policy Engine Demo Complete${RESET}"
echo ""
echo -e "${DIM}  Next: Run the AI agent demo with${RESET}"
echo -e "${BOLD}  cargo run --bin chaos_agent${RESET}"
echo -e "${CYAN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo ""