from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from synth_local_runtime.config import RuntimeConfig


class RuntimeConfigTests(unittest.TestCase):
    def test_prod_is_default_and_toml_profiles_are_overridable(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            config_path = Path(temp) / "config.toml"
            config_path.write_text(
                '[intern]\nprofile = "staging"\n[intern.endpoints]\nstaging = "https://staging.example"\n',
                encoding="utf-8",
            )
            env = {
                "SYNTH_INTERN_CONFIG": str(config_path),
                "SYNTH_INTERN_DEMO": "0",
                "SYNTH_API_KEY": "test-key",
            }
            with patch.dict(os.environ, env, clear=False):
                config = RuntimeConfig.from_env(data_dir=Path(temp) / "data")
            self.assertEqual(config.intern_profile, "staging")
            self.assertEqual(config.backend_url, "https://staging.example")
            self.assertEqual(config.intern_mode, "remote")

    def test_backend_environment_override_wins(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            with patch.dict(
                os.environ,
                {
                    "SYNTH_BACKEND_URL": "http://127.0.0.1:9999",
                    "SYNTH_INTERN_DEMO": "0",
                    "SYNTH_INTERN_PROFILE": "prod",
                },
                clear=False,
            ):
                config = RuntimeConfig.from_env(data_dir=Path(temp) / "data")
            self.assertEqual(config.backend_url, "http://127.0.0.1:9999")
            self.assertEqual(config.intern_profile, "prod")


if __name__ == "__main__":
    unittest.main()
