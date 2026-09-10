import tempfile
import unittest
from pathlib import Path

from environment_qa.config_checks import check_configuration, declared_bytes


def write(root, **files):
    for name, text in files.items():
        target = root / name.replace('__', '/')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)


class ConfigurationCheckTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)

    def test_declared_size_parsing_rejects_unknown_shapes(self):
        self.assertEqual(declared_bytes('4G'), 4 * 1024 ** 3)
        self.assertEqual(declared_bytes('512MiB'), 512 * 1024 ** 2)
        self.assertIsNone(declared_bytes('lots'))
        self.assertIsNone(declared_bytes(''))

    def test_memory_limit_finding_anchors_on_task_toml_not_the_allocating_file(self):
        write(self.root, **{'task.toml': '[environment]\ncpus = 2\nmemory = "4G"\n'})
        findings = check_configuration(self.root, [('solution/solve.sh', 12, 7.25e9)])
        self.assertEqual(len(findings), 1)
        item = findings[0]
        self.assertEqual(item['path'], 'task.toml')
        self.assertEqual(item['mechanism'], 'declared_memory_limit_enforcement_dependent')
        self.assertIn('memory = "4G"', item['evidence'])
        # The claim must assert the enforcement dependency without asserting a
        # mechanism the check never measured.
        self.assertNotIn('swap or overcommit can mask', item['causal_claim'])
        self.assertIn('swap', item['failure_condition'])
        self.assertIn('overcommit', item['failure_condition'])
        self.assertTrue(any('requires runtime measurement' in l for l in item['limitations']))

    def test_no_memory_finding_without_an_allocation_bound(self):
        write(self.root, **{'task.toml': '[environment]\nmemory = "4G"\n'})
        self.assertEqual(check_configuration(self.root, []), [])

    def test_single_cpu_masking_a_parallel_build(self):
        write(self.root, **{'task.toml': '[environment]\ncpus = 1\nmemory = "2G"\n',
                            'solution__solve.sh': '#!/bin/sh\nmake -j 8 all\n'})
        mechanisms = {f['mechanism']: f for f in check_configuration(self.root, [])}
        item = mechanisms['declared_cpu_limit_masks_parallel_build']
        self.assertEqual(item['path'], 'task.toml')
        self.assertIn('cpus = 1', item['evidence'])
        self.assertIn('-j 8', item['supporting_evidence'][1]['evidence'])

    def test_multi_cpu_declaration_is_not_flagged(self):
        write(self.root, **{'task.toml': '[environment]\ncpus = 4\n',
                            'solution__solve.sh': 'make -j 8 all\n'})
        self.assertEqual(check_configuration(self.root, []), [])

    def test_nonpositive_timeout(self):
        write(self.root, **{'task.toml': '[verifier]\ntimeout_sec = 0\n'})
        self.assertEqual([f['mechanism'] for f in check_configuration(self.root, [])],
                         ['nonpositive_declared_timeout_verifier'])

    def test_missing_or_invalid_task_toml_is_not_an_error(self):
        self.assertEqual(check_configuration(self.root, []), [])
        write(self.root, **{'task.toml': 'this is not = valid toml ['})
        self.assertEqual(check_configuration(self.root, []), [])


if __name__ == '__main__':
    unittest.main()
