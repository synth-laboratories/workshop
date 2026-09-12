-- Device/session enrollment and thread access grants.
-- See docs/WORKSHOP_GRANT_CONTRACT.md. Client input never sets incarnation,
-- generation, principal or status directly.

CREATE TABLE mq_enrollments (
    enrollment_id UUID PRIMARY KEY,
    org_id        TEXT NOT NULL,
    owner_kind    TEXT NOT NULL CHECK (owner_kind = 'human'),
    owner_id      TEXT NOT NULL,
    device_id     TEXT NOT NULL,
    session_id    TEXT NOT NULL,
    label         TEXT,
    incarnation   BIGINT NOT NULL CHECK (incarnation >= 1),
    created_at    TIMESTAMPTZ NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL,
    CONSTRAINT uq_mq_enrollments_owner_device_session
        UNIQUE (org_id, owner_kind, owner_id, device_id, session_id)
);

CREATE INDEX idx_mq_enrollments_owner ON mq_enrollments (org_id, owner_kind, owner_id);

CREATE TABLE mq_grants (
    grant_id          UUID PRIMARY KEY,
    org_id            TEXT NOT NULL,
    thread_id         UUID NOT NULL REFERENCES mq_threads(thread_id) ON DELETE CASCADE,
    enrollment_id     UUID NOT NULL REFERENCES mq_enrollments(enrollment_id) ON DELETE CASCADE,
    principal_kind    TEXT NOT NULL,
    principal_id      TEXT NOT NULL,
    operations        TEXT[] NOT NULL,
    history_after_seq BIGINT NOT NULL CHECK (history_after_seq >= 0),
    expires_at        TIMESTAMPTZ NOT NULL,
    generation        BIGINT NOT NULL DEFAULT 0 CHECK (generation >= 0),
    status            TEXT NOT NULL CHECK (status IN ('active', 'revoked')),
    granted_by_kind   TEXT NOT NULL,
    granted_by_id     TEXT NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL,
    CONSTRAINT uq_mq_grants_thread_enrollment UNIQUE (thread_id, enrollment_id),
    CONSTRAINT ck_mq_grants_operations CHECK (
        cardinality(operations) BETWEEN 1 AND 2
        AND operations <@ ARRAY['read', 'publish']::TEXT[]
    ),
    -- The grantee is always the server-derived enrollment principal.
    CONSTRAINT ck_mq_grants_principal CHECK (
        principal_kind = 'actor' AND principal_id = 'enrollment:' || enrollment_id::TEXT
    )
);

CREATE INDEX idx_mq_grants_recipient ON mq_grants (thread_id, principal_kind, principal_id);
CREATE INDEX idx_mq_grants_enrollment ON mq_grants (enrollment_id);

-- Hard tenancy: a grant's org must match both its thread and its enrollment.
CREATE OR REPLACE FUNCTION mq_reject_cross_org_grant()
RETURNS trigger AS $$
DECLARE
  thread_org TEXT;
  enrollment_org TEXT;
BEGIN
  SELECT org_id INTO thread_org FROM mq_threads WHERE thread_id = NEW.thread_id;
  SELECT org_id INTO enrollment_org FROM mq_enrollments WHERE enrollment_id = NEW.enrollment_id;
  IF thread_org IS NULL OR enrollment_org IS NULL THEN
    RAISE EXCEPTION 'mq_grant_parent_missing';
  END IF;
  IF NEW.org_id IS DISTINCT FROM thread_org OR NEW.org_id IS DISTINCT FROM enrollment_org THEN
    RAISE EXCEPTION 'mq_org_workspace_mismatch';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_mq_grants_org
  BEFORE INSERT OR UPDATE ON mq_grants
  FOR EACH ROW EXECUTE PROCEDURE mq_reject_cross_org_grant();
