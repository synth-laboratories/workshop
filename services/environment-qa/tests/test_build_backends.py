import unittest

from environment_qa.build_backends import COMMAND, parse, summary


class BuildBackendTests(unittest.TestCase):
    def test_resolved_versions_are_reported(self):
        stdout = 'noise\nQA_BUILD_BACKENDS {"interpreter": "3.12.1", "resolved": {"setuptools": "82.0.0", "wheel": null}}\n'
        profile = parse(stdout)
        self.assertTrue(profile['measured'])
        self.assertEqual(profile['resolved'], {'setuptools': '82.0.0'})
        self.assertEqual(profile['installed'], ['setuptools'])
        self.assertEqual(profile['interpreter'], '3.12.1')

    def test_missing_measurement_stays_unknown_not_absent(self):
        for stdout in ('', 'command not found', 'QA_BUILD_BACKENDS not-json'):
            profile = parse(stdout)
            self.assertFalse(profile['measured'])
            self.assertEqual(profile['resolved'], {})
            self.assertIn('unknown, not absent', profile['notice'])

    def test_summary_names_measured_versions_and_forbids_inference(self):
        line = summary(parse('QA_BUILD_BACKENDS {"interpreter": "3.12.1", "resolved": {"setuptools": "82.0.0"}}'))
        self.assertIn('setuptools 82.0.0', line)
        self.assertIn('do not infer a version', line)

    def test_summary_without_measurement_forbids_assumption(self):
        self.assertIn('do not assume any version', summary(parse('')))
        self.assertIn('do not assume any version', summary(None))

    def test_probe_command_is_read_only(self):
        # No installation, no build, no writes: resolution is a measurement.
        for forbidden in ('pip install', 'apt-get', 'build_ext', 'setup.py', ' > ', 'rm '):
            self.assertNotIn(forbidden, COMMAND)


if __name__ == '__main__':
    unittest.main()
