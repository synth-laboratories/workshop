from __future__ import annotations

import argparse
import os

import uvicorn

from .app import build_app
from .config import LagunaConfig


def main() -> None:
    parser = argparse.ArgumentParser(
        description=(
            "Synth Laguna sidecar — independent OpenAI-compatible MLX endpoint "
            "(Poolside ACP-compatible API shape)."
        )
    )
    parser.add_argument("--host", default=None, help="Bind host (default 127.0.0.1)")
    parser.add_argument("--port", type=int, default=None, help="Bind port (default 7333)")
    parser.add_argument(
        "--models-dir",
        default=None,
        help="Models directory (Poolside layout: <dir>/poolside/Laguna-XS-2.1-NVFP4-mlx)",
    )
    parser.add_argument(
        "--default-model",
        default=None,
        help="Default model id (default poolside/Laguna-XS-2.1-NVFP4-mlx)",
    )
    parser.add_argument(
        "--api-key",
        default=None,
        help="Bearer token required on all routes (stored under data dir if omitted)",
    )
    parser.add_argument(
        "--backend",
        choices=["auto", "mock", "mlx_lm", "external"],
        default=None,
    )
    parser.add_argument(
        "--external-url",
        default=None,
        help="When backend=external, upstream OpenAI-compatible base URL",
    )
    args = parser.parse_args()

    if args.host:
        os.environ["SYNTH_LAGUNA_HOST"] = args.host
    if args.port is not None:
        os.environ["SYNTH_LAGUNA_PORT"] = str(args.port)
    if args.models_dir:
        os.environ["SYNTH_LAGUNA_MODELS_DIR"] = args.models_dir
    if args.default_model:
        os.environ["SYNTH_LAGUNA_DEFAULT_MODEL"] = args.default_model
        os.environ["SYNTH_LAGUNA_MODEL"] = args.default_model
    if args.api_key:
        os.environ["SYNTH_LAGUNA_API_KEY"] = args.api_key
    if args.backend:
        os.environ["SYNTH_LAGUNA_BACKEND"] = args.backend
    if args.external_url:
        os.environ["SYNTH_LAGUNA_EXTERNAL_URL"] = args.external_url
        os.environ.setdefault("SYNTH_LAGUNA_BACKEND", "external")

    config = LagunaConfig.from_env()
    app = build_app(config)
    key_hint = "configured" if config.api_key else "(auth disabled)"
    print(
        f"[synth-laguna-sidecar] backend={config.backend} "
        f"listen={config.public_url} models_dir={config.models_dir} "
        f"default_model={config.default_model} api_key={key_hint}",
        flush=True,
    )
    print(
        f"[synth-laguna-sidecar] export SYNTH_LAGUNA_BASE_URL={config.public_url}",
        flush=True,
    )
    uvicorn.run(app, host=config.host, port=config.port, log_level="info")


if __name__ == "__main__":
    main()
