-- 002_agent_memory.sql
-- Telegram bot: user onboarding state + conversation memory

-- ── User columns ────────────────────────────────────────────
-- API key for the user's own LLM provider (Anthropic for MVP).
-- Encrypted with AES-256-GCM at the application layer (src/encrypt.rs).
-- Column stores base64(nonce || ciphertext). Requires ENCRYPTION_KEY
-- env var to decrypt. Without it, values are unrecoverable.
ALTER TABLE users ADD COLUMN IF NOT EXISTS llm_api_key_encrypted TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS onboarded BOOLEAN NOT NULL DEFAULT FALSE;

-- ── Conversations ───────────────────────────────────────────
-- One row per Telegram chat session. A new session starts on /start
-- or /reset. The bot loads the most recent active session on each
-- message and passes its history to Rig.
--
-- messages_json stores the full ordered conversation as a JSONB array:
-- [
--   { "role": "user", "content": "..." },
--   { "role": "assistant", "content": "...", "tool_calls": [...] },
--   ...
-- ]
--
-- This preserves tool call ordering within a single assistant turn,
-- which a one-row-per-message schema cannot do without a sub-index.
CREATE TABLE IF NOT EXISTS conversations (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    messages_json   JSONB NOT NULL DEFAULT '[]'::jsonb,
    active          BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Ensure at most one active conversation per user.
-- Also serves as the fast lookup index for "give me this user's active conversation."
CREATE UNIQUE INDEX IF NOT EXISTS idx_conversations_user_active_unique
    ON conversations(user_id)
    WHERE active = TRUE;

-- Time-ordered listing for history/export
CREATE INDEX IF NOT EXISTS idx_conversations_user_time
    ON conversations(user_id, created_at DESC);