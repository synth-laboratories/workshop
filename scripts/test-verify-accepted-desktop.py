"""Exercise accepted-byte publication checks using disposable distribution fixtures."""

import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "accepted_desktop", Path(__file__).with_name("verify-accepted-desktop.py")
)
accepted_desktop = importlib.util.module_from_spec(spec)
spec.loader.exec_module(accepted_desktop)


class AcceptedDesktopTests(unittest.TestCase):
    def test_identity_checks(self):
        version = json.loads(Path("apps/synth_desktop/package.json").read_text())["version"]
        base = f"Synth-Workshop-v{version}-stable-macOS-arm64-UNNOTARIZED"
        payload = b"release-byte-fixture"
        digest = hashlib.sha256(payload).hexdigest()
        source = "1" * 40
        manifest = {
            "schema": "workshop.distribution.v1", "version": version,
            "channel": "stable", "platform": "macOS", "architecture": "arm64",
            "sourceCommit": source, "sourceTreeDirty": False,
            "archive": f"{base}.zip", "archiveBytes": len(payload),
            "sha256": digest, "signature": "ad-hoc", "notarization": "none",
        }
        for fault in [None, "bytes", "source", "dirty", "sidecar", "extra", "size"]:
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                document = copy.deepcopy(manifest)
                if fault == "source":
                    document["sourceCommit"] = "2" * 40
                if fault == "dirty":
                    document["sourceTreeDirty"] = True
                if fault == "size":
                    document["archiveBytes"] += 1
                (root / f"{base}.json").write_text(json.dumps(document))
                (root / f"{base}.zip").write_bytes(b"wrong" if fault == "bytes" else payload)
                (root / f"{base}.zip.sha256").write_text(
                    f"{digest}  {'wrong.zip' if fault == 'sidecar' else base + '.zip'}\n"
                )
                if fault == "extra":
                    (root / "unexpected.zip").write_bytes(payload)
                if fault is None:
                    accepted_desktop.verify(root, source, digest)
                else:
                    with self.assertRaises(ValueError):
                        accepted_desktop.verify(root, source, digest)


if __name__ == "__main__":
    unittest.main()
