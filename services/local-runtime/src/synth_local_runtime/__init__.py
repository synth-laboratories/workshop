"""Synth Desktop local runtime daemon."""

from .config import RuntimeConfig
from .service import RuntimeService

__all__ = ["RuntimeConfig", "RuntimeService"]
__version__ = "0.1.0"
