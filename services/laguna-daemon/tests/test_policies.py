from __future__ import annotations

import json
import tempfile
import time
import unittest
from pathlib import Path

from fastapi.testclient import TestClient

from laguna_daemon.app import build_app
from laguna_daemon.config import LagunaConfig
from laguna_daemon.responses_api.policies import PolicyError, PolicyRegistry

BASE = "poolside/Laguna-XS-2.1-NVFP4-mlx"
FT = "synth/Laguna-XS-2.1-ft"


def _config(tmp: Path) -> LagunaConfig:
    models = tmp / "models"
    models.mkdir(parents=True, exist_ok=True)
    data = tmp / "data"
    data.mkdir(parents=True, exist_ok=True)
    return LagunaConfig(
        host="127.0.0.1",
        port=7333,
        backend="mock",
        api_key="synth-test-key",
        models_dir=models,
        default_model=BASE,
        model=BASE,
        revision=None,
        draft_model=None,
        adapter=None,
        external_url=None,
        upstream_api_key=None,
        data_dir=data,
        auto_load=True,
        idle_unload_after_seconds=900,
        context_length=262144,
        started_at=time.time(),
    )


def _adapter(root: Path, name: str = "ft") -> Path:
    adapter = root / name
    adapter.mkdir(parents=True, exist_ok=True)
    (adapter / "adapter_config.json").write_text(
        json.dumps({"lora_parameters": {"rank": 8, "scale": 20.0, "keys": ["layer"]}}),
        encoding="utf-8",
    )
    (adapter / "adapters.safetensors").write_bytes(b"weights")
    return adapter


class PolicyRegistryTests(unittest.TestCase):
    def test_base_policy_is_always_present_and_protected(self) -> None:
        with tempfile.TemporaryDirectory(prefix="laguna-policy-") as tmp:
            root = Path(tmp)
            registry = PolicyRegistry(root, BASE)
            self.assertEqual([policy.model_id for policy in registry.list()], [BASE])
            self.assertTrue(registry.resolve(None).is_base)
            with self.assertRaises(PolicyError):
                registry.remove(BASE)
            with self.assertRaises(PolicyError):
                registry.register(BASE, _adapter(root))

    def test_unknown_model_is_refused_rather_than_served_by_the_base(self) -> None:
        with tempfile.TemporaryDirectory(prefix="laguna-policy-") as tmp:
            registry = PolicyRegistry(Path(tmp), BASE)
            with self.assertRaises(PolicyError) as caught:
                registry.resolve("synth/not-registered")
            self.assertEqual(caught.exception.field, "model")

    def test_registration_requires_an_mlx_lora_directory(self) -> None:
        with tempfile.TemporaryDirectory(prefix="laguna-policy-") as tmp:
            root = Path(tmp)
            registry = PolicyRegistry(root, BASE)
            with self.assertRaises(PolicyError):
                registry.register(FT, root / "missing")
            empty = root / "empty"
            empty.mkdir()
            with self.assertRaises(PolicyError):
                registry.register(FT, empty)

    def test_policies_survive_a_restart(self) -> None:
        with tempfile.TemporaryDirectory(prefix="laguna-policy-") as tmp:
            root = Path(tmp)
            adapter = _adapter(root)
            PolicyRegistry(root, BASE).register(FT, adapter, digest="sha256:abc")
            reopened = PolicyRegistry(root, BASE)
            self.assertEqual(
                [policy.model_id for policy in reopened.list()], [BASE, FT]
            )
            self.assertEqual(reopened.resolve(FT).digest, "sha256:abc")

    def test_a_policy_whose_bytes_vanished_is_dropped(self) -> None:
        """A picker entry that fails at the first turn is worse than absent."""
        with tempfile.TemporaryDirectory(prefix="laguna-policy-") as tmp:
            root = Path(tmp)
            adapter = _adapter(root)
            PolicyRegistry(root, BASE).register(FT, adapter)
            for child in adapter.iterdir():
                child.unlink()
            adapter.rmdir()
            self.assertEqual(
                [policy.model_id for policy in PolicyRegistry(root, BASE).list()], [BASE]
            )


class PolicyHttpTests(unittest.TestCase):
    def test_models_lists_every_policy_and_turns_pin_by_model_id(self) -> None:
        with tempfile.TemporaryDirectory(prefix="laguna-policy-") as tmp:
            root = Path(tmp)
            config = _config(root)
            app = build_app(config)
            headers = {"Authorization": "Bearer synth-test-key"}
            with TestClient(app) as client:
                listed = client.get("/v1/models", headers=headers).json()
                self.assertEqual([item["id"] for item in listed["data"]], [BASE])

                registered = client.post(
                    "/v1/synth/policies",
                    headers=headers,
                    json={"model_id": FT, "adapter_path": str(_adapter(root))},
                )
                self.assertEqual(registered.status_code, 200)
                self.assertFalse(registered.json()["policy"]["is_base"])

                listed = client.get("/v1/models", headers=headers).json()
                self.assertEqual([item["id"] for item in listed["data"]], [BASE, FT])
                # Codex reads its own envelope; both must describe the same set.
                self.assertEqual([item["slug"] for item in listed["models"]], [BASE, FT])

                service = app.state.responses_service
                answered = client.post(
                    "/v1/responses",
                    headers=headers,
                    json={"model": FT, "input": "hello", "stream": False},
                )
                self.assertEqual(answered.status_code, 200)
                self.assertEqual(service.backend.attached_policy, FT)

                answered = client.post(
                    "/v1/responses",
                    headers=headers,
                    json={"model": BASE, "input": "hello", "stream": False},
                )
                self.assertEqual(answered.status_code, 200)
                self.assertEqual(service.backend.attached_policy, BASE)

    def test_an_unregistered_model_is_a_404_not_a_silent_base_turn(self) -> None:
        with tempfile.TemporaryDirectory(prefix="laguna-policy-") as tmp:
            app = build_app(_config(Path(tmp)))
            headers = {"Authorization": "Bearer synth-test-key"}
            with TestClient(app) as client:
                answered = client.post(
                    "/v1/responses",
                    headers=headers,
                    json={"model": "synth/never-registered", "input": "hi", "stream": False},
                )
                self.assertEqual(answered.status_code, 404)
                self.assertEqual(answered.json()["error"]["code"], "model_not_found")

    def test_removing_a_policy(self) -> None:
        with tempfile.TemporaryDirectory(prefix="laguna-policy-") as tmp:
            root = Path(tmp)
            app = build_app(_config(root))
            headers = {"Authorization": "Bearer synth-test-key"}
            with TestClient(app) as client:
                client.post(
                    "/v1/synth/policies",
                    headers=headers,
                    json={"model_id": FT, "adapter_path": str(_adapter(root))},
                )
                self.assertEqual(
                    client.delete(f"/v1/synth/policies/{FT}", headers=headers).status_code, 200
                )
                self.assertEqual(
                    client.delete(f"/v1/synth/policies/{FT}", headers=headers).status_code, 404
                )
                self.assertEqual(
                    client.delete(f"/v1/synth/policies/{BASE}", headers=headers).status_code, 400
                )


if __name__ == "__main__":
    unittest.main()


class PolicyTelemetryTests(unittest.TestCase):
    """Per-policy decode speed, and the rules about when not to show it."""

    @staticmethod
    def _record(telemetry, policy: str, latencies: list[float]) -> None:
        from laguna_daemon.responses_api.telemetry import GenerationTiming

        timing = GenerationTiming(generation_id="gen", queued_at=0.0, policy=policy)
        timing.decode_latencies = latencies
        telemetry.record_completed(timing, 1000.0)

    def _telemetry(self):
        from laguna_daemon.responses_api.telemetry import InferenceTelemetry

        return InferenceTelemetry()

    def test_a_thin_sample_reports_nothing_rather_than_a_confident_number(self) -> None:
        telemetry = self._telemetry()
        self._record(telemetry, BASE, [0.02] * 10)
        row = telemetry.policy_snapshot(BASE)["policies"][BASE]
        self.assertIsNone(row["tokensPerSecondP10"])
        self.assertEqual(row["tokenSamples"], 10)

    def test_decode_speed_is_reported_per_policy(self) -> None:
        telemetry = self._telemetry()
        self._record(telemetry, BASE, [0.02] * 500)
        self._record(telemetry, FT, [0.025] * 500)
        snapshot = telemetry.policy_snapshot(BASE)
        self.assertEqual(snapshot["policies"][BASE]["tokensPerSecondP10"], 50.0)
        self.assertEqual(snapshot["policies"][FT]["tokensPerSecondP10"], 40.0)
        self.assertIsNone(snapshot["policies"][BASE]["deltaVsBasePct"])
        self.assertEqual(snapshot["policies"][FT]["deltaVsBasePct"], -20.0)
        self.assertTrue(snapshot["policies"][FT]["deltaIsResolvable"])

    def test_a_delta_under_the_noise_floor_is_not_resolvable(self) -> None:
        telemetry = self._telemetry()
        # One half of the base samples is much slower than the other, so this
        # policy disagrees with itself by more than the two policies differ.
        self._record(telemetry, BASE, [0.02] * 300 + [0.04] * 300)
        self._record(telemetry, FT, [0.0201] * 300 + [0.0402] * 300)
        row = telemetry.policy_snapshot(BASE)["policies"][FT]
        self.assertIsNotNone(row["deltaVsBasePct"])
        self.assertGreater(row["measurementFloorPct"], abs(row["deltaVsBasePct"]))
        self.assertFalse(row["deltaIsResolvable"])

    def test_metrics_omit_a_policy_that_was_never_measured(self) -> None:
        from laguna_daemon.app import _policy_metric_lines

        telemetry = self._telemetry()
        self._record(telemetry, BASE, [0.02] * 500)
        self._record(telemetry, FT, [0.02] * 5)
        lines = _policy_metric_lines(telemetry.policy_snapshot(BASE))
        rendered = "\n".join(lines)
        self.assertIn(f'policy="{BASE}"', rendered)
        # A zero here would read as "infinitely slow" on a dashboard.
        self.assertNotIn(FT, rendered)
