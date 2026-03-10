mod common;

use callipsos_core::db::user::User;
use callipsos_core::policy::types::UserId;
use uuid::Uuid;

#[tokio::test]
async fn create_user_without_telegram_id() {
    let app = common::spawn_app().await;

    let user = User::create(&app.db, None)
        .await
        .expect("Failed to create user");

    assert_ne!(user.id, UserId::from(Uuid::new_v4()));
    assert!(user.telegram_id.is_none());
    assert!(user.wallet_address.is_none());
    assert!(user.created_at <= chrono::Utc::now());
}

#[tokio::test]
async fn create_user_with_telegram_id() {
    let app = common::spawn_app().await;

    let user = User::create(&app.db, Some(123456789))
        .await
        .expect("Failed to create user");

    assert_eq!(user.telegram_id, Some(123456789));
}

#[tokio::test]
async fn find_user_by_id() {
    let app = common::spawn_app().await;

    let created = User::create(&app.db, Some(987654321))
        .await
        .expect("Failed to create user");

    let found = User::find_by_id(&app.db, created.id)
        .await
        .expect("Failed to query user");

    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.id, created.id);
    assert_eq!(found.telegram_id, Some(987654321));
}

#[tokio::test]
async fn find_user_by_nonexistent_id() {
    let app = common::spawn_app().await;

    let found = User::find_by_id(&app.db, UserId::from(Uuid::new_v4()))
        .await
        .expect("Failed to query user");

    assert!(found.is_none());
}

// ── HTTP route tests ────────────────────────────────────────

#[tokio::test]
async fn route_create_user_returns_201() {
    let app = common::spawn_app().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/users", app.addr))
        .json(&serde_json::json!({ "telegram_id": 111222333 }))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 201);

    let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
    assert_eq!(body["telegram_id"], 111222333);
    assert!(body["id"].is_string());
    assert!(body["wallet_address"].is_null());
    assert!(body["created_at"].is_string());
    assert!(body["updated_at"].is_string());
}

#[tokio::test]
async fn route_create_user_without_telegram_id_returns_201() {
    let app = common::spawn_app().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/users", app.addr))
        .json(&serde_json::json!({}))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 201);

    let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
    assert!(body["id"].is_string());
    assert!(body["telegram_id"].is_null());
}

#[tokio::test]
async fn route_duplicate_telegram_id_returns_409() {
    let app = common::spawn_app().await;
    let client = reqwest::Client::new();

    // First create succeeds
    let response = client
        .post(format!("{}/api/v1/users", app.addr))
        .json(&serde_json::json!({ "telegram_id": 444555666 }))
        .send()
        .await
        .expect("Failed to send request");
    assert_eq!(response.status(), 201);

    // Second create with same telegram_id → 409
    let response = client
        .post(format!("{}/api/v1/users", app.addr))
        .json(&serde_json::json!({ "telegram_id": 444555666 }))
        .send()
        .await
        .expect("Failed to send request");
    assert_eq!(response.status(), 409);

    let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
    assert!(body["error"].is_string());
}