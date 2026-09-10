import tempfile
import unittest
from pathlib import Path
from environment_qa.admission import validate_compose


class AdmissionTests(unittest.TestCase):
    def check(self, text):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            (root/"docker-compose.yaml").write_text(text)
            return validate_compose(root)

    def test_private_sidecar_allowed(self):
        self.assertEqual(self.check('services:\n  db:\n    image: postgres:16\n    volumes: ["state:/data"]\nvolumes:\n  state: {}\n'),["docker-compose.yaml"])

    def test_host_bind_denied(self):
        with self.assertRaises(ValueError): self.check('services:\n  main:\n    volumes: ["/Users:/host"]\n')

    def test_privileged_denied(self):
        with self.assertRaises(ValueError): self.check('services:\n  main:\n    privileged: true\n')

    def test_include_not_resolved(self):
        with self.assertRaises(ValueError): self.check('include: /private/not-authorized.yml\nservices: {}\n')

    def test_external_volume_denied(self):
        with self.assertRaises(ValueError): self.check('services: {}\nvolumes:\n  user_data:\n    external: true\n')
