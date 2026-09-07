import unittest
from environment_qa.harbor import component_complete

class CompletionTests(unittest.TestCase):
    def test_instrumentation_alone_does_not_complete_a_component_probe(self):
        self.assertFalse(component_complete([{'instrumentation':'read_only_cgroup_limits','observation':{'return_code':0}},
                                             {'action':{'done':True,'command':''}}]))
    def test_failed_command_is_not_completion(self):
        self.assertFalse(component_complete([{'action':{'done':True,'command':'nonexistent'},'observation':{'return_code':127}}]))
    def test_exhaustion_is_not_completion(self):
        self.assertFalse(component_complete([{'observation':{'return_code':0}},{'termination':'step_budget_exhausted'}]))
    def test_observed_completion(self):
        self.assertTrue(component_complete([{'action':{'done':True,'command':'echo ok'},'observation':{'return_code':0}}]))
    def test_empty_done_without_observations_is_not_completion(self):
        self.assertFalse(component_complete([{'action':{'done':True,'command':''}}]))
