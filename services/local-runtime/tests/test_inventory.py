from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from synth_local_runtime.config import RuntimeConfig
from synth_local_runtime.service import RuntimeService


class InventoryServiceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp(prefix="synth-inv-"))
        workshop = Path(__file__).resolve().parents[3]
        self.config = RuntimeConfig.from_env(
            host="127.0.0.1",
            port=0,
            data_dir=self.tmp,
        )
        object.__setattr__(self.config, "visuals_root", workshop / "visuals")
        object.__setattr__(self.config, "workshop_root", workshop)
        object.__setattr__(self.config, "openrouter_api_key", None)
        self.service = RuntimeService(self.config)

    def test_seeded_containers_and_traces(self) -> None:
        containers = self.service.list_containers()
        traces = self.service.list_traces()
        self.assertGreaterEqual(len(containers), 2)
        self.assertGreaterEqual(len(traces), 1)
        locations = {c["location"] for c in containers}
        self.assertIn("local", locations)
        self.assertIn("cloud", locations)

    def test_visual_templates_catalog(self) -> None:
        templates = self.service.list_visual_templates()
        self.assertGreaterEqual(len(templates), 9)
        ids = {t["id"] for t in templates}
        self.assertIn("posttrain.rollout_viewer.v1", ids)
        self.assertIn("craftax.eval_matrix.v1", ids)

    def test_create_visual_and_save_tsx(self) -> None:
        visual = self.service.create_visual(
            {
                "templateId": "posttrain.rollout_viewer.v1",
                "title": "Test rollout",
                "bindings": {"kind": "fixture"},
            }
        )
        saved = self.service.save_visual_tsx(visual["id"])
        self.assertTrue(Path(saved["tsxPath"]).exists())
        text = Path(saved["tsxPath"]).read_text(encoding="utf-8")
        self.assertIn(visual["id"], text)

    def test_remote_target_requires_key(self) -> None:
        session = self.service.create_session(
            {
                "kind": "remote",
                "provider": "openrouter",
                "model": "moonshotai/kimi-k2.5",
                "adapter": None,
            }
        )
        self.assertEqual(session["status"], "configuration_required")

    def test_local_target_requires_responses_server(self) -> None:
        object.__setattr__(self.config, "laguna_base_url", None)
        session = self.service.create_session(
            {"kind": "local", "model": "laguna-xs-2.1", "adapter": None}
        )
        self.assertEqual(session["status"], "configuration_required")
        with self.assertRaisesRegex(RuntimeError, "not configured"):
            self.service.send_message(session["id"], "hello inventory")


if __name__ == "__main__":
    unittest.main()
