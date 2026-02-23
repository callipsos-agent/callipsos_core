mod common;

#[tokio::test]
async fn health_check_returns_200() {
    let app = common::spawn_app().await;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/health", app.addr))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
    assert_eq!(body["status"], "ok");
}