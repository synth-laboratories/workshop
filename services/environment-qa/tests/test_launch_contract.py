"""The shared client and the service must agree on how a run is launched.

Both interfaces drive the same HTML client, so a field renamed on one side and not
the other produces a launch that silently degrades rather than an error anyone sees.
These assertions are deliberately about the contract's field names, not behaviour:
they are what drifts.
"""
import re
import unittest
from pathlib import Path

from environment_qa.profiles import advertise

ROOT = Path(__file__).resolve().parent.parent
APP = (ROOT / "web" / "app.js").read_text()
HTML = (ROOT / "web" / "index.html").read_text()
SERVER = (ROOT / "environment_qa" / "server.py").read_text()


class LaunchContractTests(unittest.TestCase):
    def test_the_form_submits_a_profile_not_a_pipeline(self):
        self.assertIn('name="profile_id"', HTML)
        self.assertNotIn('name="pipeline"', HTML)

    def test_the_server_admits_the_field_the_form_sends(self):
        self.assertIn('body.get("profile_id")', SERVER)

    def test_legacy_is_not_submitted_as_a_profile(self):
        # "legacy" is the rules-only baseline; sending it as a profile id would be
        # refused as unknown, so the client must drop the field instead.
        self.assertIn('data.profile_id === "legacy"', APP)
        self.assertIn("delete data.profile_id", APP)

    def test_an_unknown_pipeline_is_refused_rather_than_defaulted(self):
        self.assertIn("Unknown pipeline", SERVER)
        self.assertNotIn('full = body.get("pipeline") in {"full", "targeted"}', SERVER)

    def test_the_client_reads_every_field_the_config_publishes(self):
        for field in ("ai_runtime", "ai_runtime_disabled_reason", "profiles", "provider_calls_enabled"):
            self.assertIn(field, APP, f"the client ignores config.{field}")
            self.assertIn(f'"{field}"', SERVER, f"the service does not publish {field}")

    def test_the_client_uses_only_advertised_profile_fields(self):
        published = set(advertise()[0])
        used = set(re.findall(r"profile\.(\w+)", APP))
        self.assertTrue(used, "the client never reads a profile entry")
        self.assertLessEqual(used, published, f"client reads fields the service does not publish: {used - published}")

    def test_mode_follows_the_profile_rather_than_the_operator(self):
        # A profile pins its mode; the server refuses a contradicting one, so the
        # form must not present mode as an independent choice alongside a profile.
        self.assertIn("dataset.mode", APP)
        self.assertIn("elements.mode.disabled", APP)

    def test_the_runtime_gate_is_enforced_by_the_service_too(self):
        # A client cannot be trusted to hide a disabled option.
        self.assertIn("No AI runtime is configured", SERVER)


if __name__ == "__main__":
    unittest.main()
