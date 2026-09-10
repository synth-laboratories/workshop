import json
import unittest
from environment_qa.runtime import trajectory_preview

class PreviewTests(unittest.TestCase):
    def test_final_result_survives_long_early_output(self):
        events=[{'observation':{'stdout':'x'*20000,'stderr':'','return_code':0}} for _ in range(8)]
        events.append({'termination':'step_budget_exhausted'})
        view=json.loads(trajectory_preview(json.dumps(events)))
        self.assertEqual(view['omitted_earlier_events'],0)
        self.assertEqual(view['events'][-1]['termination'],'step_budget_exhausted')
        self.assertIn('projection',view['events'][0]['observation']['stdout'])

    def test_early_measurement_not_erased_by_build_tail(self):
        text='CHECKED API keys: vertex_position\n'+'install chatter\n'*1500+'FAILED compiler\n'
        view=json.loads(trajectory_preview(json.dumps([{'observation':{'stdout':text}}])))
        self.assertIn('CHECKED API keys: vertex_position',view['events'][0]['observation']['stdout'])
        self.assertIn('FAILED compiler',view['events'][0]['observation']['stdout'])
