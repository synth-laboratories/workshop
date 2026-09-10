import unittest
from environment_qa.source_toolchain import plan

class ToolchainTests(unittest.TestCase):
    def test_source_pins_and_allowlist_are_preserved(self):
        source='sudo apt-get install -y --no-install-recommends \\\n build-essential=12.10ubuntu1 \\\n curl=1.0 git make && dangerous-command\n'
        result=plan({'solution/solve.sh':source},'tiny extension compilation and git ls-remote')
        self.assertEqual(result['packages'],['build-essential=12.10ubuntu1','git','make'])
        self.assertNotIn('curl',result['command'])
        self.assertNotIn('dangerous-command',result['command'])
        self.assertIn('build-essential=12.10ubuntu1',result['command'])
        self.assertNotIn('sudo',result['command'])

    def test_no_implicit_toolchain_for_data_or_undeclared_setup(self):
        self.assertIsNone(plan({'solution/solve.sh':'apt-get install build-essential'},'read CSV'))
        self.assertIsNone(plan({'solution/solve.sh':'# apt-get install build-essential'},'tiny extension'))
        self.assertIsNone(plan({'instruction.md':'apt-get install build-essential'},'tiny extension'))
        self.assertIsNone(plan({'solution/solve.sh':'apt-get install build-essential=${VERSION}'},'tiny extension'))
