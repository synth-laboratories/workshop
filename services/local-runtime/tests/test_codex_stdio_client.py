from __future__ import annotations

import inspect
import unittest

from synth_local_runtime.codex.stdio_client import (
    CodexAppServerClient,
    DEFAULT_REQUEST_TIMEOUT_SECONDS,
)


class CodexAppServerClientTests(unittest.TestCase):
    def test_default_request_timeout_covers_long_optimizer_proposals(self):
        timeout = inspect.signature(CodexAppServerClient.request).parameters["timeout"]
        self.assertEqual(timeout.default, DEFAULT_REQUEST_TIMEOUT_SECONDS)
        self.assertGreaterEqual(DEFAULT_REQUEST_TIMEOUT_SECONDS, 300.0)


if __name__ == "__main__":
    unittest.main()
