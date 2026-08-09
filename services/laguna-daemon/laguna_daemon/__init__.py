from __future__ import annotations

"""Native OpenResponses/OpenAI Responses Laguna loopback for Synth Desktop.

Backends:
  mock      — deterministic stream (CI / no weights)
  mlx_lm    — load Laguna/NVFP4 directly through the open MLX stack
  external  — legacy rollback support for a user-configured upstream

The native Responses engine never spawns or proxies an mlx_lm HTTP server and
never lowers its canonical representation through Chat objects.
"""

from .app import create_app
from .config import LagunaConfig

__all__ = ["LagunaConfig", "create_app"]
