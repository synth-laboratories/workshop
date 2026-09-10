import tempfile
import unittest
from pathlib import Path
from environment_qa.environment_checks import check_environment

class AnswerVisibilityTests(unittest.TestCase):
    def test_docker_staged_hash_reference_is_flagged_conditionally(self):
        with tempfile.TemporaryDirectory() as temp:
            p=Path(temp);(p/'environment/resources').mkdir(parents=True);(p/'tests').mkdir()
            (p/'environment/resources/answer').write_text('answer')
            (p/'environment/Dockerfile').write_text('COPY resources /app/resources\n')
            (p/'tests/test.py').write_text('def compare(a,b):\n assert hash_file(a)==hash_file(b)\ndef test_output():\n expected="/app/resources/answer"\n actual="/app/output"\n compare(expected,actual)\n')
            findings=check_environment(p)
            self.assertEqual(len(findings),1)
            self.assertEqual(findings[0]['severity'],'warning')
            self.assertIn('if setup retains',findings[0]['title'])
            (p/'environment/Dockerfile').write_text('FROM scratch\n')
            self.assertEqual(check_environment(p),[])
