import unittest
from environment_qa.policy import targeted_policy

class SpecialistScopes(unittest.TestCase):
    def test_assignments_are_distinct_and_all_feed_planner(self):
        nodes=targeted_policy()['nodes']
        scopes={n['id']:n['review_scope'] for n in nodes if 'review_scope' in n}
        self.assertEqual(len(scopes),16)
        self.assertEqual(len(set(scopes.values())),8)
        planner=next(n for n in nodes if n['id']=='probe-plan')
        self.assertTrue(set(scopes)<=set(planner['depends_on']))
        self.assertIn('transitive',scopes['environment-review'])
        self.assertIn('NEXT connection',scopes['environment-review-independent'])
        self.assertTrue(all(n.get('finding_limit')==4 for n in nodes if 'review_scope' in n))
