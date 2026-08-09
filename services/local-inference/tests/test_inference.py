from __future__ import annotations

import os

from fastapi.testclient import TestClient

os.environ["SYNTH_INFERENCE_MODE"] = "mock"

from synth_inference.app import create_app  # noqa: E402
from synth_inference.state import InferenceState  # noqa: E402


def test_mock_model_status_and_completion() -> None:
    state = InferenceState()
    app = create_app(state)
    with TestClient(app) as client:
        status = client.get("/v1/models/status")
        assert status.status_code == 200
        assert status.json()["active_mode"] == "mock"
        assert status.json()["state"] == "ready"

        response = client.post(
            "/v1/chat/completions",
            json={
                "model": "laguna-xs-2.1",
                "messages": [{"role": "user", "content": "Inspect this dataset"}],
                "stream": False,
            },
        )
        assert response.status_code == 200
        assert "local Laguna" in response.json()["choices"][0]["message"]["content"]


def test_mock_stream_is_openai_sse() -> None:
    state = InferenceState()
    app = create_app(state)
    with TestClient(app) as client:
        with client.stream(
            "POST",
            "/v1/chat/completions",
            json={
                "messages": [{"role": "user", "content": "hello"}],
                "stream": True,
            },
        ) as response:
            body = "".join(response.iter_text())
        assert response.status_code == 200
        assert "chat.completion.chunk" in body
        assert "data: [DONE]" in body


def test_request_adapter_must_match_loaded_adapter() -> None:
    state = InferenceState()
    app = create_app(state)
    with TestClient(app) as client:
        response = client.post(
            "/v1/chat/completions",
            json={
                "messages": [{"role": "user", "content": "hello"}],
                "adapter": "/tmp/not-loaded",
                "stream": False,
            },
        )
        assert response.status_code == 409
