import unittest
from environment_qa.context_projection import focus_external

class ContextProjectionTests(unittest.TestCase):
    def test_preserves_all_cited_lines_and_original(self):
        body='irrelevant line\n'*500+'required API\nconsumer uses API\n'+'unused\n'*500
        files={'instruction.md':'x'*150000,'external/source.txt':body}
        run={'findings':[{'path':'external/source.txt','evidence':'required API\nconsumer uses API'}]}
        projected=focus_external(files,run)
        self.assertIn('required API\nconsumer uses API',projected['external/source.txt'])
        self.assertLess(len(projected['external/source.txt']),len(body))
        self.assertEqual(files['external/source.txt'],body)
        self.assertIn('not disproven',projected['external/source.txt'])
