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
    /// AES-256-GCM encrypted LLM API key. base64(nonce || ciphertext).
    /// Null if user hasn't set a key via /setkey.
    #[serde(skip_serializing)]
    pub llm_api_key_encrypted: Option<String>,
    /// True after user has selected a policy preset and is ready to chat.
    pub onboarded: bool,
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
                llm_api_key_encrypted,
                onboarded,
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
                llm_api_key_encrypted,
                onboarded,
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

    /// Find a user by their Telegram ID. Returns None if not found.
    pub async fn find_by_telegram_id(
        pool: &PgPool,
        telegram_id: i64,
    ) -> Result<Option<User>, AppError> {
        let user = sqlx::query_as!(
            User,
            r#"
            SELECT
                id AS "id: UserId",
                telegram_id,
                wallet_address,
                llm_api_key_encrypted,
                onboarded,
                created_at,
                updated_at
            FROM users
            WHERE telegram_id = $1
            "#,
            telegram_id
        )
        .fetch_optional(pool)
        .await?;

        Ok(user)
    }

    /// Store the user's encrypted LLM API key.
    /// The caller is responsible for encrypting the key with
    /// encrypt::encrypt() before passing it here.
    pub async fn set_llm_key(
        pool: &PgPool,
        user_id: UserId,
        encrypted_key: &str,
    ) -> Result<(), AppError> {
        sqlx::query!(
            r#"
            UPDATE users
            SET llm_api_key_encrypted = $2, updated_at = NOW()
            WHERE id = $1
            "#,
            user_id.0,
            encrypted_key,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Clear the user's stored LLM API key.
    pub async fn clear_llm_key(
        pool: &PgPool,
        user_id: UserId,
    ) -> Result<(), AppError> {
        sqlx::query!(
            r#"
            UPDATE users
            SET llm_api_key_encrypted = NULL, updated_at = NOW()
            WHERE id = $1
            "#,
            user_id.0,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Mark user as onboarded (policy selected, ready to use agent).
    pub async fn set_onboarded(pool: &PgPool, user_id: UserId) -> Result<(), AppError> {
        sqlx::query!(
            "UPDATE users SET onboarded = TRUE, updated_at = NOW() WHERE id = $1",
            user_id.0,
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}