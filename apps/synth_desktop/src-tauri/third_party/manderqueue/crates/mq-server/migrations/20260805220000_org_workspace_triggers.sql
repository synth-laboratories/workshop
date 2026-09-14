-- Hard tenancy: participant/message org must match owning thread workspace.

CREATE OR REPLACE FUNCTION mq_reject_cross_org_participant()
RETURNS trigger AS $$
DECLARE
  thread_org TEXT;
BEGIN
  SELECT org_id INTO thread_org FROM mq_threads WHERE thread_id = NEW.thread_id;
  IF thread_org IS NULL THEN
    RAISE EXCEPTION 'mq_thread_missing';
  END IF;
  IF NEW.org_id IS DISTINCT FROM thread_org THEN
    RAISE EXCEPTION 'mq_org_workspace_mismatch';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_mq_participants_org ON mq_participants;
CREATE TRIGGER trg_mq_participants_org
  BEFORE INSERT OR UPDATE ON mq_participants
  FOR EACH ROW EXECUTE PROCEDURE mq_reject_cross_org_participant();

CREATE OR REPLACE FUNCTION mq_reject_cross_org_message()
RETURNS trigger AS $$
DECLARE
  thread_org TEXT;
BEGIN
  SELECT org_id INTO thread_org FROM mq_threads WHERE thread_id = NEW.thread_id;
  IF thread_org IS NULL THEN
    RAISE EXCEPTION 'mq_thread_missing';
  END IF;
  IF NEW.org_id IS DISTINCT FROM thread_org THEN
    RAISE EXCEPTION 'mq_org_workspace_mismatch';
  END IF;
  IF NEW.sender_org_id IS DISTINCT FROM thread_org THEN
    RAISE EXCEPTION 'mq_org_workspace_mismatch';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_mq_messages_org ON mq_messages;
CREATE TRIGGER trg_mq_messages_org
  BEFORE INSERT OR UPDATE ON mq_messages
  FOR EACH ROW EXECUTE PROCEDURE mq_reject_cross_org_message();

CREATE OR REPLACE FUNCTION mq_reject_cross_org_job()
RETURNS trigger AS $$
DECLARE
  thread_org TEXT;
BEGIN
  SELECT org_id INTO thread_org FROM mq_threads WHERE thread_id = NEW.thread_id;
  IF thread_org IS NULL THEN
    RAISE EXCEPTION 'mq_thread_missing';
  END IF;
  IF NEW.recipient_org_id IS DISTINCT FROM thread_org THEN
    RAISE EXCEPTION 'mq_org_workspace_mismatch';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_mq_jobs_org ON mq_delivery_jobs;
CREATE TRIGGER trg_mq_jobs_org
  BEFORE INSERT OR UPDATE ON mq_delivery_jobs
  FOR EACH ROW EXECUTE PROCEDURE mq_reject_cross_org_job();
