-- Forward-only acceptance metadata. Existing migrations/checksums remain unchanged.
-- Coordinate writers: older binaries do not populate fingerprint/recipient intent.
ALTER TABLE mq_messages ADD COLUMN request_fingerprint TEXT;
ALTER TABLE mq_messages ADD COLUMN delivery_recipients JSONB NOT NULL DEFAULT '[]';
CREATE UNIQUE INDEX uq_mq_messages_publisher_idem
    ON mq_messages (org_id, thread_id, sender_kind, sender_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
DROP INDEX uq_mq_messages_org_idem;

ALTER TABLE mq_delivery_jobs ADD COLUMN lease_until TIMESTAMPTZ;
ALTER TABLE mq_delivery_jobs ADD COLUMN next_attempt_at TIMESTAMPTZ;
CREATE INDEX idx_mq_jobs_retry ON mq_delivery_jobs (next_attempt_at, lease_until, created_at)
    WHERE status = 'pending';
