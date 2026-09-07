import unittest

from environment_qa.freeze import REQUIRED_THRESHOLDS, engine_freeze, manifest_draft, missing


class FreezeTests(unittest.TestCase):
    def test_the_freeze_pins_every_profile_by_policy_hash(self):
        freeze = engine_freeze()
        self.assertEqual(sorted(freeze["profiles"]), ["k3-non-hitl", "tbench-hitl", "tbench-non-hitl"])
        for body in freeze["profiles"].values():
            self.assertTrue(body["policy_sha256"])
            self.assertEqual(body["model"], "openai/gpt-5.6-luna")

    def test_engine_and_client_are_digested_separately(self):
        freeze = engine_freeze()
        self.assertNotEqual(freeze["engine_sha256"], freeze["client_sha256"])
        self.assertIn("codex_executor.py", freeze["sources"])
        self.assertIn("app.js", freeze["client"])

    def test_the_freeze_is_stable_for_unchanged_bytes(self):
        self.assertEqual(engine_freeze()["engine_sha256"], engine_freeze()["engine_sha256"])


class ManifestTests(unittest.TestCase):
    def test_a_draft_declares_no_thresholds(self):
        # Inventing one would manufacture the approval the manifest records.
        manifest = manifest_draft()
        self.assertEqual(set(manifest["thresholds"]), set(REQUIRED_THRESHOLDS))
        self.assertTrue(all(v is None for v in manifest["thresholds"].values()))

    def test_a_draft_is_never_ready_for_acceptance(self):
        manifest = manifest_draft()
        self.assertFalse(manifest["ready_for_acceptance"])
        self.assertTrue(manifest["missing"])

    def test_every_unset_requirement_is_named(self):
        manifest = manifest_draft()
        for expected in ("acceptance_owner", "operator_signoff", "accounting.unit", "accounting.ceiling"):
            self.assertIn(expected, manifest["missing"])
        for name in REQUIRED_THRESHOLDS:
            self.assertIn(f"thresholds.{name}", manifest["missing"])

    def test_all_three_stages_are_present_in_order(self):
        stages = manifest_draft()["stages"]
        self.assertEqual(sorted(stages), ["A", "B", "C"])
        self.assertEqual([stages[k]["cohort"] for k in ("A", "B", "C")],
                         ["expanded-tbench", "cybernetics", "reb"])

    def test_supplied_cohort_content_is_carried_but_does_not_sign_it_off(self):
        manifest = manifest_draft({"expanded-tbench": {"tasks": ["case-01", "case-05"],
                                                       "task_hashes": {"case-01": "abc"}}})
        self.assertEqual(manifest["stages"]["A"]["tasks"], ["case-01", "case-05"])
        self.assertFalse(manifest["ready_for_acceptance"])
        self.assertIn("stages.A.development", manifest["missing"])

    def test_a_fully_completed_manifest_reports_ready(self):
        manifest = manifest_draft()
        manifest["thresholds"] = {name: 0.5 for name in REQUIRED_THRESHOLDS}
        manifest["accounting"] = {"unit": "tokens", "ceiling": 100000, "rationale": "subscription"}
        manifest["runtime"].update(concurrency=2, per_gate_deadline_seconds=600,
                                   retry_allowance=1, host_baseline="idle")
        manifest["acceptance_owner"] = "an operator"
        manifest["operator_signoff"] = {"actor":"an operator", "at":"2026-09-06T13:00:00Z"}
        for stage in manifest["stages"].values():
            tasks = [f"t{i}" for i in range(21)]
            stage.update(tasks=tasks, development=tasks[:20], held_out=tasks[20:],
                         task_hashes={t: "a" * 64 for t in tasks},
                         profile_assignments={t: list(("tbench-non-hitl", "tbench-hitl", "k3-non-hitl")) for t in tasks},
                         hitl_paired_subset=[tasks[0]], k3_verifier_subset=[tasks[0]],
                         controls=[{"task_id":tasks[0], "provenance":"fixture", "label":"known-good"}],
                         gold_defects=[])
        self.assertEqual(missing(manifest), [])

    def test_deleted_structure_and_invalid_values_are_not_ready(self):
        manifest = manifest_draft()
        manifest.update(thresholds={}, runtime={}, stages={}, acceptance_owner="", operator_signoff="")
        manifest["accounting"] = {"unit":"nonsense", "ceiling":-1}
        gaps = missing(manifest)
        self.assertIn("stages.A", gaps)
        self.assertIn("thresholds.min_reference_recovery", gaps)
        self.assertIn("runtime.model", gaps)
        self.assertIn("accounting.ceiling", gaps)
        self.assertIn("operator_signoff", gaps)

    def test_the_rules_that_prevent_retrofitting_are_recorded(self):
        rules = " ".join(manifest_draft()["rules"]).lower()
        self.assertIn("never adjusted after results", rules)
        self.assertIn("agent-cua", rules)
        self.assertIn("first-attempt", rules)


if __name__ == "__main__":
    unittest.main()
