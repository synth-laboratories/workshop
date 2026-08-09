from __future__ import annotations

import asyncio
import gc
import json
import re
import threading
import time
from collections import OrderedDict
from concurrent.futures import ThreadPoolExecutor
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path
from typing import Any, AsyncIterator

from ..capabilities import ModelCapabilities
from ..compiler import compile_turn
from ..errors import ResponsesError
from ..ids import new_id
from .protocol import CompiledTurn, ModelEvent, TokenUsageEstimate, ToolBinding


_TOOL_CALL = re.compile(r"<tool_call>(.*?)</tool_call>", re.DOTALL)
_ARGUMENT = re.compile(
    r"<arg_key>(.*?)</arg_key><arg_value>(.*?)</arg_value>", re.DOTALL
)


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
        generated = self._tokenizer.decode(ids, skip_special_tokens=False)
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


def _parse_value(raw: str) -> Any:
    try:
        return json.loads(raw)
    except ValueError:
        return raw


def _rehydrate_tool_calls(
    text: str, bindings: dict[str, ToolBinding]
) -> tuple[list[ModelEvent], str]:
    events: list[ModelEvent] = []
    matched = list(_TOOL_CALL.finditer(text))
    for match in matched:
        body = match.group(1)
        name = body.split("<arg_key>", 1)[0].strip()
        binding = bindings.get(name)
        if binding is None:
            raise ResponsesError(
                "unknown_tool_call",
                f"The model selected unknown tool {name!r}.",
                422,
                error_type="model_error",
            )
        pairs = [(key.strip(), value) for key, value in _ARGUMENT.findall(body)]
        arguments = {key: _parse_value(value) for key, value in pairs}
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
            events.append(
                ModelEvent(
                    kind="custom_tool_call",
                    name=binding.original_name,
                    namespace=binding.namespace,
                    call_id=call_id,
                    input=raw,
                )
            )
        else:
            event_kind = {
                "tool_search": "tool_search_call",
                "shell": "shell_call",
                "local_shell": "shell_call",
                "apply_patch": "apply_patch_call",
                "mcp": "mcp_call",
            }.get(binding.kind, "function_call")
            events.append(
                ModelEvent(
                    kind=event_kind,
                    name=binding.original_name,
                    namespace=binding.namespace,
                    call_id=call_id,
                    arguments=serialized,
                )
            )
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
        self._generation_phases: dict[str, str] = {}
        self._last_used_at = time.time()

    async def capabilities(self, model: str) -> ModelCapabilities:
        return self._capabilities

    async def _ensure_loaded(self) -> None:
        if self._model is not None:
            return
        async with self._load_lock:
            if self._model is not None:
                return
            try:
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
            if not self.model_path.exists():
                raise ResponsesError(
                    "model_not_found",
                    f"MLX model path does not exist: {self.model_path}",
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
                )

            loop = asyncio.get_running_loop()
            self._model, self._tokenizer = await loop.run_in_executor(
                self._executor, load_model
            )

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
        return compile_turn(
            request_for_prompt,
            context_items,
            generation_id,
            prompt_builder=lambda messages, **kwargs: self._tokenizer.apply_chat_template(
                messages,
                tokenize=False,
                **kwargs,
            ),
        )

    async def count_tokens(self, turn: CompiledTurn) -> TokenUsageEstimate:
        if isinstance(turn.prompt, list):
            return TokenUsageEstimate(len(turn.prompt))
        if isinstance(turn.prompt, str):
            encoded = await asyncio.to_thread(
                self._tokenizer.encode, turn.prompt, add_special_tokens=False
            )
            return TokenUsageEstimate(len(encoded))
        return TokenUsageEstimate(0)

    async def stream(self, turn: CompiledTurn) -> AsyncIterator[ModelEvent]:
        await self._ensure_loaded()
        try:
            custom_grammar_processor = _ActivatedCustomGrammarProcessor(
                self._tokenizer, turn.bindings
            )
        except ResponsesError:
            raise
        except (ImportError, RuntimeError, ValueError) as exc:
            raise ResponsesError(
                "invalid_custom_tool_grammar",
                f"Could not compile the custom tool grammar: {exc}",
                400,
                error_type="invalid_request_error",
            ) from exc
        async with self._admission_lock:
            if self._inflight_generations >= self._max_inflight_generations:
                raise ResponsesError(
                    "model_queue_saturated",
                    "The local MLX generation queue is full; retry after an active response completes.",
                    429,
                    error_type="server_error",
                )
            self._inflight_generations += 1
            self._generation_phases[turn.generation_id] = "queued"
        try:
            await self._generation_slot.acquire()
        except BaseException:
            async with self._admission_lock:
                self._inflight_generations -= 1
                self._generation_phases.pop(turn.generation_id, None)
            raise
        async with self._admission_lock:
            self._generation_phases[turn.generation_id] = "counting_tokens"
        # Hugging Face's fast tokenizer is not re-entrant. Count before the
        # generation worker starts using the shared processor.
        try:
            input_usage = await self.count_tokens(turn)
        except BaseException:
            self._generation_slot.release()
            async with self._admission_lock:
                self._inflight_generations -= 1
                self._generation_phases.pop(turn.generation_id, None)
            raise
        loop = asyncio.get_running_loop()
        queue: asyncio.Queue[tuple[str, Any]] = asyncio.Queue()
        cancel_flag = threading.Event()
        self._cancel_flags[turn.generation_id] = cancel_flag
        async with self._admission_lock:
            self._generation_phases[turn.generation_id] = "generating"

        def worker() -> None:
            output_tokens = 0
            try:
                from mlx_vlm import stream_generate
                from mlx_vlm.sample_utils import make_sampler

                sampler = make_sampler(temp=turn.temperature, top_p=turn.top_p)
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
                cache_key = turn.request.get("prompt_cache_key")
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
                    if response.text:
                        loop.call_soon_threadsafe(
                            queue.put_nowait, ("chunk", response.text)
                        )
                    output_tokens = max(output_tokens, int(response.generation_tokens or 0))
                    prompt_tokens = max(prompt_tokens, int(response.prompt_tokens or 0))
                    cached_tokens = max(cached_tokens, int(response.cached_tokens or 0))
                    final_finish_reason = response.finish_reason or final_finish_reason
                loop.call_soon_threadsafe(
                    queue.put_nowait,
                    (
                        "done",
                        (
                            output_tokens,
                            prompt_tokens,
                            cached_tokens,
                            final_finish_reason,
                            cancel_flag.is_set(),
                        ),
                    ),
                )
            except BaseException as exc:
                loop.call_soon_threadsafe(queue.put_nowait, ("error", exc))

        worker_future = loop.run_in_executor(self._executor, worker)
        raw_chunks: list[str] = []
        pending = ""
        reasoning_text = ""
        reasoning_complete = bool(turn.structured_format)
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
                    chunk = str(payload)
                    raw_chunks.append(chunk)
                    # Tool-bearing turns stay buffered until their closing
                    # marker can be validated and rehydrated. Text-only turns
                    # begin emitting semantic deltas as soon as hidden Laguna
                    # reasoning has closed.
                    if turn.bindings:
                        continue
                    pending += chunk
                    if not reasoning_complete:
                        if "</think>" not in pending:
                            continue
                        reasoning_text, pending = _split_reasoning(pending)
                        reasoning_complete = True
                        if reasoning_text:
                            yield ModelEvent(kind="reasoning_delta", delta=reasoning_text)
                    if pending:
                        yield ModelEvent(kind="text_delta", delta=pending)
                        pending = ""
                    continue
                if kind == "done":
                    (
                        output_tokens,
                        prompt_tokens,
                        cached_tokens,
                        backend_finish,
                        cancelled,
                    ) = payload
                    break
            await worker_future
            if cancelled:
                yield ModelEvent(kind="finish", finish_reason="cancelled")
                return
            if turn.bindings:
                reasoning_text, answer = _split_reasoning("".join(raw_chunks))
                if reasoning_text:
                    yield ModelEvent(kind="reasoning_delta", delta=reasoning_text)
                tool_events, remainder = _rehydrate_tool_calls(answer, turn.bindings)
                for event in tool_events:
                    yield event
                if remainder:
                    yield ModelEvent(kind="text_delta", delta=remainder)
            elif not reasoning_complete:
                reasoning_text, answer = _split_reasoning(pending)
                if reasoning_text:
                    yield ModelEvent(kind="reasoning_delta", delta=reasoning_text)
                if answer:
                    yield ModelEvent(kind="text_delta", delta=answer)
            elif pending:
                yield ModelEvent(kind="text_delta", delta=pending)
            yield ModelEvent(
                kind="usage",
                input_tokens=prompt_tokens or input_usage.input_tokens,
                output_tokens=output_tokens,
                reasoning_tokens=max(0, len(reasoning_text) // 4),
                metadata={"cached_tokens": cached_tokens},
            )
            finish_reason = backend_finish or (
                "length" if output_tokens >= turn.max_output_tokens else "stop"
            )
            yield ModelEvent(kind="finish", finish_reason=finish_reason)
        finally:
            cancel_flag.set()
            await asyncio.gather(worker_future, return_exceptions=True)
            self._cancel_flags.pop(turn.generation_id, None)
            self._generation_slot.release()
            async with self._admission_lock:
                self._inflight_generations -= 1
                self._generation_phases.pop(turn.generation_id, None)
            self._last_used_at = time.time()

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
        phases = dict(self._generation_phases)
        return {
            "loaded": self._model is not None,
            "inflight_generations": self._inflight_generations,
            "max_inflight_generations": self._max_inflight_generations,
            "generation_slot_available": not self._generation_slot.locked(),
            "queued_generations": sum(phase == "queued" for phase in phases.values()),
            "generation_phases": phases,
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
