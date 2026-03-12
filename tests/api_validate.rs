mod common;

use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────

async fn create_test_user(app: &common::TestApp) -> String {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/v1/users", app.addr))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    body["id"].as_str().unwrap().to_string()
}

async fn create_policy_with_preset(app: &common::TestApp, user_id: &str, preset: &str) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": format!("{} policy", preset),
            "preset": preset
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
}

async fn create_policy_with_rules(
    app: &common::TestApp,
    user_id: &str,
    name: &str,
    rules: serde_json::Value,
) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": name,
            "rules": rules
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
}

/// Safe $30 supply to aave-v3 on a $10k portfolio with all context populated.
fn safe_validate_body(user_id: &str) -> serde_json::Value {
    json!({
        "user_id": user_id,
        "target_protocol": "aave-v3",
        "action": "supply",
        "asset": "USDC",
        "amount_usd": "30.00",
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
    })
}

// ── 1. Safe transaction → approved ──────────────────────────

#[tokio::test]
async fn validate_safe_transaction_approved() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    create_policy_with_preset(&app, &user_id, "safety_first").await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/v1/validate", app.addr))
        .json(&safe_validate_body(&user_id))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["decision"], "approved");
    assert!(body["engine_reason"].is_null());

    let results = body["results"].as_array().unwrap();
    assert!(!results.is_empty());
    for result in results {
        assert_eq!(result["outcome"], "pass");
    }
}

// ── 2. Over amount limit → blocked ─────────────────────────

#[tokio::test]
async fn validate_over_amount_limit_blocked() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    create_policy_with_preset(&app, &user_id, "safety_first").await;

    let mut body = safe_validate_body(&user_id);
    body["amount_usd"] = json!("600.00"); // safety_first limit is $500

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/v1/validate", app.addr))
        .json(&body)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["decision"], "blocked");

    let results = body["results"].as_array().unwrap();
    let failed: Vec<&serde_json::Value> = results
        .iter()
        .filter(|r| r["outcome"] != "pass")
        .collect();
    assert!(!failed.is_empty());

    let failed_rules: Vec<&str> = failed
        .iter()
        .map(|r| r["rule"].as_str().unwrap())
        .collect();
    assert!(failed_rules.contains(&"max_transaction_amount"));
}

// ── 3. Unaudited protocol → blocked ────────────────────────

#[tokio::test]
async fn validate_unaudited_protocol_blocked() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    create_policy_with_preset(&app, &user_id, "safety_first").await;

    let mut body = safe_validate_body(&user_id);
    body["target_protocol"] = json!("shady-yield");

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/v1/validate", app.addr))
        .json(&body)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["decision"], "blocked");

    let results = body["results"].as_array().unwrap();
    let failed_rules: Vec<&str> = results
        .iter()
        .filter(|r| r["outcome"] != "pass")
        .map(|r| r["rule"].as_str().unwrap())
        .collect();
    assert!(failed_rules.contains(&"only_audited_protocols"));
}

// ── 4. No policies → blocked with engine_reason ─────────────

#[tokio::test]
async fn validate_no_policies_blocked() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    // No policy created

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/v1/validate", app.addr))
        .json(&safe_validate_body(&user_id))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["decision"], "blocked");
    assert_eq!(body["engine_reason"], "no_policies_configured");
    assert!(body["results"].as_array().unwrap().is_empty());
}

// ── 5. Nonexistent user → 404 ──────────────────────────────

#[tokio::test]
async fn validate_nonexistent_user_returns_404() {
    let app = common::spawn_app().await;
    let fake_id = uuid::Uuid::new_v4().to_string();

    let mut body = safe_validate_body(&fake_id);
    body["user_id"] = json!(fake_id);

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/v1/validate", app.addr))
        .json(&body)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 404);
}

// ── 6. Two policies → all rules evaluated ───────────────────

#[tokio::test]
async fn validate_two_policies_evaluates_all_rules() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;

    // Policy A: safety_first preset (9 rules)
    create_policy_with_preset(&app, &user_id, "safety_first").await;

    // Policy B: custom with 2 rules
    create_policy_with_rules(
        &app,
        &user_id,
        "extra rules",
        json!([
            { "MaxTransactionAmount": "1000" },
            "OnlyAuditedProtocols"
        ]),
    )
    .await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/v1/validate", app.addr))
        .json(&safe_validate_body(&user_id))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    let results = body["results"].as_array().unwrap();
    // safety_first has 9 rules + 2 custom = 11 total
    assert_eq!(results.len(), 11);
}

// ── 7. Validate logs to transaction_log ─────────────────────

#[tokio::test]
async fn validate_logs_to_transaction_log() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    create_policy_with_preset(&app, &user_id, "safety_first").await;

    let client = reqwest::Client::new();
    client
        .post(format!("{}/api/v1/validate", app.addr))
        .json(&safe_validate_body(&user_id))
        .send()
        .await
        .expect("Failed to send request");

    // Query transaction_log directly via DB
    let user_uuid: uuid::Uuid = user_id.parse().unwrap();
    let logs = callipsos_core::db::transaction_log::TransactionLogRow::find_by_user(
        &app.db,
        callipsos_core::policy::types::UserId::from(user_uuid),
    )
    .await
    .expect("Failed to query transaction log");

    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].verdict, "approved");
    assert!(logs[0].policy_id.is_none());
    assert!(logs[0].request_json.is_object());
    assert!(logs[0].reasons_json.is_array());
}