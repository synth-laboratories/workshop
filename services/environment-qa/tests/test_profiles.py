import unittest

from environment_qa.profiles import (PROFILES, UnknownProfile, advertise, describe, k3_policy, resolve)


class ProfileTests(unittest.TestCase):
    def test_exactly_three_profiles_are_advertised(self):
        self.assertEqual([p["id"] for p in advertise()],
                         ["k3-non-hitl", "tbench-hitl", "tbench-non-hitl"])
        for entry in advertise():
            self.assertTrue(entry["version"])
            self.assertIn(entry["mode"], {"hitl", "automated"})

    def test_the_tbench_pair_shares_one_dag(self):
        _, hitl = resolve("tbench-hitl")
        _, automated = resolve("tbench-non-hitl")
        self.assertEqual([n["id"] for n in hitl["nodes"]], [n["id"] for n in automated["nodes"]])
        self.assertEqual(hitl["id"], automated["id"])

    def test_the_tbench_pair_differs_only_by_interaction_policy(self):
        self.assertEqual(resolve("tbench-hitl")[0], "hitl")
        self.assertEqual(resolve("tbench-non-hitl")[0], "automated")

    def test_profile_identity_is_inside_the_policy_hash(self):
        # A receipt naming a profile cannot be paired with another profile's policy.
        _, hitl = resolve("tbench-hitl")
        _, automated = resolve("tbench-non-hitl")
        self.assertNotEqual(hitl["sha256"], automated["sha256"])
        self.assertEqual(hitl["profile"], "tbench-hitl")
        self.assertEqual(describe(hitl)["policy_sha256"], hitl["sha256"])

    def test_a_profile_hash_is_stable_across_resolutions(self):
        self.assertEqual(resolve("k3-non-hitl")[1]["sha256"], resolve("k3-non-hitl")[1]["sha256"])

    def test_k3_has_its_own_dag_not_the_tbench_one(self):
        _, k3 = resolve("k3-non-hitl")
        _, tbench = resolve("tbench-non-hitl")
        self.assertEqual(k3["id"], "environment-qa-k3")
        self.assertNotEqual(k3["id"], tbench["id"])

    def test_k3_runs_the_candidates_its_audit_depends_on(self):
        ids = {n["id"] for n in k3_policy()["nodes"]}
        for candidate in ("known-good", "known-good-repeat", "broken-candidate", "shortcut-candidate"):
            self.assertIn(candidate, ids)
        self.assertIn("false-accept-analysis", ids)

    def test_k3_analysis_depends_on_every_candidate(self):
        nodes = {n["id"]: n for n in k3_policy()["nodes"]}
        depends = set(nodes["false-accept-analysis"]["depends_on"])
        self.assertEqual(depends, {"known-good", "known-good-repeat",
                                   "broken-candidate", "shortcut-candidate"})

    def test_k3_shortcut_has_an_ancestral_plan_producer(self):
        nodes = {n['id']: n for n in k3_policy()['nodes']}
        shortcut = nodes['shortcut-candidate']
        plan_id = shortcut.get('plan_gate', 'probe-plan')
        self.assertIn(plan_id, nodes)
        self.assertEqual(nodes[plan_id]['executor'], 'plan')
        self.assertIn(plan_id, nodes['candidate-approval']['depends_on'])

    def test_an_unknown_profile_is_refused_not_defaulted(self):
        with self.assertRaises(UnknownProfile) as caught:
            resolve("tbench-superset")
        self.assertIn("tbench-hitl", str(caught.exception))

    def test_a_mode_that_contradicts_the_profile_is_refused(self):
        with self.assertRaisesRegex(ValueError, "runs in 'hitl' mode"):
            resolve("tbench-hitl", mode="automated")
        with self.assertRaisesRegex(ValueError, "runs in 'automated' mode"):
            resolve("k3-non-hitl", mode="hitl")

    def test_a_matching_mode_is_accepted(self):
        self.assertEqual(resolve("tbench-hitl", mode="hitl")[0], "hitl")

    def test_every_profile_pins_the_luna_model_and_refuses_release(self):
        for name in PROFILES:
            _, policy = resolve(name)
            self.assertEqual(policy["model"], "openai/gpt-5.6-luna")
            self.assertFalse(policy["allow_automated_release"])


if __name__ == "__main__":
    unittest.main()
