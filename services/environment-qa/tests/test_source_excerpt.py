import unittest
from environment_qa.source_inventory import source_excerpt

class SourceExcerptTests(unittest.TestCase):
    def test_build_rule_survives_long_makefile_header(self):
        source='# header\n'*500+'build/compiler:\n\tcurl -o $@ https://example.invalid/compiler\n'+'# tail\n'*500
        excerpt=source_excerpt(source,3000)
        self.assertIn('build/compiler:',excerpt)
        self.assertIn('curl -o $@',excerpt)
        self.assertLessEqual(len(excerpt),3000)
    def test_rare_dependency_call_survives_large_module(self):
        source='import common\nimport rare\n'+'common.call()\n'*1000+'rare.layout()["node_attribute"]\n'
        excerpt=source_excerpt(source,3000)
        self.assertIn('rare.layout()["node_attribute"]',excerpt)
        self.assertLessEqual(len(excerpt),3000)
