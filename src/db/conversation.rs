use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;
use crate::policy::types::UserId;

// ── Message types ───────────────────────────────────────────
// These serialize into the JSONB array stored in conversations.messages_json.
// Kept minimal: role + content + optional tool data.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: MessageRole,
    pub content: String,
    /// Present only on assistant messages that invoked tools.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
}

// ── Row model ───────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ConversationRow {
    pub id: Uuid,
    pub user_id: UserId,
    pub messages_json: serde_json::Value,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ConversationRow {
    /// Returns the active conversation for a user, or None if no session exists.
    /// The unique partial index guarantees at most one result.
    pub async fn find_active(
        pool: &PgPool,
        user_id: UserId,
    ) -> Result<Option<ConversationRow>, AppError> {
        let row = sqlx::query_as!(
            ConversationRow,
            r#"
            SELECT
                id,
                user_id AS "user_id: UserId",
                messages_json,
                active,
                created_at,
                updated_at
            FROM conversations
            WHERE user_id = $1 AND active = TRUE
            "#,
            user_id.0,
        )
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Creates a new active conversation. Caller must deactivate any existing
    /// active conversation first, or this will fail on the unique index.
    pub async fn create(
        pool: &PgPool,
        user_id: UserId,
    ) -> Result<ConversationRow, AppError> {
        let id = Uuid::new_v4();

        let row = sqlx::query_as!(
            ConversationRow,
            r#"
            INSERT INTO conversations (id, user_id, messages_json)
            VALUES ($1, $2, '[]'::jsonb)
            RETURNING
                id,
                user_id AS "user_id: UserId",
                messages_json,
                active,
                created_at,
                updated_at
            "#,
            id,
            user_id.0,
        )
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Deactivates all active conversations for a user.
    /// Safe to call even if none are active.
    pub async fn deactivate_all(
        pool: &PgPool,
        user_id: UserId,
    ) -> Result<(), AppError> {
        sqlx::query!(
            r#"
            UPDATE conversations
            SET active = FALSE, updated_at = NOW()
            WHERE user_id = $1 AND active = TRUE
            "#,
            user_id.0,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Starts a fresh session: deactivates any existing active conversation,
    /// then creates a new one. Returns the new conversation.
    pub async fn reset(
        pool: &PgPool,
        user_id: UserId,
    ) -> Result<ConversationRow, AppError> {
        Self::deactivate_all(pool, user_id).await?;
        Self::create(pool, user_id).await
    }

    /// Appends a message to the active conversation's JSONB array.
    /// Uses jsonb_concat to atomically append without read-modify-write.
    /// Returns the updated row, or None if no active conversation exists.
    pub async fn append_message(
        pool: &PgPool,
        user_id: UserId,
        message: &ConversationMessage,
    ) -> Result<Option<ConversationRow>, AppError> {
        let message_json = serde_json::to_value(message)
            .map_err(|e| AppError::Internal(format!("Failed to serialize message: {e}")))?;

        // Wrap the single message in an array for jsonb_concat: [] || [msg] = [msg]
        let wrapper = serde_json::Value::Array(vec![message_json]);

        let row = sqlx::query_as!(
            ConversationRow,
            r#"
            UPDATE conversations
            SET messages_json = messages_json || $1::jsonb,
                updated_at = NOW()
            WHERE user_id = $2 AND active = TRUE
            RETURNING
                id,
                user_id AS "user_id: UserId",
                messages_json,
                active,
                created_at,
                updated_at
            "#,
            wrapper,
            user_id.0,
        )
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Deserializes the JSONB array into typed messages.
    pub fn messages(&self) -> Result<Vec<ConversationMessage>, AppError> {
        serde_json::from_value(self.messages_json.clone())
            .map_err(|e| AppError::Internal(format!("Failed to deserialize messages: {e}")))
    }

    /// Returns the number of messages in this conversation.
    pub fn message_count(&self) -> usize {
        self.messages_json
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0)
    }

    /// Trims the active conversation to the most recent `max_messages`.
    /// Operates atomically in PostgreSQL by slicing the JSONB array.
    /// Returns the updated row, or None if no active conversation exists.
    pub async fn trim_to_recent(
        pool: &PgPool,
        user_id: UserId,
        max_messages: i32,
    ) -> Result<Option<ConversationRow>, AppError> {
        // jsonb_array_length - max gives the start index.
        // GREATEST ensures we never use a negative index (no-op if already short enough).
        // Array slice: messages_json[start : length] keeps the tail.
        let row = sqlx::query_as!(
            ConversationRow,
            r#"
            UPDATE conversations
            SET messages_json = (
                    SELECT COALESCE(
                        jsonb_agg(elem ORDER BY idx),
                        '[]'::jsonb
                    )
                    FROM jsonb_array_elements(messages_json)
                        WITH ORDINALITY AS t(elem, idx)
                    WHERE idx > (jsonb_array_length(messages_json) - $1)
                ),
                updated_at = NOW()
            WHERE user_id = $2 AND active = TRUE
              AND jsonb_array_length(messages_json) > $1
            RETURNING
                id,
                user_id AS "user_id: UserId",
                messages_json,
                active,
                created_at,
                updated_at
            "#,
            max_messages as i64,
            user_id.0,
        )
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Extracts user/assistant text pairs for Rig's Chat trait.
    /// Tool calls are stored for auditability but Rig doesn't consume them
    /// across turns. It only needs the text content of each message.
    pub fn to_rig_messages(&self) -> Result<Vec<(MessageRole, String)>, AppError> {
        let messages = self.messages()?;
        Ok(messages
            .into_iter()
            .map(|m| (m.role, m.content))
            .collect())
    }
}