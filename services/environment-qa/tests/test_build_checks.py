import unittest
from environment_qa.build_checks import destructive_target_overlap

class BuildOwnershipTests(unittest.TestCase):
    def test_parallel_parent_cleanup_can_destroy_child_target(self):
        source='VER := 1\noutput/include: build/source-$(VER)\n\trm -rf $@\n\tcp -r source $@\noutput/include/child: build/child\n\tmkdir -p output/include\n\tcp child $@\n'
        result=destructive_target_overlap('Makefile',source)
        self.assertEqual(len(result),1)
        self.assertEqual(result[0]['severity'],'warning')
        self.assertIn('output/include/child',result[0]['causal_claim'])
        self.assertIn(result[0]['evidence'],source)
        for quote in result[0]['supporting_evidence']:self.assertIn(quote['evidence'],source)
    def test_explicit_order_prevents_overlap_warning(self):
        source='output/include:\n\trm -rf $@\noutput/include/child: | output/include\n\tcp child $@\n'
        self.assertEqual(destructive_target_overlap('Makefile',source),[])
    def test_unresolved_rule_is_not_assumed_independent(self):
        source='output/include:\n\trm -rf $@\noutput/include/child: $(UNKNOWN)\n\tcp child $@\n'
        self.assertEqual(destructive_target_overlap('Makefile',source),[])
