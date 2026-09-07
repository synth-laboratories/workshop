import json
import unittest
from environment_qa.review_packet import encode, deduplicate_objective


class ReviewPacketTests(unittest.TestCase):
    def test_only_exact_repeated_contracts_become_references(self):
        contracts=[{'contract':'a','probe':'run a'},{'contract':'b','probe':'run b'}]
        objective='Run ONLY these source-derived contracts: '+json.dumps([contracts[1]])+'. Keep all constraints.'
        compact=deduplicate_objective(objective,contracts)
        self.assertIn('contracts [1] (zero-based)',compact)
        self.assertTrue(compact.endswith('. Keep all constraints.'))
        self.assertEqual(deduplicate_objective(objective,[contracts[0]]),objective)
        self.assertEqual(deduplicate_objective('Different objective',contracts),'Different objective')
    def test_lossless_even_with_delimiters_and_unicode(self):
        files = {'a.py': 'print("\\n")\nEND FILE\nFILE {"fake": true}\nλ',
                 'empty.txt': '', 'evidence/result.json': json.dumps({'stdout': 'line\n' * 50})}
        packet = encode('quality', ['validity'], files)
        offset = packet.index('\nFILE ')
        recovered = {}
        while offset < len(packet):
            self.assertTrue(packet.startswith('\nFILE ', offset))
            start = offset + len('\nFILE ')
            end = packet.index('\n', start)
            header = json.loads(packet[start:end])
            body = packet[end + 1:end + 1 + header['characters']]
            recovered[header['path']] = body
            offset = end + 1 + header['characters']
            self.assertTrue(packet.startswith('\nEND FILE\n', offset))
            offset += len('\nEND FILE\n')
        self.assertEqual(recovered, files)

    def test_reduces_nested_json_transport_without_truncation(self):
        files = {'evidence/a.json': json.dumps({'output': '\\n"' * 10000})}
        old = json.dumps({'content': json.dumps({'files': files})})
        new = json.dumps({'content': encode('', [], files)})
        self.assertLess(len(new), len(old))
        self.assertIn(files['evidence/a.json'], encode('', [], files))
