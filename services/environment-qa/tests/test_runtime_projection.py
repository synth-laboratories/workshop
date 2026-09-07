import unittest
from environment_qa.runtime_projection import focus

class RuntimeProjectionTests(unittest.TestCase):
    def test_middle_cited_record_and_uncited_error_survive(self):
        lines=['routine output '+str(i)+'\n' for i in range(1000)]
        lines[300]='selected-record: bad value\n';lines[700]='RuntimeError: counterevidence\n'
        body=''.join(lines);files={'runtime/experiment-1/step.stdout.txt':body,'solution/solve.sh':body}
        run={'findings':[{'evidence':'selected-record: bad value'}],'evidence':[]}
        result=focus(files,run)
        projected=result['runtime/experiment-1/step.stdout.txt']
        self.assertIn('selected-record: bad value',projected)
        self.assertIn('RuntimeError: counterevidence',projected)
        self.assertIn('omitted',projected.lower())
        self.assertLess(len(projected),len(body))
        self.assertEqual(files['runtime/experiment-1/step.stdout.txt'],body)
        self.assertEqual(result['solution/solve.sh'],body)
