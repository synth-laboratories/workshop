"""Launch surface and review-actor provenance.

Two things used to be unrecoverable from a sealed run: which client launched it,
and whether "local-human" meant anything. The acceptance plan needs both — each
profile exercised through both interfaces, and real human approvals distinguished
from a CUA harness driving the same buttons — so both are recorded at the moment
they are knowable and are refused when they are not.
"""
import json
import socket
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from pathlib import Path

from environment_qa.bundles import export_bundle
from environment_qa.core import Store, verify_seal
from environment_qa.policy import full_policy
from environment_qa.worker import step


def make_task(root):
    task = root / "task"
    (task / "tests").mkdir(parents=True)
    (task / "instruction.md").write_text("Create a source file.")
    (task / "task.toml").write_text('version="1.0"\n')
    (task / "tests/test.sh").write_text("#!/bin/sh\nexit 0\n")
    return task


class SurfaceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.task = make_task(self.root)
        self.store = Store(self.root / "store")
        self.bundle = export_bundle(self.task, self.store.root, [self.root])

    def tearDown(self):
        self.temp.cleanup()

    def test_a_profile_run_without_a_surface_is_refused(self):
        # Silently recording "unknown" would let a paid acceptance run finish and
        # contribute nothing to the both-interfaces evidence it was bought for.
        with self.assertRaisesRegex(ValueError, "launch surface"):
            self.store.create(self.bundle, reviewer="ai", budget_usd=1, pipeline=full_policy())

    def test_an_unknown_surface_is_refused(self):
        with self.assertRaisesRegex(ValueError, "Unknown launch surface"):
            self.store.create(self.bundle, surface="somewhere-else")

    def test_the_surface_is_recorded_and_survives_the_seal(self):
        run = self.store.create(self.bundle, surface="workshop-embed", surface_attested="workshop-embed")
        self.assertEqual(run["policy"]["surface"],
                         {"claimed": "workshop-embed", "attested": "workshop-embed", "agrees": True})
        while step(self.store, run["id"]):
            pass
        sealed = self.store.get(run["id"])
        self.assertTrue(verify_seal(sealed))
        self.assertEqual(sealed["policy"]["surface"]["claimed"], "workshop-embed")
        # The seal covers `policy`, so a rewritten surface stops verifying rather
        # than quietly re-labelling which interface produced the run.
        sealed["policy"]["surface"]["claimed"] = "standalone-web"
        self.assertFalse(verify_seal(sealed))

    def test_a_disagreeing_attestation_is_recorded_not_resolved(self):
        # The service does not decide which side lied; it records that they differ.
        run = self.store.create(self.bundle, surface="standalone-web", surface_attested="workshop-embed")
        self.assertEqual(run["policy"]["surface"]["agrees"], False)

    def test_a_missing_attestation_is_absence_not_agreement(self):
        run = self.store.create(self.bundle, surface="cli")
        self.assertEqual(run["policy"]["surface"], {"claimed": "cli", "attested": None, "agrees": None})

    def test_the_creation_event_carries_the_surface(self):
        run = self.store.create(self.bundle, surface="standalone-web")
        created = [e for e in self.store.events(run["id"]) if e["kind"] == "run.created"]
        self.assertEqual(created[0]["payload"]["surface"]["claimed"], "standalone-web")

    def test_reusing_an_idempotency_key_from_another_surface_conflicts(self):
        from environment_qa.core import Conflict
        self.store.create(self.bundle, surface="standalone-web", request_key="same")
        with self.assertRaises(Conflict):
            self.store.create(self.bundle, surface="workshop-embed", request_key="same")


class ActorAssuranceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.task = make_task(self.root)
        self.store = Store(self.root / "store")
        self.bundle = export_bundle(self.task, self.store.root, [self.root])

    def tearDown(self):
        self.temp.cleanup()

    def waiting(self):
        run = self.store.create(self.bundle, mode="hitl", surface="test")
        while step(self.store, run["id"]):
            pass
        run = self.store.get(run["id"])
        return run, next(i for i in run["interactions"] if i["status"] == "open")

    def test_a_decision_without_an_actor_is_refused(self):
        # The old default turned a caller that said nothing into a person.
        run, interaction = self.waiting()
        with self.assertRaisesRegex(ValueError, "explicit review actor"):
            self.store.decide(run["id"], interaction["id"], "confirm", "checked",
                              interaction["context_digest"], run["revision"], "k")

    def test_an_unverified_human_decision_says_so(self):
        run, interaction = self.waiting()
        decided = self.store.decide(run["id"], interaction["id"], "confirm", "checked",
                                    interaction["context_digest"], run["revision"], "k",
                                    actor="local-human")
        resolved = next(i for i in decided["interactions"] if i["id"] == interaction["id"])
        self.assertEqual(resolved["actor"], "local-human")
        self.assertEqual(resolved["actor_assurance"], "unverified-client-claim")

    def test_a_token_backed_human_decision_is_marked_verified(self):
        run, interaction = self.waiting()
        decided = self.store.decide(run["id"], interaction["id"], "confirm", "checked",
                                    interaction["context_digest"], run["revision"], "k",
                                    actor="local-human", assurance="operator-token")
        resolved = next(i for i in decided["interactions"] if i["id"] == interaction["id"])
        self.assertEqual(resolved["actor_assurance"], "operator-token")

    def test_the_token_cannot_promote_an_agent_decision(self):
        # A CUA client holding the operator secret is still a CUA client.
        run, interaction = self.waiting()
        decided = self.store.decide(run["id"], interaction["id"], "confirm", "checked",
                                    interaction["context_digest"], run["revision"], "k",
                                    actor="agent-cua", assurance="operator-token")
        resolved = next(i for i in decided["interactions"] if i["id"] == interaction["id"])
        self.assertEqual(resolved["actor_assurance"], "agent")


class CertificateTests(unittest.TestCase):
    """The certificate reports claimed and verified human decisions separately."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.task = make_task(self.root)
        self.store = Store(self.root / "store")
        self.bundle = export_bundle(self.task, self.store.root, [self.root])

    def tearDown(self):
        self.temp.cleanup()

    def sealed(self, actor, assurance):
        from environment_qa.dag import decide, run_until_idle

        def fake(store, run, gate, path):
            return {"findings": [], "limitations": [],
                    "interaction": gate.get("role") in {"specification", "domain", "technical"}}

        run = self.store.create(self.bundle, mode="hitl", reviewer="ai",
                                pipeline=full_policy(), surface="workshop-embed")
        while True:
            current = run_until_idle(self.store, run["id"], fake)
            open_ones = [i for i in current["interactions"] if i["status"] == "open"]
            if not open_ones:
                return current
            for interaction in open_ones:
                decide(self.store, run["id"], interaction["id"], "confirm", "reviewed",
                       interaction["context_digest"], current["revision"], interaction["id"],
                       actor=actor, assurance=assurance)

    def test_cua_decisions_count_as_zero_verified_humans(self):
        run = self.sealed("agent-cua", "unverified-client-claim")
        self.assertGreater(len(run["interactions"]), 0)
        self.assertEqual(run["certificate"]["human_decision_count"], 0)
        self.assertEqual(run["certificate"]["verified_human_decision_count"], 0)

    def test_an_unverified_human_claim_is_counted_but_not_verified(self):
        run = self.sealed("local-human", "unverified-client-claim")
        self.assertGreater(run["certificate"]["human_decision_count"], 0)
        self.assertEqual(run["certificate"]["verified_human_decision_count"], 0)

    def test_a_verified_human_is_counted_on_both(self):
        run = self.sealed("local-human", "operator-token")
        count = run["certificate"]["human_decision_count"]
        self.assertGreater(count, 0)
        self.assertEqual(run["certificate"]["verified_human_decision_count"], count)

    def test_the_certificate_carries_the_launch_surface(self):
        run = self.sealed("agent-cua", "unverified-client-claim")
        self.assertEqual(run["certificate"]["launch_surface"]["claimed"], "workshop-embed")


class HttpProvenanceTests(unittest.TestCase):
    """The attestation the browser supplies, exercised over the real HTTP path.

    Page script cannot set Referer, so it is the one part of the surface claim the
    client cannot author. These tests drive the actual server rather than the
    helper, because the helper's value is entirely in being wired to the request.
    """

    @classmethod
    def setUpClass(cls):
        from environment_qa.server import serve
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temp.name)
        cls.task = make_task(cls.root)
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            cls.port = probe.getsockname()[1]
        cls.started = threading.Event()
        cls.thread = threading.Thread(
            target=serve, args=(cls.root / "store", [cls.root], cls.port),
            kwargs={"operator_token": "operator-secret",
                    "on_start": lambda server: (setattr(cls, "server", server), cls.started.set())},
            daemon=True)
        cls.thread.start()
        cls.base = f"http://127.0.0.1:{cls.port}"
        for _ in range(200):
            try:
                urllib.request.urlopen(cls.base + "/health", timeout=1).read()
                break
            except (urllib.error.URLError, ConnectionError):
                threading.Event().wait(0.05)
        else:
            raise AssertionError("QA service did not start")
        shell = urllib.request.urlopen(cls.base + "/", timeout=5).read().decode()
        cls.token = shell.split('name="qa-token" content="')[1].split('"')[0]

    @classmethod
    def tearDownClass(cls):
        # Stop the service before the store directory disappears underneath its
        # worker; otherwise the thread logs sqlite errors for the rest of the run.
        if cls.started.wait(5):
            cls.server.shutdown()
        cls.thread.join(timeout=30)
        cls.temp.cleanup()

    def post(self, path, body, **headers):
        request = urllib.request.Request(
            self.base + path, data=json.dumps(body).encode(), method="POST",
            headers={"X-QA-Token": self.token, "Content-Type": "application/json", **headers})
        return json.loads(urllib.request.urlopen(request, timeout=10).read())

    def create(self, surface, referer=None):
        headers = {"Referer": referer} if referer else {}
        return self.post("/api/runs", {"task_path": str(self.task), "surface": surface,
                                       "request_key": surface + str(referer)}, **headers)

    def test_an_embedded_referer_attests_the_workshop_surface(self):
        run = self.create("workshop-embed",
                          f"{self.base}/?embed=workshop&mode=hitl&parentOrigin=tauri%3A%2F%2Flocalhost")
        self.assertEqual(run["policy"]["surface"],
                         {"claimed": "workshop-embed", "attested": "workshop-embed", "agrees": True})

    def test_a_top_level_referer_attests_the_standalone_surface(self):
        run = self.create("standalone-web", f"{self.base}/?mode=hitl")
        self.assertEqual(run["policy"]["surface"]["attested"], "standalone-web")

    def test_a_claim_the_referer_contradicts_is_recorded_as_disagreeing(self):
        run = self.create("workshop-embed", f"{self.base}/?mode=hitl")
        self.assertEqual(run["policy"]["surface"],
                         {"claimed": "workshop-embed", "attested": "standalone-web", "agrees": False})

    def test_a_foreign_referer_attests_nothing(self):
        run = self.create("standalone-web", "http://example.invalid/?embed=workshop")
        self.assertIsNone(run["policy"]["surface"]["attested"])

    def test_config_advertises_that_an_operator_token_is_required(self):
        request = urllib.request.Request(self.base + "/api/config", headers={"X-QA-Token": self.token})
        config = json.loads(urllib.request.urlopen(request, timeout=10).read())
        self.assertTrue(config["operator_token_required"])


if __name__ == "__main__":
    unittest.main()
