mod common;

use serde_json::json;

// ── Helper: create a user and return the UUID ───────────────

async fn create_test_user(app: &common::TestApp) -> String {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/v1/users", app.addr))
        .json(&json!({}))
        .send()
        .await
        .expect("Failed to create user");

    let body: serde_json::Value = response.json().await.unwrap();
    body["id"].as_str().unwrap().to_string()
}

// ── 1. Create policy with preset → 201 ─────────────────────

#[tokio::test]
async fn create_policy_with_preset_returns_201() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": "My Safety Policy",
            "preset": "safety_first"
        }))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 201);

    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["id"].is_string());
    assert_eq!(body["user_id"], user_id);
    assert_eq!(body["name"], "My Safety Policy");
    assert!(body["rules_json"].is_array());
    assert_eq!(body["active"], true);
    assert!(body["created_at"].is_string());
}

// ── 2. Create policy with custom rules → 201 ───────────────

#[tokio::test]
async fn create_policy_with_custom_rules_returns_201() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    let client = reqwest::Client::new();

    let rules = json!([
        { "MaxTransactionAmount": "100" },
        "OnlyAuditedProtocols"
    ]);

    let response = client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": "Custom Rules",
            "rules": rules
        }))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 201);

    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["rules_json"].is_array());
    assert_eq!(body["rules_json"].as_array().unwrap().len(), 2);
}

// ── 3. Both preset and rules → 400 ─────────────────────────

#[tokio::test]
async fn create_policy_with_both_preset_and_rules_returns_400() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": "Bad Request",
            "preset": "safety_first",
            "rules": [{ "MaxTransactionAmount": "100" }]
        }))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 400);

    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("not both"));
}

// ── 4. Neither preset nor rules → 400 ──────────────────────

#[tokio::test]
async fn create_policy_with_neither_preset_nor_rules_returns_400() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": "Missing Rules"
        }))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 400);
}

// ── 5. Invalid preset name → 400 ───────────────────────────

#[tokio::test]
async fn create_policy_with_invalid_preset_returns_400() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": "Bad Preset",
            "preset": "yolo_mode"
        }))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 400);

    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("yolo_mode"));
}

// ── 6. Invalid rules JSON → 400 ────────────────────────────

#[tokio::test]
async fn create_policy_with_invalid_rules_returns_400() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": "Bad Rules",
            "rules": [{ "NotARealRule": 42 }]
        }))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 400);

    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("Invalid rules"));
}

// ── 7. Nonexistent user → 404 ──────────────────────────────

#[tokio::test]
async fn create_policy_for_nonexistent_user_returns_404() {
    let app = common::spawn_app().await;
    let client = reqwest::Client::new();
    let fake_user_id = uuid::Uuid::new_v4().to_string();

    let response = client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": fake_user_id,
            "name": "Orphan Policy",
            "preset": "balanced"
        }))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 404);

    let body: serde_json::Value = response.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("not found"));
}

// ── 8. GET policies returns active policies ─────────────────

#[tokio::test]
async fn get_policies_returns_active_policies() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    let client = reqwest::Client::new();

    // Create two policies
    client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": "Policy A",
            "preset": "safety_first"
        }))
        .send()
        .await
        .unwrap();

    client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": "Policy B",
            "preset": "balanced"
        }))
        .send()
        .await
        .unwrap();

    // GET policies
    let response = client
        .get(format!("{}/api/v1/policies?user_id={}", app.addr, user_id))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: Vec<serde_json::Value> = response.json().await.unwrap();
    assert_eq!(body.len(), 2);
    assert_eq!(body[0]["name"], "Policy A");
    assert_eq!(body[1]["name"], "Policy B");
}

// ── 9. GET policies for user with none returns empty array ──

#[tokio::test]
async fn get_policies_returns_empty_for_user_with_no_policies() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/api/v1/policies?user_id={}", app.addr, user_id))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: Vec<serde_json::Value> = response.json().await.unwrap();
    assert_eq!(body.len(), 0);
}

// ── 10. DELETE policy deactivates it, GET no longer returns it

#[tokio::test]
async fn get_policies_returns_active_only_after_delete() {
    let app = common::spawn_app().await;
    let user_id = create_test_user(&app).await;
    let client = reqwest::Client::new();

    // Create two policies
    let resp_a = client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": "Policy A",
            "preset": "safety_first"
        }))
        .send()
        .await
        .unwrap();
    let policy_a: serde_json::Value = resp_a.json().await.unwrap();
    let policy_a_id = policy_a["id"].as_str().unwrap();

    client
        .post(format!("{}/api/v1/policies", app.addr))
        .json(&json!({
            "user_id": user_id,
            "name": "Policy B",
            "preset": "balanced"
        }))
        .send()
        .await
        .unwrap();

    // DELETE policy A
    let response = client
        .delete(format!("{}/api/v1/policies/{}", app.addr, policy_a_id))
        .send()
        .await
        .expect("Failed to send request");
    assert_eq!(response.status(), 204);

    // GET policies → only Policy B remains
    let response = client
        .get(format!("{}/api/v1/policies?user_id={}", app.addr, user_id))
        .send()
        .await
        .unwrap();
    let body: Vec<serde_json::Value> = response.json().await.unwrap();
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["name"], "Policy B");
}

// ── 11. DELETE nonexistent policy → 404 ─────────────────────

#[tokio::test]
async fn delete_nonexistent_policy_returns_404() {
    let app = common::spawn_app().await;
    let client = reqwest::Client::new();
    let fake_id = uuid::Uuid::new_v4();

    let response = client
        .delete(format!("{}/api/v1/policies/{}", app.addr, fake_id))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 404);
}