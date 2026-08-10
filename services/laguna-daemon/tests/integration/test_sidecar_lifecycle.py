"""Live-daemon checks for the /v1/synth control surface.

Skipped unless SYNTH_LAGUNA_LIVE_BASE_URL is set, like test_live_mlx. These
are strictly read-only: no generation requests, no unload, no download —
a benchmark may be running against the daemon under test.

The real HuggingFaceDownloader is exercised separately, gated on
SYNTH_LAGUNA_LIVE_DOWNLOAD_REPO (point it at a tiny public repo), and never
touches the daemon at all.
"""

from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path

from .test_live_mlx import LiveDaemonTestCase, requires_live_daemon


CANONICAL_STATES = {
    "starting",
    "checking_memory",
    "downloading",
    "downloaded",
    "loading",
    "resident_idle",
    "queued",
    "prefill",
    "reasoning",
    "decoding",
    "unloading",
    "unloaded",
    "blocked_memory",
    "error",
}


@requires_live_daemon
class LiveSidecarControlTests(LiveDaemonTestCase):
    """Read-only schema and consistency checks against a real daemon."""

    def control_status(self) -> dict:
        response = self.client.get("/v1/synth/status")
        self.assertEqual(response.status_code, 200, response.text)
        return response.json()

    def test_status_schema_sanity(self) -> None:
        status = self.control_status()
        self.assertEqual(status["schema_version"], "1.0")
        self.assertIn(status["state"], CANONICAL_STATES)
        self.assertEqual(status["model"]["id"], self.model)
        self.assertIsInstance(status["model"]["available"], bool)
        self.assertIsInstance(status["model"]["resident"], bool)
        memory = status["memory"]
        self.assertIn(memory["admission"], {"allowed", "blocked"})
        self.assertGreater(memory["required_bytes"], 0)
        # Measured or null — a null must never have been rendered as 0.
        if memory["free_bytes"] is not None:
            self.assertGreater(memory["free_bytes"], 0)
        generation = status["generation"]
        self.assertGreaterEqual(generation["in_flight"], 0)
        self.assertGreaterEqual(generation["queued"], 0)
        self.assertEqual(status["reasoning"]["legacy_aliases"], {"max": "high"})

    def test_status_agrees_with_the_inference_snapshot(self) -> None:
        # Both reads race a live benchmark, so only compare facts that cannot
        # legitimately flip between two adjacent requests when idle-or-busy
        # state holds: identity and residency-vs-state coherence within one
        # response body.
        status = self.control_status()
        snapshot = self.inference()
        self.assertEqual(snapshot["model"], status["model"]["id"])
        if status["state"] in {"resident_idle", "queued", "prefill", "decoding"}:
            self.assertTrue(status["model"]["resident"])
        if status["state"] in {"unloaded", "downloaded", "blocked_memory"}:
            self.assertFalse(status["model"]["resident"])
        if status["generation"]["in_flight"] > 0:
            self.assertIsNotNone(status["generation"]["active_request_id"])
            self.assertTrue(
                status["generation"]["active_request_id"].startswith("sha256:")
            )

    def test_capabilities_and_models_inventory(self) -> None:
        capabilities = self.client.get("/v1/synth/capabilities")
        self.assertEqual(capabilities.status_code, 200)
        self.assertEqual(capabilities.json()["model"], self.model)

        models = self.client.get("/v1/synth/models")
        self.assertEqual(models.status_code, 200)
        entry = next(
            item for item in models.json()["data"] if item["id"] == self.model
        )
        self.assertTrue(entry["default"])

    def test_metrics_mirror_matches_the_rolling_snapshot_shape(self) -> None:
        metrics = self.client.get("/v1/synth/metrics")
        self.assertEqual(metrics.status_code, 200)
        body = metrics.json()
        self.assertEqual(body["schema_version"], "1.0")
        self.assertEqual(
            set(body["rolling"]), set(self.inference()["rolling"])
        )

    def test_settings_are_served_read_only_here(self) -> None:
        response = self.client.get("/v1/synth/settings")
        self.assertEqual(response.status_code, 200)
        body = response.json()
        self.assertEqual(body["schema_version"], "1.0")
        self.assertIn("default_temperature", body["settings"])

    def test_openapi_document_is_served(self) -> None:
        response = self.client.get("/v1/synth/openapi.json")
        self.assertEqual(response.status_code, 200)
        document = response.json()
        self.assertEqual(document["openapi"], "3.1.0")
        self.assertIn("/v1/synth/status", document["paths"])


@unittest.skipUnless(
    os.getenv("SYNTH_LAGUNA_LIVE_DOWNLOAD_REPO"),
    "set SYNTH_LAGUNA_LIVE_DOWNLOAD_REPO to a tiny public repo to run the "
    "real downloader",
)
class LiveDownloaderTests(unittest.TestCase):
    """The real huggingface_hub downloader, against a throwaway directory."""

    def test_snapshot_download_into_a_tempdir(self) -> None:
        from laguna_daemon.synth_control import HuggingFaceDownloader

        repo = os.environ["SYNTH_LAGUNA_LIVE_DOWNLOAD_REPO"]
        with tempfile.TemporaryDirectory(prefix="synth-live-dl-") as temp:
            destination = Path(temp) / repo
            HuggingFaceDownloader().download(repo, destination, lambda *_: None)
            files = [path for path in destination.rglob("*") if path.is_file()]
            self.assertTrue(files, "download produced no files")


if __name__ == "__main__":
    unittest.main()
