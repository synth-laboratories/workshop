-- Device sign-out. Once set, the enrollment's grants and incarnations are
-- refused and its (owner, device, session) key cannot be re-enrolled.
ALTER TABLE mq_enrollments ADD COLUMN revoked_at TIMESTAMPTZ;
