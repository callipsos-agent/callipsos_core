pub mod health;
pub mod users;
pub mod policies;
pub mod validate;

use axum::{routing::{get,post, delete}, Router};
use sqlx::PgPool;
use std::sync::Arc;

use crate::signing::SigningProvider;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub signing_provider: Option<Arc<dyn SigningProvider>>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health_check))
        .route("/api/v1/users", post(users::create_user))
        .route("/api/v1/policies", post(policies::create_policy).get(policies::get_policies))
        .route("/api/v1/policies/{id}", delete(policies::delete_policy))
        .route("/api/v1/validate", post(validate::validate))
        .with_state(state)
        
}