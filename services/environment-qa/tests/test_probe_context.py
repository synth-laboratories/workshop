import json,tempfile,unittest
from pathlib import Path
from types import SimpleNamespace
from environment_qa.core import digest
from environment_qa.probe_context import prepare

class ProbeContextTests(unittest.TestCase):
    def test_original_upstream_path_maps_to_staged_projection(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            record={'document':'external/dependency-01.txt','url':'https://raw.githubusercontent.com/example/pkg/v1/pyproject.toml','projection':'complete','sha256':'digest'}
            files={'external/dependency-01.txt':'[build-system]', 'evidence/dependency-inventory.json':json.dumps({'records':[record]})}
            (root/'context.json').write_text(json.dumps(files))
            manifest=prepare(SimpleNamespace(root=root),{'path':'context.json','sha256':digest(files)},root/'transfer')
            self.assertEqual(manifest['source_index'],[record])

    def test_transfer_only_evidence_and_check_digest(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);files={'external/dependency-01.txt':'source','instruction.md':'do not overwrite','evidence/findings.json':'[]'}
            (root/'context.json').write_text(json.dumps(files));ref={'path':'context.json','sha256':digest(files)}
            manifest=prepare(SimpleNamespace(root=root),ref,root/'transfer')
            self.assertEqual((root/'transfer/external/dependency-01.txt').read_text(),'source')
            self.assertFalse((root/'transfer/instruction.md').exists())
            self.assertEqual(len(manifest['files']),2)
            with self.assertRaisesRegex(ValueError,'digest'):prepare(SimpleNamespace(root=root),dict(ref,sha256='wrong'),root/'other')
    def test_parent_escape_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);files={'external/../../escape':'bad'}
            (root/'context.json').write_text(json.dumps(files))
            with self.assertRaisesRegex(ValueError,'Unsafe'):prepare(SimpleNamespace(root=root),{'path':'context.json','sha256':digest(files)},root/'transfer')
