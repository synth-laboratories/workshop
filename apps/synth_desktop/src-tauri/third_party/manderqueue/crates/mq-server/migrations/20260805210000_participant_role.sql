-- Add fixed role column; caps remain derived/stored for enforcement.
ALTER TABLE mq_participants
    ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'member';

UPDATE mq_participants
SET role = CASE
    WHEN 'close' = ANY (caps) AND 'invite' = ANY (caps) THEN 'owner'
    WHEN 'invite' = ANY (caps) THEN 'moderator'
    WHEN 'publish' = ANY (caps) THEN 'member'
    ELSE 'observer'
END
WHERE role = 'member' OR role IS NULL;
