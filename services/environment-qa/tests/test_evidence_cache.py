import hashlib
import tempfile
import unittest
from pathlib import Path
from environment_qa.core import Store
from environment_qa.bundles import export_bundle
from environment_qa.policy import full_policy,validate
from environment_qa.dag import run_until_idle
from environment_qa.evidence_cache import oracle_evidence

class CacheTests(unittest.TestCase):
    def test_cache_rejects_changes_and_excludes_prior_opinions(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);task=root/'task';task.mkdir()
            (task/'instruction.md').write_text('fixture')
            (task/'task.toml').write_text('version="1"')
            store=Store(root/'store');bundle=export_bundle(task,store.root,[task])
            artifact=store.root/'oracle.txt';artifact.write_text('ERROR: observed failure')
            policy=full_policy();policy['nodes']=[dict(id='oracle-1',executor='trial',depends_on=[],required=True,mode='oracle')]
            run=store.create(bundle,reviewer='ai',pipeline=validate(policy),surface='test')
            prior=run_until_idle(store,run['id'],lambda *args:{'findings':[],
                'private_opinion':'must not leak','artifacts':[{'path':'oracle.txt','sha256':hashlib.sha256(artifact.read_bytes()).hexdigest()}]})
            cached=oracle_evidence(store,prior,bundle)
            self.assertEqual(len(cached['findings']),1)
            self.assertIn('Observed reference execution diagnostic',cached['findings'][0]['title'])
            self.assertNotIn('must not leak',str(cached))
            self.assertIn('ERROR: observed failure',str(cached['documents']))
            with self.assertRaises(ValueError): oracle_evidence(store,prior,dict(bundle,sha256='wrong'))
            artifact.write_text('tampered')
            with self.assertRaises(ValueError): oracle_evidence(store,prior,bundle)
