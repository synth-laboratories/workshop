from __future__ import annotations

import argparse

import uvicorn


def main() -> None:
    parser = argparse.ArgumentParser(description="Synth local inference service")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=7332)
    parser.add_argument("--reload", action="store_true")
    args = parser.parse_args()
    uvicorn.run("synth_inference.app:app", host=args.host, port=args.port, reload=args.reload)


if __name__ == "__main__":
    main()
