import tempfile
import unittest
from pathlib import Path
from environment_qa.environment_checks import check_environment, allocation_lower_bounds

class EnvironmentChecksTests(unittest.TestCase):
    def test_image_helper_does_not_assume_later_verifier_upload(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);e=p/'environment';e.mkdir()
            (e/'filter.py').write_text('pass')
            (e/'helper.py').write_text('p="/tests/filter.py"')
            (e/'Dockerfile').write_text('COPY filter.py /app/\nCOPY helper.py /app/\n')
            fs=check_environment(p)
            self.assertEqual(len(fs),1)
            self.assertEqual(fs[0]['mechanism'],'agent_selftest_path_staging_mismatch')
            (e/'Dockerfile').write_text('COPY filter.py /tests/\nCOPY helper.py /app/\n')
            self.assertEqual(check_environment(p),[])

    def test_allocation_arithmetic_without_execution(self):
        rows=allocation_lower_bounds('n=input & 3;\nx=10+2*n;\np=malloc(8LL*x*x);')
        self.assertEqual(rows[0][2],800)

    def test_unknown_allocation_not_invented(self):
        self.assertEqual(allocation_lower_bounds('p=malloc(unknown*2);'),[])

    def test_small_allocation_not_flagged(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'task.toml').write_text('[environment]\nmemory_mb=1\n')
            (p/'solve.sh').write_text('p=malloc(1000);')
            self.assertEqual(check_environment(p),[])
    def test_cross_mount_move_is_conditional(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'test.py').write_text('import os\nos.rename("input", "/tmp/output")\nos.rename("/tmp/a", "/tmp/b")\n')
            fs=check_environment(p)
            self.assertEqual(len(fs),1)
            self.assertIn('if',fs[0]['title'])
            self.assertEqual(fs[0]['severity'],'warning')

    def test_core_limit_does_not_claim_backend_reproduction(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d);(p/'solve.sh').write_text('ulimit -c unlimited\nulimit -c 0\n')
            fs=check_environment(p)
            self.assertEqual(len(fs),1)
            self.assertIn('when',fs[0]['title'])
