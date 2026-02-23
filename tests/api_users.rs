mod common;

use callipsos_core::db::user::User;
use uuid::Uuid;

#[tokio::test]
async fn create_user_without_telegram_id() {
    let app = common::spawn_app().await;

    let user = User::create(&app.db, None)
        .await
        .expect("Failed to create user");

    assert_ne!(user.id, Uuid::nil());
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

    let found = User::find_by_id(&app.db, Uuid::new_v4())
        .await
        .expect("Failed to query user");

    assert!(found.is_none());
}