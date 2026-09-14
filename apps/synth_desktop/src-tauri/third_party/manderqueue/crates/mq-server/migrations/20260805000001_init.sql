-- Manderqueue schema (Postgres SoT)

CREATE TABLE IF NOT EXISTS mq_threads (
    thread_id   UUID PRIMARY KEY,
    org_id      TEXT NOT NULL,
    scope_kind  TEXT NOT NULL,
    scope_id    TEXT NOT NULL,
    title       TEXT,
    idempotency_key TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mq_threads_org_scope
    ON mq_threads (org_id, scope_kind, scope_id);

CREATE UNIQUE INDEX IF NOT EXISTS uq_mq_threads_org_idem
    ON mq_threads (org_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS mq_participants (
    thread_id       UUID NOT NULL REFERENCES mq_threads(thread_id) ON DELETE CASCADE,
    principal_kind  TEXT NOT NULL,
    principal_id    TEXT NOT NULL,
    org_id          TEXT NOT NULL,
    role            TEXT NOT NULL DEFAULT 'member',
    caps            TEXT[] NOT NULL DEFAULT '{}',
    PRIMARY KEY (thread_id, principal_kind, principal_id, org_id)
);

CREATE TABLE IF NOT EXISTS mq_messages (
    message_id        UUID PRIMARY KEY,
    thread_id         UUID NOT NULL REFERENCES mq_threads(thread_id) ON DELETE CASCADE,
    org_id            TEXT NOT NULL,
    seq               BIGINT NOT NULL,
    kind              TEXT NOT NULL,
    body              TEXT NOT NULL,
    payload           JSONB NOT NULL DEFAULT '{}',
    sender_kind       TEXT NOT NULL,
    sender_id         TEXT NOT NULL,
    sender_org_id     TEXT NOT NULL,
    idempotency_key   TEXT,
    correlation_id    TEXT,
    parent_message_id UUID REFERENCES mq_messages(message_id) ON DELETE SET NULL,
    causation_id      TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (thread_id, seq)
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_mq_messages_org_idem
    ON mq_messages (org_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_mq_messages_thread_seq
    ON mq_messages (thread_id, seq);

CREATE TABLE IF NOT EXISTS mq_delivery_jobs (
    job_id              UUID PRIMARY KEY,
    message_id          UUID NOT NULL REFERENCES mq_messages(message_id) ON DELETE CASCADE,
    thread_id           UUID NOT NULL REFERENCES mq_threads(thread_id) ON DELETE CASCADE,
    recipient_kind      TEXT NOT NULL,
    recipient_id        TEXT NOT NULL,
    recipient_org_id    TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'pending',
    attempts            INT NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (message_id, recipient_kind, recipient_id, recipient_org_id)
);

CREATE INDEX IF NOT EXISTS idx_mq_jobs_pending
    ON mq_delivery_jobs (status, created_at)
    WHERE status = 'pending';
