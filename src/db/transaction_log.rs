use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;
use crate::policy::types::UserId;

// ── TransactionLogRow model ─────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TransactionLogRow {
    pub id: Uuid,
    pub user_id: UserId,
    pub policy_id: Option<Uuid>,
    pub request_json: serde_json::Value,
    pub verdict: String,
    pub reasons_json: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl TransactionLogRow {
    pub async fn create(
        pool: &PgPool,
        user_id: UserId,
        policy_id: Option<Uuid>,
        request_json: serde_json::Value,
        verdict: &str,
        reasons_json: serde_json::Value,
    ) -> Result<TransactionLogRow, AppError> {
        let id = Uuid::new_v4();

        let row = sqlx::query_as!(
            TransactionLogRow,
            r#"
            INSERT INTO transaction_log (id, user_id, policy_id, request_json, verdict, reasons_json)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING
                id,
                user_id AS "user_id: UserId",
                policy_id,
                request_json,
                verdict,
                reasons_json,
                created_at
            "#,
            id,
            user_id.0,
            policy_id,
            request_json,
            verdict,
            reasons_json,
        )
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Returns all log entries for a user, ordered by most recent first.
    /// Used by tests to verify logging behavior.
    pub async fn find_by_user(
        pool: &PgPool,
        user_id: UserId,
    ) -> Result<Vec<TransactionLogRow>, AppError> {
        let rows = sqlx::query_as!(
            TransactionLogRow,
            r#"
            SELECT
                id,
                user_id AS "user_id: UserId",
                policy_id,
                request_json,
                verdict,
                reasons_json,
                created_at
            FROM transaction_log
            WHERE user_id = $1
            ORDER BY created_at DESC
            "#,
            user_id.0,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }
}