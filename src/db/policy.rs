use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;
use crate::policy::types::UserId;

// ── PolicyRow model ─────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PolicyRow {
    pub id: Uuid,
    pub user_id: UserId,
    pub name: String,
    pub rules_json: serde_json::Value,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PolicyRow {
    pub async fn create(
        pool: &PgPool,
        user_id: UserId,
        name: &str,
        rules_json: serde_json::Value,
    ) -> Result<PolicyRow, AppError> {
        let id = Uuid::new_v4();

        let row = sqlx::query_as!(
            PolicyRow,
            r#"
            INSERT INTO policies (id, user_id, name, rules_json)
            VALUES ($1, $2, $3, $4)
            RETURNING
                id,
                user_id AS "user_id: UserId",
                name,
                rules_json,
                active,
                created_at,
                updated_at
            "#,
            id,
            user_id.0,
            name,
            rules_json,
        )
        .fetch_one(pool)
        .await
        .map_err(AppError::from_db)?;

        Ok(row)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<PolicyRow>, AppError> {
        let row = sqlx::query_as!(
            PolicyRow,
            r#"
            SELECT
                id,
                user_id AS "user_id: UserId",
                name,
                rules_json,
                active,
                created_at,
                updated_at
            FROM policies
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Returns all active policies for a user.
    /// Used by the validate endpoint to collect all rules.
    pub async fn find_active_by_user(
        pool: &PgPool,
        user_id: UserId,
    ) -> Result<Vec<PolicyRow>, AppError> {
        let rows = sqlx::query_as!(
            PolicyRow,
            r#"
            SELECT
                id,
                user_id AS "user_id: UserId",
                name,
                rules_json,
                active,
                created_at,
                updated_at
            FROM policies
            WHERE user_id = $1 AND active = TRUE
            ORDER BY created_at ASC
            "#,
            user_id.0,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Soft-delete: sets active = false.
    /// Returns the updated row, or None if not found.
    pub async fn soft_delete(pool: &PgPool, id: Uuid) -> Result<bool, AppError> {
        let result = sqlx::query!(
            r#"
            UPDATE policies
            SET active = FALSE, updated_at = NOW()
            WHERE id = $1
            "#,
            id,
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}