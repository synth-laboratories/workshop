-- Thread ensure key + Intern correlation fields. Drop run as scope ontology.

ALTER TABLE mq_threads
    ADD COLUMN IF NOT EXISTS idempotency_key TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS uq_mq_threads_org_idem
    ON mq_threads (org_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

ALTER TABLE mq_messages
    ADD COLUMN IF NOT EXISTS parent_message_id UUID REFERENCES mq_messages(message_id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS causation_id TEXT;

CREATE INDEX IF NOT EXISTS idx_mq_messages_parent
    ON mq_messages (parent_message_id)
    WHERE parent_message_id IS NOT NULL;

-- Soft-delete run-scoped labels from being recommended; existing rows may remain.
-- New writes must not use scope_kind = 'run' (enforced in application).
