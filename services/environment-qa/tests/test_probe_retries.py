import unittest
from environment_qa.probe_retries import timed_out_build, repeats_build


class ProbeRetriesTests(unittest.TestCase):
    def test_only_unfinished_build_at_timeout(self):
        out={'return_code':124,'stdout':'Building wheel for small (pyproject.toml): started\nBuilding wheel for small (pyproject.toml): finished with status done\nBuilding wheel for slow-lib (pyproject.toml): started'}
        self.assertEqual(timed_out_build(out),'slow-lib')
        self.assertIsNone(timed_out_build(dict(out,return_code=0)))

    def test_changed_target_does_not_allow_same_build(self):
        self.assertEqual(repeats_build('python -m pip install --target /tmp/other slow_lib', {'slow-lib'}),'slow-lib')
        self.assertIsNone(repeats_build('python -m pip install lightweight', {'slow-lib'}))
        self.assertIsNone(repeats_build('tar -tf slow-lib.tar.gz', {'slow-lib'}))

    def test_finished_and_nonbuild_timeouts_do_not_block(self):
        self.assertIsNone(timed_out_build({'return_code':124,'stdout':'Fetching metadata'}))
        self.assertIsNone(timed_out_build({'return_code':124,'stdout':'Building wheel for pkg: started\nBuilding wheel for pkg: finished with status done'}))
