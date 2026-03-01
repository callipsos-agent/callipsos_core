use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

use uuid::Uuid;

use crate::error::AppError;
use crate::policy::types::UserId;




// ── User model ──────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: UserId,
    pub telegram_id: Option<i64>,
    pub wallet_address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl User {
    pub async fn create(pool: &PgPool, telegram_id: Option<i64>) -> Result<User, AppError> {
        let id = UserId::from(Uuid::new_v4());

        let user = sqlx::query_as!(
            User,
            r#"
            INSERT INTO users (id, telegram_id)
            VALUES ($1, $2)
            RETURNING
                id AS "id: UserId",
                telegram_id,
                wallet_address,
                created_at,
                updated_at
            "#,
            id.0,
            telegram_id
        )
        .fetch_one(pool)
        .await?;

        Ok(user)
    }

    pub async fn find_by_id(pool: &PgPool, id: UserId) -> Result<Option<User>, AppError> {
        let user = sqlx::query_as!(
            User,
            r#"
            SELECT
                id AS "id: UserId",
                telegram_id,
                wallet_address,
                created_at,
                updated_at
            FROM users
            WHERE id = $1
            "#,
            id.0
        )
        .fetch_optional(pool)
        .await?;

        Ok(user)
    }
}