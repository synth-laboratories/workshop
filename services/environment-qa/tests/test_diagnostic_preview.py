import unittest
from environment_qa.runtime import diagnostic_preview

class DiagnosticPreviewTests(unittest.TestCase):
    def test_keeps_middle_error_and_final_state(self):
        text='progress\n'*2000+'ERROR: prerequisite unavailable\n'+'progress\n'*2000+'FINAL'
        excerpt=diagnostic_preview(text)
        self.assertIn('ERROR: prerequisite unavailable',excerpt)
        self.assertTrue(excerpt.endswith('FINAL'))
        self.assertLess(len(excerpt),13000)
    def test_small_output_unchanged(self):
        self.assertEqual(diagnostic_preview(b'hello'),'hello')
    def test_compiler_flags_do_not_hide_actual_error(self):
        text=('g++ -Werror=format-security -Wno-error compile.cpp\n'*3000)+ 'error: library requires missing build tool\n'+('progress\n'*2000)
        self.assertIn('error: library requires missing build tool',diagnostic_preview(text))
