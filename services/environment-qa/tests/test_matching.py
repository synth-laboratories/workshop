import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from environment_qa.core import Store
from environment_qa.bundles import export_bundle
from environment_qa.policy import full_policy
from environment_qa.dag import run_until_idle
from environment_qa.matching import compare


class MatchingTests(unittest.TestCase):
    def test_compound_reference_uses_disjoint_supporting_findings(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);task=root/'task';task.mkdir()
            (task/'instruction.md').write_text('Fixture');(task/'task.toml').write_text('version="1"')
            store=Store(root/'detector')
            run=store.create(export_bundle(task,store.root,[task]),reviewer='ai',pipeline=full_policy(),surface='test')
            def execute(s,r,g,p):
                return {'findings':[dict(id=str(i),title='concrete '+str(i),mechanism='mechanism_'+str(i),path='instruction.md',evidence='Fixture',severity='warning',disposition='proposed') for i in range(2)] if g['id']=='structure' else [],'limitations':[]}
            run=run_until_idle(store,run['id'],execute);ids=[f['id'] for f in run['findings']]
            gold={'case_id':'fixture','defects':[{'id':'reference','status':'provisional'}]}
            response={'matches':[{'prediction_id':ids[0],'supporting_prediction_ids':[ids[1]],'gold_id':'reference','coverage':'full','reason':'Two concrete clauses supported'}],'limitations':[]}
            with patch('environment_qa.matching.request_json',return_value=response):
                report=compare(run,gold,root/'ok',1)
            self.assertEqual(report['unmatched_predictions'],[])
            response['matches'][0]['supporting_prediction_ids']=[ids[0]]
            with patch('environment_qa.matching.request_json',return_value=response):
                with self.assertRaisesRegex(ValueError,'disjoint'): compare(run,gold,root/'duplicate',1)

    def test_matching_requires_seal_and_does_not_score_provisional_gold(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); task = root/'task'; task.mkdir()
            (task/'instruction.md').write_text('Fixture')
            (task/'task.toml').write_text('version="1.0"')
            store = Store(root/'detector')
            run = store.create(export_bundle(task,store.root,[task]),reviewer='ai',pipeline=full_policy(),surface='test')
            gold = {'case_id':'fixture','defects':[{'id':'reference','status':'provisional'}]}
            with self.assertRaises(ValueError): compare(run,gold,root/'unsealed',1)
            run = run_until_idle(store,run['id'],lambda *args: {'findings':[],'limitations':[]})
            with patch('environment_qa.matching.request_json',return_value={'matches':[],'limitations':[]}):
                report = compare(run,gold,root/'scorer',1)
            self.assertEqual(report['missed_gold_ids'],['reference'])
            self.assertIsNone(report['primary_recall'])
            self.assertFalse(report['human_confirmed'])
            self.assertEqual(store.get(run['id']),run)
            self.assertFalse((store.root/'private-context').exists())
