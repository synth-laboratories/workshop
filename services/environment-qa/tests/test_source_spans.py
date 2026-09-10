import unittest
from environment_qa.source_spans import extract, numbered

class SourceSpanTests(unittest.TestCase):
    def test_explicit_lines_preserve_original_code(self):
        source='def run():\n    configure()\n    execute()\n'
        self.assertEqual(extract(source,{'start':2,'end':3}),'    configure()\n    execute()')
        self.assertIn('[2]     configure()',numbered(source))

    def test_invalid_ranges_do_not_guess(self):
        for span in [{'start':0,'end':1},{'start':1,'end':2},{'start':'1','end':1},{'start':True,'end':1},{'start':2,'end':1}]:
            self.assertIsNone(extract('one line',span))
        self.assertIsNone(extract('x\n'*25,{'start':1,'end':25}))
