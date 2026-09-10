import unittest
from environment_qa.runtime_limits import COMMAND,parse


class RuntimeLimitsTests(unittest.TestCase):
    def test_v2_separates_ram_and_swap(self):
        result=parse('/sys/fs/cgroup/memory.max=4096\n/sys/fs/cgroup/memory.swap.max=8192\n')
        self.assertEqual((result['memory_limit_bytes'],result['swap_limit_bytes']),(4096,8192))
    def test_v1_total_is_not_additional_swap(self):
        result=parse('/sys/fs/cgroup/memory/memory.limit_in_bytes=4096\n/sys/fs/cgroup/memory/memory.memsw.limit_in_bytes=8192\n')
        self.assertEqual(result['swap_limit_bytes'],4096)
    def test_unknown_is_not_zero(self):
        self.assertIsNone(parse('')['swap_limit_bytes'])
        self.assertIsNone(parse('/sys/fs/cgroup/memory.swap.max=max')['swap_limit_bytes'])
        self.assertEqual(parse('/sys/fs/cgroup/memory.swap.max=0')['swap_limit_bytes'],0)
        self.assertNotIn('sudo',COMMAND)
