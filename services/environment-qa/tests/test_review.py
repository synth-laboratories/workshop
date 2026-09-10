import io
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch, MagicMock
from environment_qa.review import ai_request, call_ai
from environment_qa.core import CHARTERS


class ReviewTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name)
        (self.path / "instruction.md").write_text("Return the required result.")
        self.policy = {"charter": CHARTERS["terminal-bench"], "task_goals": ""}
        self.env = {"QA_PROVIDER_URL": "http://127.0.0.1:9999/v1/chat/completions", "QA_PROVIDER_MODEL": "test-model",
                    "QA_PROVIDER_KEY": "test-only", "QA_INPUT_USD_PER_MILLION": "1", "QA_OUTPUT_USD_PER_MILLION": "2"}

    def tearDown(self): self.temp.cleanup()

    def response(self, quote="Return the required result."):
        return {"choices": [{"message": {"content": json.dumps({"findings": [{"category": "instruction_verifier_alignment", "severity": "warning",
            "title": "Ambiguous result", "path": "instruction.md", "line": 1, "evidence": quote, "mechanism": "underspecified_result"}], "limitations": ["No runtime probe"]})}}],
            "usage": {"prompt_tokens": 100, "completion_tokens": 50}}

    def call(self, data):
        opener = MagicMock()
        opener.open.return_value.__enter__.return_value = io.BytesIO(json.dumps(data).encode())
        with patch.dict(os.environ, self.env), patch("urllib.request.build_opener", return_value=opener):
            request = ai_request(self.path, self.policy)
            self.assertGreater(request[3], 0)
            return call_ai(request)

    def test_structured_response_and_usage(self):
        result = self.call(self.response())
        self.assertEqual(result["findings"][0]["path"], "instruction.md")
        self.assertAlmostEqual(result["provider"]["actual_usd"], 0.0002)

    def test_fabricated_evidence_is_rejected(self):
        result = self.call(self.response("invented quote"))
        self.assertEqual(result["findings"], [])
        self.assertEqual(len(result["rejected_findings"]), 1)
        self.assertAlmostEqual(result["provider"]["actual_usd"], 0.0002)

    def test_no_credential_discovery(self):
        with patch.dict(os.environ, {}, clear=True), self.assertRaises(ValueError):
            ai_request(self.path, self.policy)

    def test_claude_is_prohibited(self):
        with patch.dict(os.environ, self.env | {"QA_PROVIDER_MODEL": "anthropic/claude-sonnet-4.6"}), self.assertRaisesRegex(ValueError, "prohibited"):
            ai_request(self.path, self.policy)

    def test_missing_usage_is_unknown(self):
        response = self.response(); del response["usage"]
        self.assertIsNone(self.call(response)["provider"]["actual_usd"])


if __name__ == "__main__": unittest.main()
