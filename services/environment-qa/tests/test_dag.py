import tempfile
import time
import unittest
from pathlib import Path
from environment_qa.core import Store, Conflict, verify_seal
from environment_qa.bundles import export_bundle
from environment_qa.policy import full_policy, validate
from environment_qa.dag import claim, complete, run_until_idle, decide, expire


class DagTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.task = self.root/"task"
        self.task.mkdir()
        (self.task/"instruction.md").write_text("Test fixture")
        (self.task/"task.toml").write_text('version="1"')
        self.store = Store(self.root/"store")
        self.bundle = export_bundle(self.task,self.store.root,[self.root])

    def tearDown(self): self.tmp.cleanup()

    def create(self, mode="automated"):
        return self.store.create(self.bundle,mode=mode,reviewer="ai",pipeline=full_policy(),surface="test")

    @staticmethod
    def fake(store, run, gate, path):
        if gate["executor"] == "interaction" and run["mode"] == "hitl": return {"interaction":True}
        return {"findings":[],"limitations":[]}

    def test_full_dag_seals_and_legacy_remains_readable(self):
        run = self.create()
        result = run_until_idle(self.store,run["id"],self.fake)
        self.assertTrue(verify_seal(result))
        self.assertTrue(all(g["status"] == "succeeded" for g in result["gates"]))
        self.assertEqual(len(result["evidence"]),len(result["gates"]))

    def test_targeted_missing_review_does_not_cancel_probe(self):
        from environment_qa.policy import targeted_policy
        for failed_gate in ('environment-review-second-pass','admission'):
            with self.subTest(gate=failed_gate):
                run=self.store.create(self.bundle,reviewer='ai',pipeline=targeted_policy(),surface='test')
                seen=[]
                def execute(store,current,gate,path):
                    seen.append(gate['id'])
                    return {'gate_status':'inconclusive','limitations':['Unavailable evidence']} if gate['id']==failed_gate else self.fake(store,current,gate,path)
                result=run_until_idle(self.store,run['id'],execute)
                if failed_gate=='admission':self.assertNotIn('experiment-1',seen)
                else:self.assertIn('experiment-1',seen)
                self.assertNotEqual(result['verdict'],'pass')

    def test_targeted_human_probe_refusal_still_blocks_execution(self):
        from environment_qa.policy import targeted_policy
        run=self.store.create(self.bundle,reviewer='ai',pipeline=targeted_policy(),surface='test')
        seen=[]
        def execute(store,current,gate,path):
            seen.append(gate['id'])
            return {'gate_status':'failed'} if gate['id']=='probe-approval' else self.fake(store,current,gate,path)
        run_until_idle(self.store,run['id'],execute)
        self.assertNotIn('experiment-1',seen)

    def test_critic_disagreement_retains_unresolved_finding(self):
        run=self.create()
        def execute(store,current,gate,path):
            if gate['id']=='structure':
                return {'findings':[{'id':'f','title':'conditional failure','severity':'warning','disposition':'proposed'}]}
            if gate['id'] in {'attribution','critic'}:
                return {'dispositions':[{'finding_id':current['findings'][0]['id'],
                    'status':'dismissed' if gate['id']=='attribution' else 'confirmed','reason':'independent assessment'}]}
            return self.fake(store,current,gate,path)
        result=run_until_idle(self.store,run['id'],execute)
        self.assertEqual(result['findings'][0]['disposition'],'unresolved')
        self.assertEqual(len(result['findings'][0]['assessments']),2)

    def test_dependency_and_fencing(self):
        run = self.create()
        g, token = claim(self.store,run["id"])
        self.assertEqual(g["id"],"admission")
        self.assertIsNone(claim(self.store,run["id"]))
        with self.assertRaises(Conflict): complete(self.store,run["id"],g,"stale",{})
        complete(self.store,run["id"],g,token,{})
        claimed = [claim(self.store,run["id"])[0]["id"] for _ in range(3)]
        self.assertEqual(set(claimed),{"structure","specification","verifier"})

    def test_failed_prerequisite_never_runs_descendants(self):
        run = self.create()
        seen = []
        def executor(store,run,gate,path):
            seen.append(gate["id"])
            return {"gate_status":"failed"} if gate["id"] == "admission" else self.fake(store,run,gate,path)
        result = run_until_idle(self.store,run["id"],executor)
        self.assertNotIn("build",seen)
        self.assertNotEqual(result["verdict"],"pass")
        self.assertTrue(verify_seal(result))

    def test_agent_operated_gates_do_not_count_as_human(self):
        run = self.create("hitl")
        for _ in range(10):
            run = run_until_idle(self.store, run["id"], self.fake)
            if run["seal"]:
                break
            for interaction in run["interactions"]:
                if interaction["status"] == "open":
                    self.store.decide(run["id"], interaction["id"], "confirm", "CUA integration test only",
                                      interaction["context_digest"], run["revision"], interaction["id"], actor="agent-cua")
        self.assertTrue(verify_seal(run))
        self.assertGreater(len(run["interactions"]), 0)
        self.assertTrue(all(i["actor"] == "agent-cua" for i in run["interactions"]))
        self.assertEqual(run["certificate"]["human_decision_count"], 0)

    def test_hitl_multiple_gates_stale_and_idempotent(self):
        run = self.create("hitl")
        result = run_until_idle(self.store,run["id"],self.fake)
        self.assertFalse(result["seal"])
        i = next(i for i in result["interactions"] if i["status"] == "open")
        args = (self.store,run["id"],i["id"],"confirm","Evidence checked",i["context_digest"],result["revision"],"once")
        decided = decide(*args,actor="local-human")
        self.assertEqual(decide(*args,actor="local-human"),decided)
        result = run_until_idle(self.store,run["id"],self.fake)
        self.assertEqual(sum(i["status"]=="open" for i in result["interactions"]),1)
        i = next(i for i in result["interactions"] if i["status"] == "open")
        decide(self.store,run["id"],i["id"],"confirm","Targeted evidence permitted",i["context_digest"],result["revision"],"targeted",actor="local-human")
        result = run_until_idle(self.store,run["id"],self.fake)
        self.assertEqual(sum(i["status"]=="open" for i in result["interactions"]),2)

    def test_pause_stops_dispatch(self):
        run = self.create()
        paused = self.store.control(run["id"],"pause",run["revision"],"pause")
        self.assertIsNone(claim(self.store,run["id"]))
        self.store.control(run["id"],"resume",paused["revision"],"resume")
        self.assertIsNotNone(claim(self.store,run["id"]))

    def test_expired_interaction_never_approves(self):
        run = self.create("hitl")
        result = run_until_idle(self.store,run["id"],self.fake)
        interaction = next(i for i in result["interactions"] if i["status"] == "open")
        self.store.mutate(run["id"],"test.expire",lambda r: next(i for i in r["interactions"] if i["id"]==interaction["id"]).update(expires_at=0))
        expire(self.store,run["id"])
        current = self.store.get(run["id"])
        self.assertEqual(current["interactions"][0]["status"],"expired")
        self.assertEqual(next(g for g in current["gates"] if g["id"]==interaction["gate_id"])["status"],"inconclusive")

    def test_cancel_fences_new_dispatch(self):
        run = self.create()
        gate,token = claim(self.store,run["id"])
        run = self.store.get(run["id"])
        self.store.control(run["id"],"cancel",run["revision"],"cancel")
        self.assertIsNone(claim(self.store,run["id"]))
        complete(self.store,run["id"],gate,token,{"findings":[]})
        self.assertEqual(self.store.get(run["id"])["status"],"cancelled")

    def test_sealed_import_is_idempotent(self):
        from environment_qa.importing import import_run
        run = self.create()
        result = run_until_idle(self.store,run["id"],self.fake)
        destination = Store(self.root/"destination")
        for _ in range(2):
            self.assertEqual(import_run(self.store,destination,run["id"]),result)
        self.assertTrue(verify_seal(destination.get(run["id"])))

    def test_policy_rejects_cycles_and_other_models(self):
        p = full_policy()
        p["nodes"][0]["depends_on"] = ["disposition"]
        with self.assertRaises(ValueError): validate(p)
        p = full_policy(); p["model"] = "anthropic/claude"
        with self.assertRaises(ValueError): validate(p)
