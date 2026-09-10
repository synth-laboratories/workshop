import unittest
from environment_qa.prose_quotes import recover, recover_indentation


class ProseQuotesTests(unittest.TestCase):
    def test_uniform_code_indent_recovery_preserves_relative_structure(self):
        source='def validate():\n    check()\n    return True\n'
        proposed='    def validate():\n        check()'
        self.assertEqual(recover_indentation(source,proposed),'def validate():\n    check()')
        self.assertIsNone(recover_indentation(source,'    def validate():\n    check()'))
        self.assertIsNone(recover_indentation(source,'def validate(): check()'))
        self.assertIsNone(recover_indentation(source,'def validate():\n    other()'))

    def test_wrapped_bullet_restored_to_exact_source(self):
        source = ' * Required values must match the supplied\n   reference data exactly.\n'
        result = recover(source, '* Required values must match the supplied reference data exactly.')
        self.assertIn(result, source)
        self.assertIn('\n   reference', result)

    def test_added_bullet_on_continuation_sentence(self):
        source = ' * First requirement.\n   Returned fields must include the required identifier.\n'
        self.assertEqual(recover(source, '* Returned fields must include the required identifier.'),
                         'Returned fields must include the required identifier.')

    def test_changed_words_are_not_repaired(self):
        self.assertIsNone(recover('Required values may differ from reference data.',
                                  'Required values must match reference data.'))

    def test_ambiguous_normalized_quote_is_rejected(self):
        self.assertIsNone(recover('one two\nthree four\none two\nthree four', 'one two three four'))

    def test_exact_and_empty(self):
        self.assertEqual(recover('a b', 'a b'), 'a b')
        self.assertIsNone(recover('anything', ''))
