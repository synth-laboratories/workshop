from __future__ import annotations

"""User-modifiable runtime settings for the Laguna daemon.

The file lives beside the daemon's generated api_key file:
``<data_dir>/settings.toml`` (``~/.synth-desktop/laguna/settings.toml`` by
default; tests point ``data_dir`` elsewhere). A missing file means all
defaults. An unknown key is a startup error that names the key — a typo'd
knob that silently leaves the real default in place is worse than a crash.

Startup-only facts (models dir, default model, port, backend, api key) are
deliberately not settable here; they remain CLI/env configuration and are
reported read-only by GET /v1/synth/settings.

Precedence for ``idle_unload_after_seconds``: a value in settings.toml (or
set via PUT /v1/synth/settings) wins over the legacy
``SYNTH_LAGUNA_IDLE_UNLOAD_SECONDS`` environment variable. The env var is kept
only as a fallback for daemons whose settings file does not set the key.

For the sampling keys the precedence is: explicit request value > settings >
built-in (temperature 1.0, top_p 1.0, top_k 0, reasoning high, max output
8192). Settings are consulted only when a request omits the field.
"""

import json
import os
import tempfile
import tomllib
from pathlib import Path
from typing import Any

from .config import LagunaConfig
from .responses_api.capabilities import HARD_MAX_OUTPUT_TOKENS, SamplingDefaults


SETTINGS_SCHEMA_VERSION = "1.0"
SETTINGS_FILENAME = "settings.toml"

# Historical backend constants, restated as defaults so the settings surface
# reports them even when nothing was configured.
DEFAULT_PROMPT_CACHE_SLOTS = 2
DEFAULT_QUEUE_CAPACITY = 9


class SettingsError(RuntimeError):
    """A rejected settings key or value; `field` names the offender."""

    def __init__(self, message: str, *, field: str | None = None) -> None:
        super().__init__(message)
        self.field = field


def _require_int(field: str, value: Any, low: int, high: int | None) -> int:
    if not isinstance(value, int) or isinstance(value, bool):
        raise SettingsError(f"{field} must be an integer.", field=field)
    if value < low or (high is not None and value > high):
        bound = f"{low}-{high}" if high is not None else f">= {low}"
        raise SettingsError(f"{field} must be in range {bound}.", field=field)
    return value


def _require_float(
    field: str, value: Any, low: float, high: float, *, exclusive_low: bool = False
) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise SettingsError(f"{field} must be a number.", field=field)
    number = float(value)
    below = number <= low if exclusive_low else number < low
    if below or number > high:
        left = "(" if exclusive_low else "["
        raise SettingsError(
            f"{field} must be in range {left}{low}, {high}].", field=field
        )
    return number


def _validate(field: str, value: Any) -> Any:
    if field == "default_temperature":
        return _require_float(field, value, 0.0, 2.0)
    if field == "default_top_p":
        return _require_float(field, value, 0.0, 1.0, exclusive_low=True)
    if field == "default_top_k":
        return _require_int(field, value, 0, 8192)
    if field == "default_reasoning_effort":
        if value not in {"none", "high"}:
            raise SettingsError(
                f'{field} must be "none" or "high".', field=field
            )
        return value
    if field == "default_max_output_tokens":
        return _require_int(field, value, 1, HARD_MAX_OUTPUT_TOKENS)
    if field == "idle_unload_after_seconds":
        # 0 means never unload.
        return _require_int(field, value, 0, None)
    if field == "prompt_cache_slots":
        return _require_int(field, value, 1, 32)
    if field == "queue_capacity":
        return _require_int(field, value, 1, 32)
    raise SettingsError(f"unknown settings key {field!r}.", field=field)


def _utc_iso() -> str:
    from datetime import datetime, timezone

    return (
        datetime.now(timezone.utc)
        .isoformat(timespec="milliseconds")
        .replace("+00:00", "Z")
    )


class SettingsStore:
    """Effective runtime settings plus their TOML persistence.

    `sampling` is one shared, mutable `SamplingDefaults` instance handed to
    the backend at construction; updating it here changes what an absent
    request field means on the very next request, with no restart.
    """

    FIELDS = (
        "default_temperature",
        "default_top_p",
        "default_top_k",
        "default_reasoning_effort",
        "default_max_output_tokens",
        "idle_unload_after_seconds",
        "prompt_cache_slots",
        "queue_capacity",
    )

    def __init__(self, path: Path, *, idle_unload_fallback: int) -> None:
        self.path = path
        self.sampling = SamplingDefaults()
        # Env/config value is only the fallback; the file wins when present.
        self.idle_unload_after_seconds = idle_unload_fallback
        self.prompt_cache_slots = DEFAULT_PROMPT_CACHE_SLOTS
        self.queue_capacity = DEFAULT_QUEUE_CAPACITY
        self.loaded_at = _utc_iso()

    @classmethod
    def load(cls, config: LagunaConfig) -> "SettingsStore":
        store = cls(
            config.data_dir / SETTINGS_FILENAME,
            idle_unload_fallback=config.idle_unload_after_seconds,
        )
        if not store.path.exists():
            return store
        try:
            data = tomllib.loads(store.path.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError) as error:
            raise SettingsError(
                f"could not read settings file {store.path}: {error}"
            ) from error
        unknown = sorted(set(data) - set(cls.FIELDS))
        if unknown:
            # Fail closed at startup: a typo'd knob must not silently leave
            # the real default in place.
            raise SettingsError(
                f"unknown settings key(s) {unknown!r} in {store.path}; "
                f"known keys are {list(cls.FIELDS)!r}."
            )
        for field, value in data.items():
            store._apply(field, _validate(field, value))
        store.loaded_at = _utc_iso()
        return store

    def _apply(self, field: str, value: Any) -> None:
        if field == "default_temperature":
            self.sampling.temperature = value
        elif field == "default_top_p":
            self.sampling.top_p = value
        elif field == "default_top_k":
            self.sampling.top_k = value
        elif field == "default_reasoning_effort":
            self.sampling.reasoning_effort = value
        elif field == "default_max_output_tokens":
            self.sampling.max_output_tokens = value
        else:
            setattr(self, field, value)

    def effective(self) -> dict[str, Any]:
        return {
            "default_temperature": self.sampling.temperature,
            "default_top_p": self.sampling.top_p,
            "default_top_k": self.sampling.top_k,
            "default_reasoning_effort": self.sampling.reasoning_effort,
            "default_max_output_tokens": self.sampling.max_output_tokens,
            "idle_unload_after_seconds": self.idle_unload_after_seconds,
            "prompt_cache_slots": self.prompt_cache_slots,
            "queue_capacity": self.queue_capacity,
        }

    def update(self, changes: dict[str, Any], backend: Any = None) -> dict[str, Any]:
        """Validate every change, then apply and persist atomically.

        Nothing is applied until the whole batch validates, so a rejected
        value cannot leave the daemon half-updated.
        """
        if not isinstance(changes, dict):
            raise SettingsError("settings update must be a JSON object.")
        validated: dict[str, Any] = {}
        for field, value in changes.items():
            if field not in self.FIELDS:
                raise SettingsError(
                    f"unknown settings key {field!r}; known keys are "
                    f"{list(self.FIELDS)!r}.",
                    field=str(field),
                )
            validated[field] = _validate(field, value)
        for field, value in validated.items():
            self._apply(field, value)
        if backend is not None:
            self.apply_to_backend(backend)
        self.persist()
        self.loaded_at = _utc_iso()
        return self.effective()

    def apply_to_backend(self, backend: Any) -> None:
        """Push the backend-owned knobs onto the live backend.

        A reduced prompt-cache bound takes effect on the backend's next use
        (its own LRU trim), and a reduced queue capacity applies to new
        admissions only — neither interrupts a generation in flight.
        """
        if hasattr(backend, "_max_prompt_caches"):
            backend._max_prompt_caches = self.prompt_cache_slots
        if hasattr(backend, "_max_inflight_generations"):
            backend._max_inflight_generations = self.queue_capacity

    def persist(self) -> None:
        lines = [
            "# Laguna daemon runtime settings. Managed by PUT /v1/synth/settings;",
            "# hand-edits are read at daemon startup. Unknown keys fail startup.",
        ]
        for field, value in self.effective().items():
            if isinstance(value, str):
                rendered = json.dumps(value)
            elif isinstance(value, float):
                rendered = repr(value)
            else:
                rendered = str(value)
            lines.append(f"{field} = {rendered}")
        body = "\n".join(lines) + "\n"
        self.path.parent.mkdir(parents=True, exist_ok=True)
        # Temp-write plus rename so a crash cannot leave a torn file.
        descriptor, temp_name = tempfile.mkstemp(
            dir=str(self.path.parent), prefix=".settings-", suffix=".toml"
        )
        try:
            with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
                handle.write(body)
            os.replace(temp_name, self.path)
        except BaseException:
            try:
                os.unlink(temp_name)
            except OSError:
                pass
            raise
