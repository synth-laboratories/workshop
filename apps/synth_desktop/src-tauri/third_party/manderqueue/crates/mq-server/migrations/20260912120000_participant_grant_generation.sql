-- Credential revocation survives role restoration. Client input never sets this.
ALTER TABLE mq_participants ADD COLUMN grant_generation BIGINT NOT NULL DEFAULT 0
    CHECK (grant_generation >= 0);
