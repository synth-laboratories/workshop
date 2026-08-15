from __future__ import annotations

import asyncio
import base64
import gc
import json
import os
import re
import shlex
import sys
import threading
import time
from collections import OrderedDict
from concurrent.futures import ThreadPoolExecutor
from functools import partial
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path
from typing import Any, AsyncIterator

from ..capabilities import ModelCapabilities
from ..compiler import compile_messages, compile_turn
from ..errors import ResponsesError
from ..ids import new_id
from ..telemetry import GenerationTiming
from .protocol import CompiledTurn, ModelEvent, TokenUsageEstimate, ToolBinding


_TOOL_CALL = re.compile(r"<tool_call>(.*?)</tool_call>", re.DOTALL)
_ARGUMENT = re.compile(
    r"<arg_key>(.*?)</arg_key><arg_value>(.*?)</arg_value>", re.DOTALL
)
_TOOL_CALL_STOP_GRACE_TOKENS = 16
_MIN_SYSTEM_MEMORY_BYTES = 32 * 1024**3
_MODEL_MEMORY_HEADROOM_BYTES = 8 * 1024**3
_MIN_AVAILABLE_MEMORY_BYTES = 24 * 1024**3


def _physical_memory_bytes() -> int | None:
    """Return installed physical memory without shelling out.

    MLX uses unified memory. Total capacity is the stable admission fact here;
    macOS's momentary "free" page count excludes reclaimable file cache and is
    a poor reason to reject a request that the OS can satisfy safely.
    """
    try:
        pages = int(os.sysconf("SC_PHYS_PAGES"))
        page_size = int(os.sysconf("SC_PAGE_SIZE"))
    except (AttributeError, OSError, TypeError, ValueError):
        return None
    total = pages * page_size
    return total if total > 0 else None


def _available_memory_bytes() -> int | None:
    """Return memory that can be reclaimed without forcing swap.

    The Laguna deployment target is macOS, where Mach's free, inactive, and
    purgeable page counts are the closest stable no-subprocess admission fact.
    Speculative pages are already included in ``free_count``. If the Mach
    query fails on macOS, return zero so a safety gate cannot silently open.
    """
    if sys.platform != "darwin":
        try:
            pages = int(os.sysconf("SC_AVPHYS_PAGES"))
            page_size = int(os.sysconf("SC_PAGE_SIZE"))
        except (AttributeError, OSError, TypeError, ValueError):
            return None
        available = pages * page_size
        return available if available > 0 else None

    try:
        import ctypes

        class VmStatistics64(ctypes.Structure):
            _fields_ = [
                ("free_count", ctypes.c_uint32),
                ("active_count", ctypes.c_uint32),
                ("inactive_count", ctypes.c_uint32),
                ("wire_count", ctypes.c_uint32),
                ("zero_fill_count", ctypes.c_uint64),
                ("reactivations", ctypes.c_uint64),
                ("pageins", ctypes.c_uint64),
                ("pageouts", ctypes.c_uint64),
                ("faults", ctypes.c_uint64),
                ("cow_faults", ctypes.c_uint64),
                ("lookups", ctypes.c_uint64),
                ("hits", ctypes.c_uint64),
                ("purges", ctypes.c_uint64),
                ("purgeable_count", ctypes.c_uint32),
                ("speculative_count", ctypes.c_uint32),
                ("decompressions", ctypes.c_uint64),
                ("compressions", ctypes.c_uint64),
                ("swapins", ctypes.c_uint64),
                ("swapouts", ctypes.c_uint64),
                ("compressor_page_count", ctypes.c_uint32),
                ("throttled_count", ctypes.c_uint32),
                ("external_page_count", ctypes.c_uint32),
                ("internal_page_count", ctypes.c_uint32),
                ("total_uncompressed_pages_in_compressor", ctypes.c_uint64),
            ]

        system = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
        system.mach_host_self.restype = ctypes.c_uint32
        host = system.mach_host_self()
        page_size = ctypes.c_uint32()
        if system.host_page_size(host, ctypes.byref(page_size)) != 0:
            return 0
        stats = VmStatistics64()
        count = ctypes.c_uint32(ctypes.sizeof(stats) // ctypes.sizeof(ctypes.c_int32))
        if system.host_statistics64(host, 4, ctypes.byref(stats), ctypes.byref(count)) != 0:
            return 0
        pages = stats.free_count + stats.inactive_count + stats.purgeable_count
        return pages * page_size.value
    except (AttributeError, OSError, TypeError, ValueError):
        return 0


def _required_available_memory_bytes(model_path: Path) -> int:
    weights = _model_weight_bytes(model_path)
    allocation_floor = (weights + 4 * 1024**3) if weights else 0
    return max(_MIN_AVAILABLE_MEMORY_BYTES, allocation_floor)


def _model_weight_bytes(model_path: Path) -> int | None:
    index_path = model_path / "model.safetensors.index.json"
    try:
        index = json.loads(index_path.read_text())
        declared = (index.get("metadata") or {}).get("total_size")
        shards = {
            value
            for value in (index.get("weight_map") or {}).values()
            if isinstance(value, str)
            and value.endswith(".safetensors")
            and "/" not in value
            and "\\" not in value
        }
        if not shards:
            return None
        total = sum((model_path / shard).stat().st_size for shard in shards)
        if isinstance(declared, int) and declared > 0 and total != declared:
            return None
        return total if total > 0 else None
    except (OSError, TypeError, ValueError, json.JSONDecodeError):
        return None


def _required_system_memory_bytes(model_path: Path) -> int:
    weights = _model_weight_bytes(model_path)
    weight_floor = (weights + _MODEL_MEMORY_HEADROOM_BYTES) if weights else 0
    return max(_MIN_SYSTEM_MEMORY_BYTES, weight_floor)


class _ActivatedCustomGrammarProcessor:
    """Activate a custom-tool grammar only after Laguna selects that tool.

    This preserves normal auto tool selection while preventing free-form
    sampling once a grammar-backed custom tool's raw `input` begins.
    """

    def __init__(self, tokenizer: Any, bindings: dict[str, ToolBinding]) -> None:
        import llguidance
        import llguidance.hf
        from mlx_vlm.structured import LLGuidanceLogitsProcessor, _llg_tokenizer_cache

        llg_tokenizer = _llg_tokenizer_cache.get(id(tokenizer))
        if llg_tokenizer is None:
            llg_tokenizer = llguidance.hf.from_tokenizer(tokenizer)
            _llg_tokenizer_cache[id(tokenizer)] = llg_tokenizer
        self._tokenizer = tokenizer
        self._choices: list[tuple[str, Any]] = []
        self._activation_window_tokens = 32
        self._active: Any = None
        self._active_completed = False
        for binding in bindings.values():
            format_spec = binding.format or {}
            if binding.kind != "custom" or format_spec.get("type") != "grammar":
                continue
            syntax = str(format_spec.get("syntax") or "")
            definition = str(format_spec.get("definition") or "")
            if syntax != "lark" or not definition:
                raise ResponsesError(
                    "unsupported_custom_tool_grammar",
                    "Native MLX custom tools require a non-empty Lark grammar.",
                    400,
                    error_type="invalid_request_error",
                )
            if not re.search(r"(?m)^start\s*:", definition):
                raise ResponsesError(
                    "invalid_custom_tool_grammar",
                    f"Custom tool {binding.original_name!r} grammar has no start rule.",
                    400,
                    error_type="invalid_request_error",
                )
            grammar = llguidance.grammar_from("lark", definition)
            marker = (
                f"<tool_call>{binding.model_name}"
                "<arg_key>input</arg_key><arg_value>"
            )
            marker_tokens = tokenizer.encode(marker, add_special_tokens=False)
            # Only the generated suffix can activate a grammar. Decoding the
            # entire Codex prompt for every sampled token makes a 10k+ token
            # tool turn quadratic and can delay its first call for minutes.
            self._activation_window_tokens = max(
                self._activation_window_tokens, len(marker_tokens) + 8
            )
            self._choices.append(
                (marker, LLGuidanceLogitsProcessor(grammar, llg_tokenizer))
            )

    @property
    def enabled(self) -> bool:
        return bool(self._choices)

    def __call__(self, input_ids: Any, logits: Any) -> Any:
        if self._active_completed:
            return logits
        ids = input_ids[0].tolist() if getattr(input_ids, "ndim", 1) > 1 else input_ids.tolist()
        generated = self._tokenizer.decode(
            ids[-self._activation_window_tokens :], skip_special_tokens=False
        )
        if self._active is not None:
            processor = self._active
            one_dimensional = getattr(logits, "ndim", 1) == 1
            import mlx.core as mx

            if one_dimensional:
                input_ids = mx.expand_dims(input_ids, 0)
                logits = mx.expand_dims(logits, 0)
            elif getattr(input_ids, "ndim", 1) == 1 and logits.shape[0] == 1:
                input_ids = mx.expand_dims(input_ids, 0)
            if processor.is_first_token:
                processor._setup(logits.shape[0])
                processor.is_first_token = False
            else:
                processor._consume_tokens(input_ids[:, -1].tolist())
                if all(matcher.is_accepting() for matcher in processor.ll_matchers):
                    # The raw payload now satisfies the declared grammar.
                    # Release the mask so Laguna can emit its special-token
                    # argument/tool closing envelope; those special tokens do
                    # not have ordinary UTF-8 byte representations in
                    # llguidance's vocabulary.
                    self._active_completed = True
                    return logits[0] if one_dimensional else logits
            masked = processor._apply_bitmask(logits)
            if one_dimensional:
                return masked[0]
            return masked
        for marker, processor in self._choices:
            if generated.endswith(marker):
                self._active = processor
                return processor(input_ids, logits)
        return logits


def _split_reasoning(text: str) -> tuple[str, str]:
    text = text.strip()
    if text.startswith("<think>") and "</think>" in text:
        reasoning, answer = text[len("<think>") :].split("</think>", 1)
        return reasoning.strip(), answer.strip()
    if text.startswith("</think>"):
        return "", text[len("</think>") :].strip()
    if "</think>" in text:
        # Laguna can omit the opening marker when the template already placed
        # the model inside a thinking span. Preserve the prefix as reasoning
        # instead of leaking it into assistant output.
        reasoning, answer = text.split("</think>", 1)
        return reasoning.removeprefix("<think>").strip(), answer.strip()
    return "", text


class _TurnStateMachine:
    """Classify streamed model text into reasoning, answer, and tool envelopes.

    This is the shape every reference stack converges on (mlx-lm's
    ``TextStateMachine``, vLLM/SGLang's poolside_v1 parsers): a small state
    machine over decoded text with marker-prefix holdback, so nothing is
    buffered beyond what is genuinely ambiguous. Matching on text rather than
    token ids is deliberate — it is robust to BPE merges of marker boundaries.

    States and transitions::

        reasoning --</think>--> answer          (template pre-opens <think>)
        reasoning --<tool_call>--> tool         (models may skip </think>)
        answer    --<think>--> reasoning        (Laguna interleaves thinking)
        answer    --<tool_call>--> tool
        tool      --</tool_call>--> answer      (envelope emitted complete)

    Reasoning and answer text stream out as they arrive; only the inside of a
    tool envelope is withheld, and it is emitted as one ``("tool_call", body)``
    event when its closing marker lands. An envelope still open when the model
    stops is discarded by :meth:`flush` — raw ``<tool_call>`` markup must never
    reach a client — and recorded as :attr:`truncated_tool_call`.
    """

    OPEN = "<think>"
    CLOSE = "</think>"
    TOOL_OPEN = "<tool_call>"
    TOOL_CLOSE = "</tool_call>"

    def __init__(self, *, thinking_open: bool, tools: bool = False) -> None:
        self._pending = ""
        # Laguna's chat template opens the thinking span in the prompt, so a
        # thinking-enabled turn usually starts already inside it and emits no
        # opening marker -- only the closing one. Trust the compiled turn so
        # that leading reasoning never leaks into assistant content.
        self._mode = "reasoning" if thinking_open else "answer"
        self._stripped_open = not thinking_open
        self._tools = tools
        self._tool_buffer = ""
        self.truncated_tool_call = False

    def _markers(self) -> dict[str, str]:
        if self._mode == "reasoning":
            markers = {self.CLOSE: "answer"}
        elif self._mode == "answer":
            markers = {self.OPEN: "reasoning"}
        else:
            return {self.TOOL_CLOSE: "answer"}
        if self._tools:
            markers[self.TOOL_OPEN] = "tool"
        return markers

    @staticmethod
    def _held_back_suffix(text: str, markers: dict[str, str]) -> int:
        """Length of the longest tail that could still become a marker."""
        held = 0
        for marker in markers:
            limit = min(len(text), len(marker) - 1)
            for length in range(limit, held, -1):
                if text.endswith(marker[:length]):
                    held = length
                    break
        return held

    def _ingest(self, text: str, events: list[tuple[str, str]]) -> None:
        if not text:
            return
        if self._mode == "tool":
            self._tool_buffer += text
        else:
            events.append((self._mode, text))

    @property
    def maybe_in_tool_call(self) -> bool:
        """True while held-back text may still belong to a tool envelope."""
        if self._mode == "tool":
            return True
        if not self._tools or not self._pending:
            return False
        return any(
            self._pending.endswith(self.TOOL_OPEN[:length])
            for length in range(1, len(self.TOOL_OPEN))
        )

    def feed(self, chunk: str) -> list[tuple[str, str]]:
        self._pending += chunk
        events: list[tuple[str, str]] = []
        if not self._stripped_open:
            stripped = self._pending.lstrip()
            if stripped.startswith(self.OPEN):
                self._pending = stripped[len(self.OPEN) :]
                self._stripped_open = True
            elif len(stripped) >= len(self.OPEN) or not self.OPEN.startswith(stripped):
                self._stripped_open = True
            else:
                return events

        while True:
            markers = self._markers()
            first: tuple[str, int, str] | None = None
            for marker, next_mode in markers.items():
                index = self._pending.find(marker)
                if index != -1 and (first is None or index < first[1]):
                    first = (marker, index, next_mode)
            if first is None:
                break
            marker, index, next_mode = first
            before, self._pending = self._pending[:index], self._pending[index + len(marker) :]
            self._ingest(before, events)
            if self._mode == "tool" and next_mode == "answer":
                events.append(("tool_call", self._tool_buffer))
                self._tool_buffer = ""
            self._mode = next_mode

        held = self._held_back_suffix(self._pending, self._markers())
        safe = len(self._pending) - held
        if safe > 0:
            self._ingest(self._pending[:safe], events)
            self._pending = self._pending[safe:]
        return events

    def flush(self) -> list[tuple[str, str]]:
        """Emit whatever is still held back once the model has stopped."""
        if self._mode == "tool":
            # An unterminated envelope has no faithful representation: it is
            # neither a dispatchable call nor assistant prose. Discard it and
            # record the truncation rather than leaking raw markup.
            self._tool_buffer = ""
            self._pending = ""
            self.truncated_tool_call = True
            return []
        if not self._pending:
            return []
        remainder, self._pending = self._pending, ""
        return [(self._mode, remainder)]


class _IncrementalReasoningSplitter(_TurnStateMachine):
    """Tool-marker-free view of the state machine, kept for its test surface."""

    def __init__(self, *, thinking_open: bool) -> None:
        super().__init__(thinking_open=thinking_open, tools=False)


def _parse_value(raw: str) -> Any:
    try:
        return json.loads(raw)
    except ValueError:
        return raw


def _resolve_poolside_tool_alias(
    name: str,
    pairs: list[tuple[str, str]],
    bindings: dict[str, ToolBinding],
) -> tuple[ToolBinding, list[tuple[str, str]]] | None:
    """Lower one evidenced Laguna-native tool dialect call onto Codex.

    Laguna's checkpoint can emit a small pretrained file-tool dialect even when
    modern Codex advertises only ``exec_command``. Translate only exact,
    lossless cases observed in real Harbor traces. Unknown aliases and extra
    arguments still fail closed instead of being guessed into executable code.
    """

    binding = bindings.get("exec_command")
    if binding is None or binding.kind != "function":
        return None

    if name in {"read", "read_file"} and len(pairs) == 1 and pairs[0][0] == "path":
        path = _parse_value(pairs[0][1])
        if (
            not isinstance(path, str)
            or not path
            or len(path) > 4096
            or "\x00" in path
        ):
            return None
        command_path = (
            f"./{path}"
            if not Path(path).is_absolute() and path.startswith("-")
            else path
        )
        command = f"sed -n '1,240p' {shlex.quote(command_path)}"
        # The synthesized command is already a plain string; argument
        # coercion is schema-driven and keeps declared strings verbatim.
        return binding, [("cmd", command)]

    if name == "grep":
        arguments = {key: _parse_value(value) for key, value in pairs}
        if set(arguments) != {"output_mode", "path", "pattern"}:
            return None
        pattern = arguments["pattern"]
        path = arguments["path"]
        mode = arguments["output_mode"]
        flags = {"content": "-n", "files_with_matches": "-l", "count": "-c"}
        if (
            not isinstance(pattern, str)
            or not pattern
            or len(pattern) > 4096
            or "\x00" in pattern
            or not isinstance(path, str)
            or not path
            or len(path) > 4096
            or "\x00" in path
            or mode not in flags
        ):
            return None
        command = (
            f"rg {flags[mode]} -- {shlex.quote(pattern)} {shlex.quote(path)}"
        )
        # The synthesized command is already a plain string; argument
        # coercion is schema-driven and keeps declared strings verbatim.
        return binding, [("cmd", command)]

    if name == "write":
        arguments = {key: _parse_value(value) for key, value in pairs}
        content_keys = set(arguments) - {"path"}
        if set(arguments) not in ({"input", "path"}, {"contents", "path"}):
            return None
        path = arguments["path"]
        content = arguments[next(iter(content_keys))]
        if (
            not isinstance(path, str)
            or not path
            or len(path) > 4096
            or "\x00" in path
            or not isinstance(content, str)
            or len(content.encode("utf-8")) > 1_048_576
        ):
            return None
        command_path = (
            f"./{path}"
            if not Path(path).is_absolute() and path.startswith("-")
            else path
        )
        parent = str(Path(command_path).parent)
        encoded = base64.b64encode(content.encode("utf-8")).decode("ascii")
        command = (
            f"mkdir -p {shlex.quote(parent)} && printf %s {encoded} | "
            f"base64 -D > {shlex.quote(command_path)}"
        )
        # The synthesized command is already a plain string; argument
        # coercion is schema-driven and keeps declared strings verbatim.
        return binding, [("cmd", command)]

    return None


def _coerce_arguments(
    pairs: list[tuple[str, str]], binding: ToolBinding
) -> dict[str, Any]:
    """Type argument values by the tool's declared schema, like the template.

    Laguna's chat template renders string-valued arguments raw and everything
    else as JSON, so the way back out must be schema-driven (vLLM/SGLang's
    poolside_v1 behavior): a declared string keeps its text verbatim —
    whitespace in file content is significant, and a path spelled "123" must
    not become an integer — while undeclared or non-string values fall back
    to JSON decoding.
    """
    properties = (binding.schema or {}).get("properties") or {}
    arguments: dict[str, Any] = {}
    for key, raw in pairs:
        declared = properties.get(key)
        if isinstance(declared, dict) and declared.get("type") == "string":
            arguments[key] = raw
        else:
            arguments[key] = _parse_value(raw)
    return arguments


def _envelope_event(body: str, bindings: dict[str, ToolBinding]) -> ModelEvent:
    """Parse one complete tool-envelope body into its typed call event."""
    name = body.split("<arg_key>", 1)[0].strip()
    pairs = [(key.strip(), value) for key, value in _ARGUMENT.findall(body)]
    binding = bindings.get(name)
    if binding is None:
        alias = _resolve_poolside_tool_alias(name, pairs, bindings)
        if alias is not None:
            binding, pairs = alias
    if binding is None:
        argument_keys = sorted({key for key, _ in pairs})
        raise ResponsesError(
            "unknown_tool_call",
            f"The model selected unknown tool {name!r} with argument keys "
            f"{argument_keys!r}.",
            422,
            error_type="model_error",
        )
    arguments = _coerce_arguments(pairs, binding)
    raw_arguments = {key: value for key, value in pairs}
    call_id = new_id("call")
    serialized = json.dumps(arguments, ensure_ascii=False, separators=(",", ":"))
    if binding.kind == "custom":
        parsed_raw = arguments.get("input")
        raw = parsed_raw if isinstance(parsed_raw, str) else raw_arguments.get("input")
        if not isinstance(raw, str):
            raise ResponsesError(
                "invalid_custom_tool_input",
                f"Custom tool {binding.original_name!r} did not return a raw input string.",
                422,
                error_type="model_error",
            )
        return ModelEvent(
            kind="custom_tool_call",
            name=binding.original_name,
            namespace=binding.namespace,
            call_id=call_id,
            input=raw,
        )
    event_kind = {
        "tool_search": "tool_search_call",
        "shell": "shell_call",
        "local_shell": "shell_call",
        "apply_patch": "apply_patch_call",
        "mcp": "mcp_call",
    }.get(binding.kind, "function_call")
    return ModelEvent(
        kind=event_kind,
        name=binding.original_name,
        namespace=binding.namespace,
        call_id=call_id,
        arguments=serialized,
    )


def _rehydrate_tool_calls(
    text: str, bindings: dict[str, ToolBinding]
) -> tuple[list[ModelEvent], str]:
    events: list[ModelEvent] = []
    for match in _TOOL_CALL.finditer(text):
        events.append(_envelope_event(match.group(1), bindings))
    remainder = _TOOL_CALL.sub("", text).strip()
    # Codex treats an assistant message accompanying a tool call as a terminal
    # answer after it dispatches that call. Laguna commonly appends a planning
    # sentence (for example, "I'll start by...") to the same sampled turn.
    # Keep tool-bearing turns unambiguous so the client always continues with
    # the tool output; prose belongs on the subsequent model turn.
    return events, "" if events else remainder


class NativeMlxBackend:
    """Direct, self-contained MLX backend using the open Laguna implementation."""

    SUPPORTED_MLX_LM = {"0.31.3"}
    SUPPORTED_MLX_VLM = {"0.6.6"}

    def __init__(
        self,
        *,
        model_path: Path,
        adapter_path: str | None = None,
        context_length: int = 262_144,
        system_memory_bytes: int | None = None,
        available_memory_bytes: int | None = None,
    ) -> None:
        self.model_path = model_path
        self.adapter_path = adapter_path
        self._capabilities = ModelCapabilities(context_length=context_length)
        self._model: Any = None
        self._tokenizer: Any = None
        self._load_lock = asyncio.Lock()
        self._cancel_flags: dict[str, threading.Event] = {}
        # MLX GPU streams are thread-affine. Model load and every generation
        # run on this single owned worker rather than arbitrary asyncio pool
        # threads.
        self._executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="laguna-mlx")
        self._prompt_caches: OrderedDict[str, Any] = OrderedDict()
        # A 50k-token Codex prefill cache is large on a 24B model. Two entries
        # preserve active Desktop continuation while bounding unified-memory
        # residency across ephemeral integration sessions.
        self._max_prompt_caches = 2
        # One Apple GPU generation is admitted at a time. Keep the pending
        # queue bounded so a busy Desktop cannot turn memory pressure into an
        # unbounded backlog.
        self._generation_slot = asyncio.Semaphore(1)
        self._admission_lock = asyncio.Lock()
        self._inflight_generations = 0
        self._max_inflight_generations = 9
        self._generations: dict[str, GenerationTiming] = {}
        # Completed timings linger briefly so the runner can read a turn's real
        # numbers after its stream has closed.
        self._recent_generations: OrderedDict[str, GenerationTiming] = OrderedDict()
        self._max_recent_generations = 32
        self._loading = False
        self._last_used_at = time.time()
        self._system_memory_bytes = (
            system_memory_bytes
            if system_memory_bytes is not None
            else _physical_memory_bytes()
        )
        self._available_memory_override = available_memory_bytes

    async def capabilities(self, model: str) -> ModelCapabilities:
        return self._capabilities

    async def load(self) -> None:
        """Explicit residency request from the control surface.

        Loading counts as use: it must reset the idle clock so the watchdog
        does not evict weights the caller just asked for.
        """
        await self._ensure_loaded()
        self._last_used_at = time.time()

    async def _ensure_loaded(self) -> None:
        if self._model is not None:
            return
        async with self._load_lock:
            if self._model is not None:
                return
            self._loading = True
            try:
                required_memory = _required_system_memory_bytes(self.model_path)
                available_memory = (
                    self._available_memory_override
                    if self._available_memory_override is not None
                    else _available_memory_bytes()
                )
                required_available = _required_available_memory_bytes(self.model_path)
                if (
                    self._system_memory_bytes is not None
                    and self._system_memory_bytes < required_memory
                ) or (
                    available_memory is not None
                    and available_memory < required_available
                ):
                    capacity_shortfall = (
                        self._system_memory_bytes is not None
                        and self._system_memory_bytes < required_memory
                    )
                    available_gib = min(
                        value
                        for value in (self._system_memory_bytes, available_memory)
                        if value is not None
                    ) / 1024**3
                    required_gib = (
                        required_memory if capacity_shortfall else required_available
                    ) / 1024**3
                    raise ResponsesError(
                        "insufficient_system_memory",
                        f"{self.model_path.name} was not loaded because this Mac has "
                        f"{available_gib:.1f} GiB of unified memory; "
                        f"at least {required_gib:.1f} GiB is required.",
                        503,
                        error_type="server_error",
                    )
                if _model_weight_bytes(self.model_path) is None:
                    raise ResponsesError(
                        "model_not_found",
                        f"MLX model artifacts are incomplete or invalid: {self.model_path}",
                        503,
                        error_type="server_error",
                    )
                installed = version("mlx-lm")
                installed_vlm = version("mlx-vlm")
            except PackageNotFoundError as exc:
                raise ResponsesError(
                    "mlx_runtime_unavailable",
                    "mlx, mlx-lm, and mlx-vlm must be installed in the Laguna daemon environment.",
                    503,
                    error_type="server_error",
                ) from exc
            if installed not in self.SUPPORTED_MLX_LM:
                raise ResponsesError(
                    "unsupported_mlx_lm_version",
                    f"mlx-lm {installed} is not supported; expected one of {sorted(self.SUPPORTED_MLX_LM)}.",
                    503,
                    error_type="server_error",
                )
            if installed_vlm not in self.SUPPORTED_MLX_VLM:
                raise ResponsesError(
                    "unsupported_mlx_vlm_version",
                    f"mlx-vlm {installed_vlm} is not supported; expected one of {sorted(self.SUPPORTED_MLX_VLM)}.",
                    503,
                    error_type="server_error",
                )
            def load_model() -> tuple[Any, Any]:
                # mlx-vlm carries the open Laguna architecture and NVFP4
                # loader. We use only its in-process model primitives, never
                # its HTTP server or the closed Poolside sidecar.
                from mlx_vlm import load

                return load(
                    str(self.model_path),
                    adapter_path=self.adapter_path,
                    lazy=False,
                    # Laguna uses the Mistral tokenizer family. Transformers
                    # otherwise retains the historical broken regex and emits
                    # an incorrect-tokenization warning for this checkpoint.
                    fix_mistral_regex=True,
                )

            try:
                loop = asyncio.get_running_loop()
                self._model, self._tokenizer = await loop.run_in_executor(
                    self._executor, load_model
                )
            finally:
                self._loading = False

    async def compile(
        self,
        request: dict[str, Any],
        context_items: list[dict[str, Any]],
        generation_id: str,
    ) -> CompiledTurn:
        await self._ensure_loaded()
        self._last_used_at = time.time()
        format_spec = ((request.get("text") or {}).get("format") or {})
        request_for_prompt = dict(request)
        if format_spec.get("type") in {"json_object", "json_schema"}:
            schema = format_spec.get("schema")
            instruction = "Return only valid JSON."
            if isinstance(schema, dict):
                instruction += f" The JSON must match this schema: {json.dumps(schema, separators=(',', ':'))}"
            prior = str(request.get("instructions") or "")
            request_for_prompt["instructions"] = f"{prior}\n{instruction}".strip()
        loop = asyncio.get_running_loop()
        return await loop.run_in_executor(
            self._executor,
            partial(
                compile_turn,
                request_for_prompt,
                context_items,
                generation_id,
                prompt_builder=self._prompt_builder,
                defaults=getattr(self, "sampling_defaults", None),
            ),
        )

    async def compile_messages(self, **kwargs: Any) -> CompiledTurn:
        await self._ensure_loaded()
        self._last_used_at = time.time()
        kwargs.setdefault("prompt_builder", self._prompt_builder)
        loop = asyncio.get_running_loop()
        return await loop.run_in_executor(
            self._executor, partial(compile_messages, **kwargs)
        )

    def _prompt_builder(self, messages: list[dict[str, Any]], **kwargs: Any) -> str | list[int]:
        return self._tokenizer.apply_chat_template(messages, tokenize=False, **kwargs)

    async def count_tokens(self, turn: CompiledTurn) -> TokenUsageEstimate:
        if isinstance(turn.prompt, list):
            return TokenUsageEstimate(len(turn.prompt))
        if isinstance(turn.prompt, str):
            loop = asyncio.get_running_loop()
            encoded = await loop.run_in_executor(
                self._executor,
                partial(self._tokenizer.encode, turn.prompt, add_special_tokens=False),
            )
            return TokenUsageEstimate(len(encoded))
        return TokenUsageEstimate(0)

    async def stream(self, turn: CompiledTurn) -> AsyncIterator[ModelEvent]:
        await self._ensure_loaded()
        async with self._admission_lock:
            if self._inflight_generations >= self._max_inflight_generations:
                raise ResponsesError(
                    "model_queue_saturated",
                    "The local MLX generation queue is full; retry after an active response completes.",
                    429,
                    error_type="server_error",
                )
            self._inflight_generations += 1
            timing = GenerationTiming(
                generation_id=turn.generation_id, queued_at=time.monotonic()
            )
            self._generations[turn.generation_id] = timing
        try:
            await self._generation_slot.acquire()
        except BaseException:
            async with self._admission_lock:
                self._inflight_generations -= 1
                self._generations.pop(turn.generation_id, None)
            raise
        async with self._admission_lock:
            timing.admitted_at = time.monotonic()
            timing.phase = "compiling"
        # Hugging Face's fast tokenizer is not re-entrant. Prompt compilation,
        # token counting, grammar construction, and generation all run on the
        # same owned executor so queued requests cannot borrow it concurrently.
        try:
            loop = asyncio.get_running_loop()
            custom_grammar_processor = await loop.run_in_executor(
                self._executor,
                partial(_ActivatedCustomGrammarProcessor, self._tokenizer, turn.bindings),
            )
            input_usage = await self.count_tokens(turn)
        except ResponsesError:
            self._generation_slot.release()
            async with self._admission_lock:
                self._inflight_generations -= 1
                self._generations.pop(turn.generation_id, None)
            raise
        except (ImportError, RuntimeError, ValueError) as exc:
            self._generation_slot.release()
            async with self._admission_lock:
                self._inflight_generations -= 1
                self._generations.pop(turn.generation_id, None)
            raise ResponsesError(
                "invalid_custom_tool_grammar",
                f"Could not compile the custom tool grammar: {exc}",
                400,
                error_type="invalid_request_error",
            ) from exc
        except BaseException:
            self._generation_slot.release()
            async with self._admission_lock:
                self._inflight_generations -= 1
                self._generations.pop(turn.generation_id, None)
            raise
        queue: asyncio.Queue[tuple[str, Any]] = asyncio.Queue()
        cancel_flag = threading.Event()
        self._cancel_flags[turn.generation_id] = cancel_flag
        async with self._admission_lock:
            timing.compiled_at = time.monotonic()
            timing.prompt_tokens = input_usage.input_tokens
            # Prefill runs until the model emits its first token.
            timing.phase = "prefill"

        def worker() -> None:
            output_tokens = 0
            measured_decode_tps: float | None = None
            try:
                from mlx_vlm import stream_generate
                from mlx_vlm.sample_utils import make_sampler

                sampler = make_sampler(
                    temp=turn.temperature, top_p=turn.top_p, top_k=turn.top_k
                )
                logits_processors = []
                if custom_grammar_processor.enabled:
                    logits_processors.append(custom_grammar_processor)
                format_spec = turn.structured_format or {}
                if format_spec.get("type") in {"json_schema", "json_object"}:
                    from mlx_vlm.structured import build_json_schema_logits_processor

                    schema = (
                        format_spec.get("schema")
                        if format_spec.get("type") == "json_schema"
                        else {"type": "object"}
                    )
                    if not isinstance(schema, dict):
                        raise ValueError("json_schema format requires an object schema")
                    logits_processors.append(
                        build_json_schema_logits_processor(self._tokenizer, schema)
                    )
                prompt_cache_state = None
                cache_key = turn.prompt_cache_key
                if isinstance(cache_key, str) and cache_key:
                    from mlx_vlm import PromptCacheState

                    prompt_cache_state = self._prompt_caches.get(cache_key)
                    if prompt_cache_state is None:
                        prompt_cache_state = PromptCacheState()
                        self._prompt_caches[cache_key] = prompt_cache_state
                    else:
                        self._prompt_caches.move_to_end(cache_key)
                    while len(self._prompt_caches) > self._max_prompt_caches:
                        self._prompt_caches.popitem(last=False)
                        try:
                            import mlx.core as mx

                            mx.clear_cache()
                        except RuntimeError:
                            pass
                final_finish_reason = None
                cached_tokens = 0
                prompt_tokens = 0
                for response in stream_generate(
                    self._model,
                    self._tokenizer,
                    prompt=turn.prompt,
                    max_tokens=turn.max_output_tokens,
                    sampler=sampler,
                    verbose=False,
                    prompt_cache_state=prompt_cache_state,
                    logits_processors=logits_processors,
                ):
                    if cancel_flag.is_set():
                        break
                    output_tokens = max(output_tokens, int(response.generation_tokens or 0))
                    prompt_tokens = max(prompt_tokens, int(response.prompt_tokens or 0))
                    cached_tokens = max(cached_tokens, int(response.cached_tokens or 0))
                    source_decode_tps = float(response.generation_tps or 0.0)
                    if source_decode_tps > 0:
                        # MLX reports cumulative generation throughput at the
                        # token source. Keep the latest value rather than
                        # deriving a rate from event-loop delivery timing.
                        measured_decode_tps = source_decode_tps
                    if response.text:
                        loop.call_soon_threadsafe(
                            queue.put_nowait,
                            (
                                "chunk",
                                (
                                    response.text,
                                    time.monotonic(),
                                    output_tokens,
                                    prompt_tokens,
                                    cached_tokens,
                                    measured_decode_tps,
                                ),
                            ),
                        )
                    final_finish_reason = response.finish_reason or final_finish_reason
                loop.call_soon_threadsafe(
                    queue.put_nowait,
                    (
                        "done",
                        (
                            output_tokens,
                            prompt_tokens,
                            cached_tokens,
                            measured_decode_tps,
                            final_finish_reason,
                            cancel_flag.is_set(),
                        ),
                    ),
                )
            except BaseException as exc:
                loop.call_soon_threadsafe(queue.put_nowait, ("error", exc))

        worker_future = loop.run_in_executor(self._executor, worker)
        structured_tool_chunks: list[str] = []
        reasoning_text = ""
        # Structured turns emit grammar-constrained JSON directly; there is no
        # thinking span or tool envelope to split.
        splitter = (
            None
            if turn.structured_format
            else _TurnStateMachine(
                thinking_open=turn.enable_thinking, tools=bool(turn.bindings)
            )
        )
        envelopes_completed = 0
        last_close_tokens: int | None = None
        answer_after_close = False
        tool_stop = False
        try:
            while True:
                kind, payload = await queue.get()
                if kind == "error":
                    raise ResponsesError(
                        "model_generation_failed",
                        str(payload),
                        500,
                        error_type="model_error",
                    )
                if kind == "chunk":
                    (
                        chunk,
                        sampled_at,
                        output_tokens,
                        prompt_tokens,
                        cached_tokens,
                        measured_decode_tps,
                    ) = payload
                    chunk = str(chunk)
                    sampled_at = float(sampled_at)
                    if timing.first_token_at is None:
                        timing.first_token_at = sampled_at
                        timing.phase = "decode"
                    timing.last_token_at = sampled_at
                    timing.output_tokens = int(output_tokens)
                    timing.prompt_tokens = int(prompt_tokens) or timing.prompt_tokens
                    timing.cached_tokens = int(cached_tokens)
                    timing.measured_decode_tps = measured_decode_tps
                    if splitter is None:
                        if turn.bindings:
                            structured_tool_chunks.append(chunk)
                        else:
                            yield ModelEvent(kind="text_delta", delta=chunk)
                        continue
                    # Reasoning and answer text stream as they arrive; only the
                    # inside of a tool envelope is withheld, and each envelope
                    # is emitted as a typed call event the moment it closes.
                    for split_kind, text in splitter.feed(chunk):
                        if split_kind == "reasoning":
                            reasoning_text += text
                            yield ModelEvent(kind="reasoning_delta", delta=text)
                        elif split_kind == "tool_call":
                            yield _envelope_event(text, turn.bindings)
                            envelopes_completed += 1
                            last_close_tokens = int(output_tokens)
                            answer_after_close = False
                        elif envelopes_completed:
                            # Codex treats prose accompanying a dispatched call
                            # as a terminal answer, so post-call prose is
                            # dropped; it only arms the stop grace below.
                            if text.strip():
                                answer_after_close = True
                        else:
                            yield ModelEvent(kind="text_delta", delta=text)
                    if (
                        envelopes_completed
                        and answer_after_close
                        and last_close_tokens is not None
                        and not splitter.maybe_in_tool_call
                        and int(output_tokens) - last_close_tokens
                        >= _TOOL_CALL_STOP_GRACE_TOKENS
                    ):
                        # Laguna normally ends a tool turn with its own eos;
                        # when it rambles into prose instead, stop paying
                        # decode for text the protocol will drop. A partial
                        # `<tool_call>` prefix in the holdback keeps the turn
                        # alive, so sibling calls survive the grace window.
                        tool_stop = True
                        cancel_flag.set()
                    continue
                if kind == "done":
                    (
                        output_tokens,
                        prompt_tokens,
                        cached_tokens,
                        measured_decode_tps,
                        backend_finish,
                        cancelled,
                    ) = payload
                    # mlx reports the real prefill/cache split; never estimate.
                    timing.output_tokens = output_tokens
                    timing.prompt_tokens = prompt_tokens or timing.prompt_tokens
                    timing.cached_tokens = cached_tokens
                    timing.measured_decode_tps = measured_decode_tps
                    break
            await worker_future
            if cancelled and not tool_stop:
                yield ModelEvent(kind="finish", finish_reason="cancelled")
                return
            if splitter is not None:
                # An unterminated envelope is discarded by flush (raw markup
                # must never reach a client); everything else is classified
                # exactly as it would have been mid-stream.
                for split_kind, text in splitter.flush():
                    if split_kind == "reasoning":
                        reasoning_text += text
                        yield ModelEvent(kind="reasoning_delta", delta=text)
                    elif not envelopes_completed:
                        yield ModelEvent(kind="text_delta", delta=text)
            elif turn.bindings:
                # Structured turns with tools have no incremental envelope
                # parsing; rehydrate from the buffered text at the end.
                answer = "".join(structured_tool_chunks)
                tool_events, remainder = _rehydrate_tool_calls(answer, turn.bindings)
                for event in tool_events:
                    yield event
                envelopes_completed += len(tool_events)
                if remainder and not tool_events:
                    yield ModelEvent(kind="text_delta", delta=remainder)
            # Count reasoning tokens; never estimate them. Re-encoding the
            # reasoning span costs one tokenizer pass on the owned executor
            # and cannot exceed what the model actually generated.
            reasoning_tokens = 0
            if reasoning_text:
                encoded_reasoning = await loop.run_in_executor(
                    self._executor,
                    partial(
                        self._tokenizer.encode,
                        reasoning_text,
                        add_special_tokens=False,
                    ),
                )
                reasoning_tokens = min(len(encoded_reasoning), output_tokens)
            yield ModelEvent(
                kind="usage",
                input_tokens=prompt_tokens or input_usage.input_tokens,
                output_tokens=output_tokens,
                reasoning_tokens=reasoning_tokens,
                metadata={"cached_tokens": cached_tokens},
            )
            finish_reason = backend_finish or (
                "length" if output_tokens >= turn.max_output_tokens else "stop"
            )
            if tool_stop or (envelopes_completed and finish_reason == "stop"):
                # Every reference stack rewrites a natural stop to a tool
                # finish when calls were dispatched; clients continue the
                # loop based on this field.
                finish_reason = "tool_call"
            yield ModelEvent(kind="finish", finish_reason=finish_reason)
        finally:
            cancel_flag.set()
            # The admission slot must reopen only once the MLX worker thread
            # has genuinely stopped, and awaiting that here cannot guarantee
            # it: when the caller is already being cancelled — the ordinary
            # client-disconnect path — an `await` in this `finally` returns
            # immediately instead of joining the thread. The executor has a
            # single worker, so reopening the slot early lets the next request
            # take it, queue its own worker behind the orphan, and sit in
            # prefill forever while the whole queue stalls behind it.
            #
            # Attaching the release to the worker future instead makes the
            # slot's lifetime exactly the thread's lifetime, cancelled or not.
            # The callback runs on the event loop, so these mutations are
            # atomic with respect to the `_admission_lock` holders, which never
            # await while holding it.
            worker_future.add_done_callback(
                partial(self._retire_generation, turn.generation_id, timing)
            )

    def _retire_generation(
        self, generation_id: str, timing: GenerationTiming, _future: Any
    ) -> None:
        """Release the admission slot once its worker thread has finished."""
        self._cancel_flags.pop(generation_id, None)
        self._generation_slot.release()
        self._inflight_generations -= 1
        timing.completed_at = time.monotonic()
        timing.phase = "complete"
        self._generations.pop(generation_id, None)
        self._recent_generations[generation_id] = timing
        while len(self._recent_generations) > self._max_recent_generations:
            self._recent_generations.popitem(last=False)
        self._last_used_at = time.time()

    def memory_bytes(self) -> int | None:
        """Bytes MLX is actually holding, or None when it cannot be measured.

        `mx.get_active_memory()` is the real allocator figure. It is used in
        preference to the model's on-disk size, which is a fact about the
        filesystem rather than about memory, and in preference to process RSS,
        which under-reports Metal buffers on Apple silicon.
        """
        if self._model is None:
            return 0
        try:
            import mlx.core as mx

            return int(mx.get_active_memory())
        except (ImportError, RuntimeError, AttributeError):
            return None

    def residency(self, idle_unload_after_seconds: int) -> dict[str, Any]:
        """Expose the real in-process MLX residency used by native Responses."""
        loaded = self._model is not None
        last_used_at = int(self._last_used_at * 1000)
        return {
            "loaded": loaded,
            "idle_seconds": max(0, int(time.time() - self._last_used_at)),
            "last_used_at": last_used_at,
            "free_at": (
                last_used_at + idle_unload_after_seconds * 1000
                if loaded and idle_unload_after_seconds > 0
                else None
            ),
        }

    async def unload_if_idle(self, idle_unload_after_seconds: int) -> bool:
        """Release native MLX weights once no generation has used them recently."""
        if idle_unload_after_seconds <= 0 or self._model is None:
            return False
        if time.time() - self._last_used_at < idle_unload_after_seconds:
            return False
        async with self._load_lock:
            async with self._admission_lock:
                if (
                    self._model is None
                    or self._inflight_generations > 0
                    or time.time() - self._last_used_at < idle_unload_after_seconds
                ):
                    return False
            await self._release_model_memory()
            return True

    async def _release_model_memory(self) -> None:
        def release() -> None:
            self._model = None
            self._tokenizer = None
            self._prompt_caches.clear()
            gc.collect()
            try:
                import mlx.core as mx

                mx.clear_cache()
            except (ImportError, RuntimeError):
                pass

        await asyncio.get_running_loop().run_in_executor(self._executor, release)

    def diagnostics(self) -> dict[str, Any]:
        phases = {
            generation_id: timing.phase
            for generation_id, timing in self._generations.items()
        }
        return {
            "loaded": self._model is not None,
            "loading": self._loading,
            "inflight_generations": self._inflight_generations,
            "max_inflight_generations": self._max_inflight_generations,
            "generation_slot_available": not self._generation_slot.locked(),
            "queued_generations": sum(phase == "queued" for phase in phases.values()),
            "generation_phases": phases,
        }

    def generation_metrics(self, generation_id: str) -> GenerationTiming | None:
        """Real timings for a generation, during or shortly after its run."""
        timing = self._generations.get(generation_id)
        if timing is not None:
            return timing
        return self._recent_generations.get(generation_id)

    def active_generation(self) -> GenerationTiming | None:
        """The generation currently holding the GPU slot, if any."""
        for timing in self._generations.values():
            if timing.phase != "queued":
                return timing
        return next(iter(self._generations.values()), None)

    def queue_state(self) -> dict[str, int]:
        return {
            "depth": self._inflight_generations,
            "capacity": self._max_inflight_generations,
        }

    async def cancel(self, generation_id: str) -> None:
        flag = self._cancel_flags.get(generation_id)
        if flag is not None:
            flag.set()

    async def close(self) -> None:
        for flag in self._cancel_flags.values():
            flag.set()
        self._cancel_flags.clear()
        await self._release_model_memory()
        self._executor.shutdown(wait=True, cancel_futures=True)
