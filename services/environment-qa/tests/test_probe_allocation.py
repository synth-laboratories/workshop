import json
import unittest
from environment_qa.probe_allocation import allocate


class ProbeAllocationTests(unittest.TestCase):
    def test_all_contracts_once_in_two_bounded_groups(self):
        contracts = [{'package': 'dependency-' + str(i)} for i in range(6)]
        probes, deferred = allocate(contracts, [{'hypothesis': 'optional'}])
        self.assertEqual(len(probes), 2)
        for contract in contracts:
            self.assertEqual(sum(json.dumps(contract) in p['objective'] for p in probes), 1)
        self.assertTrue(deferred)
        self.assertTrue(all('120-second' in p['objective'] for p in probes))

    def test_small_inventory_preserves_one_independent_experiment(self):
        independent = {'hypothesis': 'lifecycle'}
        probes, deferred = allocate([{'package': 'one'}], [independent])
        self.assertEqual(probes[1], independent)
        self.assertEqual(deferred, [])

    def test_no_contracts_preserves_experiments(self):
        experiments = [{'hypothesis': 'one'}, {'hypothesis': 'two'}]
        self.assertEqual(allocate([], experiments), (experiments, []))
