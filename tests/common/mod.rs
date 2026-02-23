use callipsos_core::db;
use callipsos_core::routes::{self, AppState};
use sqlx::PgPool;
use tokio::net::TcpListener;

pub struct TestApp {
    pub addr: String,
    pub db: PgPool,
}

/// Spawns the app on a random port with a real test database.
pub async fn spawn_app() -> TestApp {
    dotenvy::dotenv().ok();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for integration tests");

    let pool = db::connect(&database_url)
        .await
        .expect("Failed to connect to test database");
    db::migrate(&pool)
        .await
        .expect("Failed to run migrations");

    let state = AppState { db: pool.clone() };
    let app = routes::create_router(state);

    // Port 0 → OS assigns random available port
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let addr = format!("http://{}", listener.local_addr().unwrap());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestApp { addr, db: pool }
}