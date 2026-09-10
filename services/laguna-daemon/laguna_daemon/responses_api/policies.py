"""Named inference policies: one resident base, several selectable adapters.

A policy is a model id the client can ask for. `poolside/Laguna-XS-2.1-NVFP4-mlx`
is the base weights with nothing attached; `synth/Laguna-XS-2.1-ft` is those
same weights with a LoRA attached. Both are served by one process holding one
copy of the base.

The pin travels in the request's `model` field rather than in daemon state.
Codex owns the request body and always sends `model`, so a policy chosen for
one conversation cannot leak into another's turn — which is exactly what
mutating a process-global adapter allowed.
"""

from __future__ import annotations

import json
import os
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

POLICIES_FILENAME = "policies.json"
SCHEMA_VERSION = "1.0"
REQUIRED_ADAPTER_FILES = ("adapter_config.json", "adapters.safetensors")


class PolicyError(RuntimeError):
    """A rejected policy registration; `field` names the offender."""

    def __init__(self, message: str, *, field: str | None = None) -> None:
        super().__init__(message)
        self.field = field


@dataclass(frozen=True, slots=True)
class Policy:
    """One selectable model id. `adapter_path` of None is the base weights."""

    model_id: str
    adapter_path: Path | None
    digest: str | None = None
    title: str | None = None

    @property
    def is_base(self) -> bool:
        return self.adapter_path is None

    def json(self) -> dict[str, Any]:
        return {
            "model_id": self.model_id,
            "adapter_path": None if self.adapter_path is None else str(self.adapter_path),
            "digest": self.digest,
            "title": self.title,
            "is_base": self.is_base,
        }


def _validate_adapter(path: Path) -> Path:
    resolved = path.expanduser().resolve()
    if not resolved.is_dir():
        raise PolicyError(
            f"adapter_path is not a directory: {resolved}", field="adapter_path"
        )
    for required in REQUIRED_ADAPTER_FILES:
        if not (resolved / required).is_file():
            raise PolicyError(
                f"not mlx-lora.v1: {resolved} is missing {required}", field="adapter_path"
            )
    return resolved


class PolicyRegistry:
    """The daemon's set of selectable policies, persisted beside its api key.

    The base policy is always present and cannot be removed or overwritten: a
    daemon that cannot serve its own default model has nothing to fall back to.
    """

    def __init__(self, data_dir: Path, default_model: str) -> None:
        self._path = data_dir / POLICIES_FILENAME
        self._default_model = default_model
        self._policies: dict[str, Policy] = {
            default_model: Policy(model_id=default_model, adapter_path=None, title="Base")
        }
        self._load()

    @property
    def default_model(self) -> str:
        return self._default_model

    def _load(self) -> None:
        if not self._path.is_file():
            return
        try:
            raw = json.loads(self._path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise PolicyError(f"{self._path} is not readable policy JSON: {error}") from error
        for entry in raw.get("policies", []):
            model_id = str(entry.get("model_id") or "").strip()
            adapter = entry.get("adapter_path")
            if not model_id or model_id == self._default_model or not adapter:
                continue
            path = Path(str(adapter))
            # A policy whose bytes vanished is dropped rather than served: the
            # alternative is a picker entry that fails at the first turn.
            if not path.is_dir():
                continue
            self._policies[model_id] = Policy(
                model_id=model_id,
                adapter_path=path,
                digest=entry.get("digest"),
                title=entry.get("title"),
            )

    def _save(self) -> None:
        payload = {
            "schema_version": SCHEMA_VERSION,
            "policies": [
                policy.json() for policy in self._policies.values() if not policy.is_base
            ],
        }
        self._path.parent.mkdir(parents=True, exist_ok=True)
        handle, temporary = tempfile.mkstemp(dir=str(self._path.parent))
        try:
            with os.fdopen(handle, "w", encoding="utf-8") as file:
                json.dump(payload, file, indent=2)
                file.write("\n")
            os.replace(temporary, self._path)
        except BaseException:
            Path(temporary).unlink(missing_ok=True)
            raise

    def list(self) -> list[Policy]:
        base = self._policies[self._default_model]
        others = sorted(
            (policy for policy in self._policies.values() if not policy.is_base),
            key=lambda policy: policy.model_id,
        )
        return [base, *others]

    def get(self, model_id: str) -> Policy | None:
        return self._policies.get(model_id)

    def resolve(self, model_id: str | None) -> Policy:
        """Resolve a requested model id, or fail rather than guess.

        Serving the base when the client asked for an adapter would be the
        silent wrong-policy answer this design exists to prevent.
        """
        requested = (model_id or self._default_model).strip()
        policy = self._policies.get(requested)
        if policy is None:
            known = ", ".join(sorted(self._policies))
            raise PolicyError(
                f"Unknown model {requested!r}. Registered policies: {known}.",
                field="model",
            )
        return policy

    def register(
        self,
        model_id: str,
        adapter_path: str | Path,
        *,
        digest: str | None = None,
        title: str | None = None,
    ) -> Policy:
        model_id = str(model_id).strip()
        if not model_id:
            raise PolicyError("model_id is required.", field="model_id")
        if model_id == self._default_model:
            raise PolicyError(
                "The base model id cannot be redefined as an adapter policy.",
                field="model_id",
            )
        policy = Policy(
            model_id=model_id,
            adapter_path=_validate_adapter(Path(str(adapter_path))),
            digest=digest,
            title=title,
        )
        self._policies[model_id] = policy
        self._save()
        return policy

    def remove(self, model_id: str) -> bool:
        if model_id == self._default_model:
            raise PolicyError("The base policy cannot be removed.", field="model_id")
        removed = self._policies.pop(model_id, None) is not None
        if removed:
            self._save()
        return removed
