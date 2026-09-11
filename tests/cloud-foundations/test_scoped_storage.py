"""Offline design fixtures, not product migration or W04/W05 qualification."""
import hashlib
import pathlib
import sqlite3
import unittest

SQL = pathlib.Path(__file__).with_name('scoped_storage.sql').read_text()

class ScopedStorage(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.execute('PRAGMA foreign_keys = ON')
        self.db.executescript("CREATE TABLE sessions(id TEXT PRIMARY KEY); INSERT INTO sessions VALUES('legacy'); CREATE TABLE journal(scope_id TEXT, event_id TEXT, body TEXT, PRIMARY KEY(scope_id,event_id));")
        self.db.executescript(SQL)
        self.db.execute("INSERT INTO cloud_scopes VALUES('a','https://api.example','account-a','org-a','fixture',0)")
        self.db.execute("INSERT INTO cloud_scopes VALUES('b','https://api.example','account-b','org-b','fixture',0)")
        self.db.commit()

    def tearDown(self):
        self.db.close()

    def commit_page(self, scope, epoch, fail=False):
        with self.db:
            current = self.db.execute('SELECT auth_epoch FROM cloud_scopes WHERE id=?', (scope,)).fetchone()
            if current != (epoch,):
                raise ValueError('stale authentication epoch')
            self.db.execute('INSERT OR IGNORE INTO journal VALUES(?,?,?)', (scope, 'event-1', 'fixture'))
            if fail:
                raise RuntimeError('simulated crash before checkpoint')
            self.db.execute('INSERT OR REPLACE INTO external_checkpoints VALUES(?,?,?,?)', (scope,'intern','runtime-1','{"sequence":1,"generation":0}'))

    def test_legacy_rows_remain_unbound_and_accounts_do_not_alias(self):
        self.assertEqual(self.db.execute('SELECT id FROM sessions').fetchall(), [('legacy',)])
        self.assertEqual(self.db.execute('SELECT * FROM cloud_session_bindings').fetchall(), [])
        self.commit_page('a',0)
        self.assertEqual(self.db.execute("SELECT * FROM external_checkpoints WHERE scope_id='b'").fetchall(), [])
        self.commit_page('b',0)
        self.assertEqual(self.db.execute('SELECT count(*) FROM journal').fetchone(), (2,))

    def test_commit_before_checkpoint_and_replay_dedupe(self):
        with self.assertRaises(RuntimeError):
            self.commit_page('a',0,fail=True)
        self.assertEqual(self.db.execute('SELECT count(*) FROM journal').fetchone(), (0,))
        self.assertEqual(self.db.execute('SELECT count(*) FROM external_checkpoints').fetchone(), (0,))
        self.commit_page('a',0)
        self.commit_page('a',0)
        self.assertEqual(self.db.execute('SELECT count(*) FROM journal').fetchone(), (1,))

    def test_epoch_fences_late_writeback(self):
        self.db.execute("UPDATE cloud_scopes SET auth_epoch=1 WHERE id='a'")
        self.db.commit()
        with self.assertRaises(ValueError): self.commit_page('a',0)
        self.assertEqual(self.db.execute('SELECT count(*) FROM journal').fetchone(), (0,))

    def test_outcome_unknown_keeps_original_identity_and_remote_reconciling(self):
        body=b'{"expected_generation":0,"body":"fixture"}'
        digest=hashlib.sha256(body).hexdigest()
        self.db.execute("INSERT INTO cloud_command_outbox VALUES('a','cmd','send','key',?,?,0,0,'pending')", (body,digest))
        self.db.execute("UPDATE cloud_command_outbox SET delivery_state='outcome_unknown'")
        self.db.execute("INSERT INTO execution_bindings(scope_id,adapter,external_run_id) VALUES('a','intern','remote')")
        self.db.commit()
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("UPDATE cloud_command_outbox SET body='different'")
        self.assertEqual(self.db.execute('SELECT body,body_sha256 FROM cloud_command_outbox').fetchone(), (body,digest))
        self.assertEqual(self.db.execute('SELECT remote_state FROM execution_bindings').fetchone(), ('reconciling',))

if __name__ == '__main__': unittest.main()
