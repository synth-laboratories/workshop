from __future__ import annotations

"""OpenAI-compatible Laguna loopback for Synth Desktop.

Backends:
  mock      — deterministic stream (CI / no weights)
  mlx_lm    — spawn `python -m mlx_lm.server` (vanilla MLX path)
  external  — reverse-proxy an already-running optimized server
              (mere.run / mlxfast fork / dflash-serve)
"""

from .app import create_app
from .config import LagunaConfig

__all__ = ["LagunaConfig", "create_app"]
