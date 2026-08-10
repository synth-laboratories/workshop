from __future__ import annotations

import ast
import importlib
import tempfile
import time
import unittest
from pathlib import Path

from fastapi.testclient import TestClient

from laguna_daemon.app import build_app
from laguna_daemon.config import LagunaConfig


PACKAGE_ROOT = Path(__file__).parents[1] / "laguna_daemon"

#: Anything that would let the daemon start, manage, or talk to a second local
#: model server. The architecture is one process owning one runtime; if any of
#: these reappears in the production package, that invariant is already broken.
FORBIDDEN_SUBSTRINGS = (
    "mlx_lm.server",
    "LagunaProcessManager",
    "SYNTH_LAGUNA_RESPONSES_ENGINE",
    "SYNTH_LAGUNA_UPSTREAM_HOST",
    "SYNTH_LAGUNA_UPSTREAM_PORT",
    "7334",
)

#: Modules that may never be imported anywhere in the serving package.
FORBIDDEN_IMPORTS = {"subprocess", "multiprocessing", "psutil"}


def _production_sources() -> list[tuple[Path, str]]:
    return [
        (path, path.read_text(encoding="utf-8"))
        for path in sorted(PACKAGE_ROOT.rglob("*.py"))
    ]


def _config(tmp: Path) -> LagunaConfig:
    models = tmp / "models"
    models.mkdir(parents=True, exist_ok=True)
    data = tmp / "data"
    data.mkdir(parents=True, exist_ok=True)
    return LagunaConfig(
        host="127.0.0.1",
        port=7333,
        backend="mock",
        api_key=None,
        models_dir=models,
        default_model="poolside/Laguna-XS-2.1-NVFP4-mlx",
        model="poolside/Laguna-XS-2.1-NVFP4-mlx",
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


class NoSecondRuntimeTests(unittest.TestCase):
    """The one-runtime architecture, enforced rather than documented.

    Note for anyone reading a process list while debugging: the legacy managed
    child was ours. Poolside's own sidecar runs on `:63300` from
    /Applications/Poolside.app and is unrelated to this daemon.

    A GGUF selection (Muse Glimmer) does involve a second process, and that is
    still not a second *runtime* by this invariant: the daemon does not start
    it, does not restart it, does not discover it, and does not know its port —
    the Desktop supervisor owns its lifecycle and passes one address in through
    the environment. Every rule below therefore still holds verbatim for the
    llama.cpp backend, including the ban on the legacy port literal, and
    `tests/test_muse_llama_cpp.py` asserts the same properties from its side.
    """

    def test_production_sources_cannot_reference_a_second_local_server(self) -> None:
        for path, source in _production_sources():
            for needle in FORBIDDEN_SUBSTRINGS:
                self.assertNotIn(
                    needle,
                    source,
                    msg=f"{path.relative_to(PACKAGE_ROOT)} references {needle!r}",
                )

    def test_production_sources_cannot_import_process_spawning_modules(self) -> None:
        for path, source in _production_sources():
            tree = ast.parse(source, filename=str(path))
            for node in ast.walk(tree):
                if isinstance(node, ast.Import):
                    names = {alias.name.split(".")[0] for alias in node.names}
                elif isinstance(node, ast.ImportFrom):
                    names = {(node.module or "").split(".")[0]}
                else:
                    continue
                offenders = names & FORBIDDEN_IMPORTS
                self.assertFalse(
                    offenders,
                    msg=(
                        f"{path.relative_to(PACKAGE_ROOT)} imports {offenders}; "
                        "the daemon must not spawn a process"
                    ),
                )

    def test_legacy_modules_are_gone(self) -> None:
        for module in ("laguna_daemon.manager", "laguna_daemon.responses"):
            with self.assertRaises(
                ModuleNotFoundError, msg=f"{module} should have been deleted"
            ):
                importlib.import_module(module)

    def test_config_exposes_no_local_upstream(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synth-laguna-guard-") as tmp:
            config = _config(Path(tmp))
            for field in ("upstream_host", "upstream_port", "responses_engine"):
                self.assertFalse(
                    hasattr(config, field), msg=f"config still carries {field}"
                )
            # A local backend has no upstream at all; asking for one is a bug,
            # not a silently-defaulted loopback address.
            with self.assertRaises(RuntimeError):
                _ = config.upstream_url

    def test_process_manager_routes_are_gone(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synth-laguna-routes-") as tmp:
            client = TestClient(build_app(_config(Path(tmp))))
            # /v1/synth/status is now the typed control surface for the
            # in-process runtime. The guarded invariant is unchanged: no
            # process-manager vocabulary — nothing that names a child
            # process, a second server, or a restart — may resurface.
            response = client.get("/v1/synth/status")
            self.assertEqual(response.status_code, 200)
            body = response.json()
            for legacy_field in ("pid", "process", "processState", "upstream", "restarts"):
                self.assertNotIn(legacy_field, body)
            self.assertEqual(body["backend"], "mock")

    def test_both_surfaces_serve_with_no_second_runtime_available(self) -> None:
        """Chat and Responses must both work with process spawning unavailable."""
        with tempfile.TemporaryDirectory(prefix="synth-laguna-both-") as tmp:
            client = TestClient(build_app(_config(Path(tmp))))
            chat = client.post(
                "/v1/chat/completions",
                json={
                    "model": "poolside/Laguna-XS-2.1-NVFP4-mlx",
                    "messages": [{"role": "user", "content": "hi"}],
                },
            )
            self.assertEqual(chat.status_code, 200)
            self.assertEqual(chat.json()["object"], "chat.completion")

            responses = client.post(
                "/v1/responses",
                json={
                    "model": "poolside/Laguna-XS-2.1-NVFP4-mlx",
                    "input": "hi",
                    "store": False,
                },
            )
            self.assertEqual(responses.status_code, 200)
            self.assertEqual(responses.json()["object"], "response")


if __name__ == "__main__":
    unittest.main()
