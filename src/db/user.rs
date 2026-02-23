use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use std::fmt;
use std::ops::Deref;
use uuid::Uuid;

use crate::error::AppError;



#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize)]
#[sqlx(transparent)]
pub struct UserId(pub Uuid);

impl Deref for UserId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for UserId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

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
        let id = Uuid::new_v4();

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
            id,
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