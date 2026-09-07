import unittest
from environment_qa.diagnostics import reference_diagnostics

class DiagnosticTests(unittest.TestCase):
    def test_only_observed_reference_errors(self):
        result=reference_diagnostics({'x/oracle.txt':'g++ -Werror\nerror: tool missing\nerror: tool missing\n','x/qa-trajectory.json':'ERROR: intentionally invalid probe'})
        self.assertEqual(len(result),1)
        self.assertEqual(result[0]['evidence'],'error: tool missing')
        self.assertEqual(result[0]['severity'],'warning')
