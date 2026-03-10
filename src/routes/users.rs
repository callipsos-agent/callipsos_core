use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::db::user::User;
use crate::error::AppError;
use crate::routes::AppState;

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub telegram_id: Option<i64>,
}

pub async fn create_user(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<User>), AppError> {
    let user = User::create(&state.db, req.telegram_id)
        .await
        .map_err(|e| match e {
            AppError::Database(sqlx_err) => AppError::from_db(sqlx_err),
            other => other,
        })?;

    Ok((StatusCode::CREATED, Json(user)))
}