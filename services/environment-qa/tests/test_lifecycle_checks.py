import unittest
from environment_qa.lifecycle_checks import check

SOURCE = '''qemu-system-x86_64 -serial mon:telnet:127.0.0.1:7777,server,nowait
cat <<'ONE' > ready
spawn telnet 127.0.0.1 7777
expect "ready"
ONE
expect -f ready
cat <<'TWO' > configure
set host "localhost"
set port "7777"
spawn telnet $host $port
send "configure\\r"
TWO
expect -f configure
'''


class LifecycleChecksTests(unittest.TestCase):
    def test_next_connection_in_verifier_is_traced_without_executing_python(self):
        oracle=SOURCE.split("cat <<'TWO'")[0]
        verifier='import os\nscript = "spawn telnet localhost 7777"\nwith open("run.exp", "w") as f:\n    f.write(script)\nos.popen("expect -f run.exp").read()\n'
        findings=check('solution/solve.sh',oracle,{'tests/test_outputs.py':verifier})
        self.assertEqual(len(findings),1)
        self.assertTrue(any(s['path']=='tests/test_outputs.py' for s in findings[0]['supporting_evidence']))
        self.assertEqual(check('solution/solve.sh',oracle,{'tests/test_outputs.py':verifier.replace('f.write(script)','f.write("unrelated")')}),[])

    def test_repeated_exclusive_endpoint_is_conditional(self):
        findings=check('solution/solve.sh',SOURCE)
        self.assertEqual(len(findings),1)
        self.assertEqual(findings[0]['evidence_grade'],'conditional_source')
        self.assertIn('If that spawned client survives',findings[0]['causal_claim'])
        for evidence in [findings[0],*findings[0]['supporting_evidence']]:
            self.assertIn(evidence['evidence'],SOURCE)

    def test_visible_cleanup_suppresses_warning(self):
        self.assertEqual(check('s',SOURCE.replace('expect "ready"','expect "ready"\nclose\nwait')),[])
        self.assertEqual(check('s',SOURCE.replace('expect "ready"','expect "ready"\nexpect "telnet>"\nsend "quit\\r"')),[])
        self.assertEqual(len(check('s',SOURCE.replace('expect "ready"','expect "ready"\nsend "quit\\r"'))),1)

    def test_different_endpoint_or_no_second_invocation(self):
        self.assertEqual(check('s',SOURCE.replace('set port "7777"','set port "8888"')),[])
        self.assertEqual(check('s',SOURCE.replace('expect -f configure','')),[])

    def test_does_not_assume_arbitrary_server_is_exclusive(self):
        self.assertEqual(check('s',SOURCE.replace('qemu-system-x86_64 -serial mon:telnet:127.0.0.1:7777,server,nowait','other-server')),[])


class OracleActorAttributionTests(unittest.TestCase):
    """case-18 regression: a session leak must be attributed to the actor that leaks it.

    The v32 cohort missed this defect by alleging that the VERIFIER left a telnet
    session open, when the reference (and the changed file) put the leak in the
    ORACLE's solve.sh. A finding anchored on the verifier does not identify an
    oracle-side leak, so the actor is part of the claim, not a detail.
    """

    ORACLE = SOURCE.split("cat <<'TWO'")[0]
    VERIFIER = ('import os\nscript = "spawn telnet localhost 7777"\n'
                'with open("run.exp", "w") as f:\n    f.write(script)\n'
                'os.popen("expect -f run.exp").read()\n')

    def test_oracle_leak_blocking_the_verifier_is_anchored_on_the_oracle(self):
        findings = check('solution/solve.sh', self.ORACLE, {'tests/test_outputs.py': self.VERIFIER})
        self.assertEqual(len(findings), 1)
        item = findings[0]
        # The finding must sit on the oracle, and the verifier may appear only as
        # the blocked party in supporting evidence.
        self.assertEqual(item['path'], 'solution/solve.sh')
        self.assertEqual(item['mechanism'], 'exclusive_console_client_ownership')
        self.assertTrue(any(s['path'] == 'tests/test_outputs.py' for s in item['supporting_evidence']))

    def test_a_verifier_only_leak_is_not_reported_against_the_oracle(self):
        # The oracle opens nothing; only the verifier spawns a session. There is no
        # oracle-side leak to allege, so nothing may be attributed to solve.sh.
        oracle_without_session = 'qemu-system-x86_64 -serial mon:telnet:127.0.0.1:7777,server,nowait\n'
        findings = check('solution/solve.sh', oracle_without_session, {'tests/test_outputs.py': self.VERIFIER})
        self.assertEqual([f['path'] for f in findings], [])

    def test_every_reported_leak_anchors_on_the_inspected_actor(self):
        for verifiers in ({}, {'tests/test_outputs.py': self.VERIFIER}):
            for item in check('solution/solve.sh', SOURCE, verifiers):
                self.assertEqual(item['path'], 'solution/solve.sh')
