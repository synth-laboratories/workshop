from __future__ import annotations

"""Local GGUF backend: llama.cpp weights behind the same turn core as MLX.

This is the peer of `NativeMlxBackend`, not a proxy. Both wire surfaces
(`/v1/responses` and `/v1/chat/completions`) compile onto the neutral
`CompiledTurn`, are admitted through one generation slot, are cancelled through
one registry, and are accounted for with one set of measured counters. The only
thing that differs from MLX is where the weights live: a GGUF runtime cannot be
loaded in this process, so generation is carried out over loopback by an engine
the Desktop supervisor owns.

That transport is deliberately not the `external` passthrough. `RemoteResponsesBackend`
forwards a Responses body upstream and 501s Chat; a llama.cpp engine has no
Responses surface at all and speaks only Chat Completions, so the passthrough
could serve neither surface. Here the engine is a *token source*: it never sees
a Responses object, never decides an item's type, and never terminates a turn —
Laguna synthesizes Responses from `ModelEvent`s exactly as it does for MLX.

The daemon never starts, restarts, discovers, or supervises that process. It
receives one address and reports honestly on what it finds there.
"""

import asyncio
import json
import time
from collections import OrderedDict
from contextlib import aclosing
from typing import Any, AsyncIterator

import httpx

from ..capabilities import ModelCapabilities
from ..compiler import compile_messages, compile_turn
from ..errors import ResponsesError
from ..telemetry import GenerationTiming
from .protocol import CompiledTurn, ModelEvent, TokenUsageEstimate, ToolBinding
from .reasoning import _IncrementalReasoningSplitter
from .tool_events import resolve_binding, tool_call_event


#: How long a probe of the engine's own health endpoint may take. The engine
#: answers this from its main loop, so a slow answer is itself a signal.
_PROBE_TIMEOUT_SECONDS = 3.0
#: Longest gap between streamed chunks before the transport is considered dead.
#: Generous: the first chunk of a large prompt arrives only after prefill.
_STREAM_READ_TIMEOUT_SECONDS = 600.0
#: How long a caller waits for weights that are still being mapped in. A 17 GB
#: K-quant takes tens of seconds from cold page cache.
_READY_TIMEOUT_SECONDS = 300.0
#: Freshness of the cached engine snapshot that `/health` reads. Desktop polls
#: health about once a second; this keeps that from becoming a probe per poll
#: while still failing closed within a poll or two of the engine dying.
_STATUS_CACHE_SECONDS = 1.0

_FINISH_REASONS = {
    "stop": "stop",
    "length": "length",
    "tool_calls": "tool_call",
}


def _wire_messages(messages: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Lower the compiler's message form onto the OpenAI Chat wire form.

    The compiler carries tool-call arguments decoded, because that is what a
    prompt template renders from; the wire carries them as a JSON string.
    `reasoning_content` is passed through under the same name the engine emits,
    which is what lets a multi-step turn keep its earlier thinking instead of
    silently dropping it at the first tool boundary.
    """
    wire: list[dict[str, Any]] = []
    for message in messages:
        entry: dict[str, Any] = {
            "role": message.get("role"),
            "content": message.get("content") or "",
        }
        if message.get("tool_call_id"):
            entry["tool_call_id"] = message["tool_call_id"]
        reasoning = message.get("reasoning_content")
        if isinstance(reasoning, str) and reasoning:
            entry["reasoning_content"] = reasoning
        tool_calls = message.get("tool_calls")
        if isinstance(tool_calls, list) and tool_calls:
            entry["tool_calls"] = [
                {
                    "id": call.get("id"),
                    "type": "function",
                    "function": {
                        "name": (call.get("function") or {}).get("name"),
                        "arguments": _wire_arguments(
                            (call.get("function") or {}).get("arguments")
                        ),
                    },
                }
                for call in tool_calls
                if isinstance(call, dict)
            ]
        wire.append(entry)
    return wire


def _wire_arguments(arguments: Any) -> str:
    if isinstance(arguments, str):
        return arguments
    return json.dumps(arguments or {}, ensure_ascii=False, separators=(",", ":"))


def _response_format(structured: dict[str, Any] | None) -> dict[str, Any] | None:
    if not structured:
        return None
    kind = structured.get("type")
    if kind == "json_object":
        return {"type": "json_object"}
    if kind == "json_schema":
        schema = structured.get("schema")
        return {
            "type": "json_schema",
            "json_schema": {
                "name": "response",
                "schema": schema if isinstance(schema, dict) else {"type": "object"},
                "strict": True,
            },
        }
    return None


async def _shutdown(response: httpx.Response, client: httpx.AsyncClient) -> None:
    """Really close one engine connection.

    `response.aclose()` on its own is not enough: httpx hands the socket back to
    its pool, where it stays open and the engine — which stops only when the
    client it is writing to goes away — keeps decoding a turn nobody will read.
    Closing the client that owns the connection is what makes the socket close.
    """
    try:
        await response.aclose()
    except (httpx.HTTPError, RuntimeError):
        pass
    try:
        await client.aclose()
    except (httpx.HTTPError, RuntimeError):
        pass


class _ToolCallAccumulator:
    """Reassemble OpenAI `tool_calls` deltas into complete calls, in order."""

    def __init__(self) -> None:
        self._calls: OrderedDict[int, dict[str, str]] = OrderedDict()

    def feed(self, deltas: list[Any]) -> None:
        for delta in deltas:
            if not isinstance(delta, dict):
                continue
            index = delta.get("index")
            if not isinstance(index, int):
                index = len(self._calls)
            call = self._calls.setdefault(index, {"name": "", "arguments": ""})
            function = delta.get("function") or {}
            name = function.get("name")
            if isinstance(name, str) and name:
                # A name arrives whole in the opening delta on every engine
                # build observed; concatenating guards the streamed-name case
                # without corrupting the common one.
                call["name"] = name if not call["name"] else call["name"] + name
            arguments = function.get("arguments")
            if isinstance(arguments, str):
                call["arguments"] += arguments

    def events(self, bindings: dict[str, ToolBinding]) -> list[ModelEvent]:
        events: list[ModelEvent] = []
        for call in self._calls.values():
            name = call["name"].strip()
            if not name:
                continue
            binding = resolve_binding(name, bindings)
            raw = call["arguments"].strip() or "{}"
            try:
                arguments = json.loads(raw)
            except ValueError as exc:
                raise ResponsesError(
                    "invalid_tool_arguments",
                    f"The model emitted unparseable arguments for tool {name!r}.",
                    422,
                    error_type="model_error",
                ) from exc
            if not isinstance(arguments, dict):
                arguments = {"input": arguments}
            raw_input = arguments.get("input")
            events.append(
                tool_call_event(
                    binding,
                    arguments,
                    raw_input=raw_input if isinstance(raw_input, str) else None,
                )
            )
        return events

    def __bool__(self) -> bool:
        return bool(self._calls)


class LlamaCppChatBackend:
    """`ModelBackend` over a supervisor-owned llama.cpp engine."""

    def __init__(
        self,
        *,
        engine_url: str,
        model: str,
        context_length: int,
        api_key: str | None = None,
        ready_timeout_seconds: float = _READY_TIMEOUT_SECONDS,
        transport: Any = None,
    ) -> None:
        self.engine_url = engine_url.rstrip("/")
        self.model = model
        self.api_key = api_key
        self._capabilities = ModelCapabilities(
            context_length=context_length,
            # The engine holds an mmproj and could accept image parts, but no
            # wire surface here lowers one to it: the compiler keeps text
            # parts only. Advertising vision before that path exists would be
            # a capability bit that does not match behavior.
            images=False,
            files=False,
            video=False,
            audio=False,
            hosted_mcp=False,
            hosted_web_search=False,
        )
        self._ready_timeout_seconds = ready_timeout_seconds
        self._transport = transport
        self._client: httpx.AsyncClient | None = None
        self._client_lock = asyncio.Lock()
        self._ready_lock = asyncio.Lock()
        self._ready = False
        self._loading = False
        self._last_error: str | None = None
        self._status: dict[str, Any] = {
            "reachable": False,
            "ready": False,
            "state": "unknown",
            "detail": "The Muse engine has not been probed yet.",
        }
        self._status_checked_at = 0.0
        # One engine slot: the supervisor starts llama.cpp without `--parallel`,
        # so a second concurrent generation would contend for the same KV cache
        # rather than run beside it. Queue the rest, bounded, exactly as the MLX
        # backend does.
        self._generation_slot = asyncio.Semaphore(1)
        self._admission_lock = asyncio.Lock()
        self._inflight_generations = 0
        self._max_inflight_generations = 9
        self._generations: dict[str, GenerationTiming] = {}
        self._recent_generations: OrderedDict[str, GenerationTiming] = OrderedDict()
        self._max_recent_generations = 32
        self._cancel_flags: dict[str, asyncio.Event] = {}
        # Live engine connections by generation. Closing one is the only signal
        # llama.cpp has that a turn is no longer wanted, so each generation owns
        # its own client: closing a *response* alone releases the socket back to
        # the pool still open, and the engine keeps decoding into it.
        self._open_streams: dict[str, tuple[httpx.Response, httpx.AsyncClient]] = {}
        self._last_used_at = time.time()

    # -- transport -------------------------------------------------------------

    def _new_client(self) -> httpx.AsyncClient:
        return httpx.AsyncClient(
            base_url=self.engine_url,
            headers=(
                {"Authorization": f"Bearer {self.api_key}"} if self.api_key else None
            ),
            timeout=httpx.Timeout(
                connect=5.0,
                read=_STREAM_READ_TIMEOUT_SECONDS,
                write=30.0,
                pool=30.0,
            ),
            transport=self._transport,
        )

    async def _http(self) -> httpx.AsyncClient:
        """Pooled client for short control calls: health, template, tokenize."""
        if self._client is None:
            async with self._client_lock:
                if self._client is None:
                    self._client = self._new_client()
        return self._client

    def _unavailable(self, detail: str) -> ResponsesError:
        return ResponsesError(
            "muse_engine_unavailable",
            detail,
            503,
            error_type="server_error",
        )

    # -- engine readiness ------------------------------------------------------

    async def _probe(self) -> dict[str, Any]:
        """One cheap, truthful look at the engine."""
        client = await self._http()
        try:
            response = await client.get("/health", timeout=_PROBE_TIMEOUT_SECONDS)
        except httpx.HTTPError as exc:
            return {
                "reachable": False,
                "ready": False,
                "state": "unreachable",
                "detail": (
                    f"No engine is answering at {self.engine_url} ({type(exc).__name__}). "
                    "Synth Desktop starts it when Muse Glimmer is the selected model."
                ),
            }
        if response.status_code == 503:
            return {
                "reachable": True,
                "ready": False,
                "state": "loading",
                "detail": "The engine is mapping Muse Glimmer's weights into memory.",
            }
        if response.status_code >= 400:
            return {
                "reachable": True,
                "ready": False,
                "state": "error",
                "detail": (
                    f"The engine answered {response.status_code} on /health: "
                    f"{response.text[:200]}"
                ),
            }
        return {
            "reachable": True,
            "ready": True,
            "state": "ready",
            "detail": "The Muse engine is serving.",
        }

    async def engine_status(self) -> dict[str, Any]:
        """Cached engine snapshot for the health and control surfaces.

        Health is polled about once a second by Desktop; this bounds the probe
        rate without letting the report go stale enough to keep advertising a
        working surface after the engine has died.
        """
        now = time.monotonic()
        if now - self._status_checked_at < _STATUS_CACHE_SECONDS:
            return dict(self._status)
        status = await self._probe()
        self._status = status
        self._status_checked_at = now
        self._ready = bool(status["ready"])
        if not status["ready"]:
            self._last_error = None if status["state"] == "loading" else status["detail"]
        return dict(status)

    async def _ensure_ready(self) -> None:
        """Block until the engine can generate, or fail with a typed error.

        This is the residency gate. It is what makes `loading` a visible phase
        rather than a request that mysteriously takes a minute, and what makes a
        missing engine a 503 that names the cause instead of a connection error
        surfacing from inside a stream.
        """
        if self._ready:
            return
        async with self._ready_lock:
            if self._ready:
                return
            self._loading = True
            try:
                loop = asyncio.get_running_loop()
                deadline = loop.time() + self._ready_timeout_seconds
                while True:
                    status = await self._probe()
                    self._status = status
                    self._status_checked_at = time.monotonic()
                    if status["ready"]:
                        self._ready = True
                        self._last_error = None
                        return
                    if not status["reachable"] or loop.time() >= deadline:
                        self._last_error = status["detail"]
                        raise self._unavailable(status["detail"])
                    await asyncio.sleep(0.5)
            finally:
                self._loading = False

    async def load(self) -> None:
        """Explicit residency request; loading counts as use."""
        await self._ensure_ready()
        self._last_used_at = time.time()

    # -- compilation -----------------------------------------------------------

    async def capabilities(self, model: str) -> ModelCapabilities:
        return self._capabilities

    async def compile(
        self,
        request: dict[str, Any],
        context_items: list[dict[str, Any]],
        generation_id: str,
    ) -> CompiledTurn:
        # No prompt builder: the engine owns the model's chat template, so the
        # neutral turn stops at messages. Handing it a second, locally rendered
        # prompt would produce a prompt the weights were not trained on.
        self._last_used_at = time.time()
        return compile_turn(
            request,
            context_items,
            generation_id,
            defaults=getattr(self, "sampling_defaults", None),
        )

    async def compile_messages(self, **kwargs: Any) -> CompiledTurn:
        self._last_used_at = time.time()
        return compile_messages(**kwargs)

    async def count_tokens(self, turn: CompiledTurn) -> TokenUsageEstimate:
        """Count with the engine's own tokenizer; never estimate.

        Applying the template first is what makes the number comparable to the
        context limit the same engine enforces. When a build has no template
        endpoint the fallback still counts real tokens, over the message text
        alone — short of the true prompt by its template scaffolding, and
        reported as such rather than padded with a guess.
        """
        await self._ensure_ready()
        prompt = await self._apply_template(turn)
        if prompt is None:
            prompt = "\n".join(
                str(message.get("content") or "") for message in turn.messages
            )
        return TokenUsageEstimate(await self._tokenize(prompt))

    async def _apply_template(self, turn: CompiledTurn) -> str | None:
        client = await self._http()
        body: dict[str, Any] = {"messages": _wire_messages(turn.messages)}
        if turn.tools:
            body["tools"] = turn.tools
        try:
            response = await client.post("/apply-template", json=body, timeout=30.0)
        except httpx.HTTPError as exc:
            raise self._unavailable(
                f"The engine became unreachable while compiling a prompt: {exc}"
            ) from exc
        if response.status_code in {404, 405, 501}:
            return None
        if response.status_code >= 400:
            return None
        prompt = response.json().get("prompt")
        return prompt if isinstance(prompt, str) else None

    async def _tokenize(self, prompt: str) -> int:
        client = await self._http()
        try:
            response = await client.post(
                "/tokenize", json={"content": prompt}, timeout=60.0
            )
            response.raise_for_status()
        except httpx.HTTPError as exc:
            raise self._unavailable(
                f"The engine could not tokenize the prompt: {exc}"
            ) from exc
        tokens = response.json().get("tokens")
        return len(tokens) if isinstance(tokens, list) else 0

    # -- generation ------------------------------------------------------------

    def _request_body(self, turn: CompiledTurn) -> dict[str, Any]:
        body: dict[str, Any] = {
            "model": self.model,
            "messages": _wire_messages(turn.messages),
            "stream": True,
            # Without this the engine ends the stream with no usage object and
            # the turn's token counts would have to be invented.
            "stream_options": {"include_usage": True},
            "max_tokens": turn.max_output_tokens,
            "temperature": turn.temperature,
            "top_p": turn.top_p,
            # The template decides what a thinking span looks like; the request
            # only says whether this turn has one.
            "chat_template_kwargs": {"enable_thinking": turn.enable_thinking},
        }
        if turn.top_k:
            body["top_k"] = turn.top_k
        if turn.tools:
            body["tools"] = turn.tools
            body["tool_choice"] = "auto"
        response_format = _response_format(turn.structured_format)
        if response_format is not None:
            body["response_format"] = response_format
        return body

    async def stream(self, turn: CompiledTurn) -> AsyncIterator[ModelEvent]:
        await self._ensure_ready()
        async with self._admission_lock:
            if self._inflight_generations >= self._max_inflight_generations:
                raise ResponsesError(
                    "model_queue_saturated",
                    "The local Muse generation queue is full; retry after an "
                    "active response completes.",
                    429,
                    error_type="server_error",
                )
            self._inflight_generations += 1
            timing = GenerationTiming(
                generation_id=turn.generation_id, queued_at=time.monotonic()
            )
            self._generations[turn.generation_id] = timing
        cancel = asyncio.Event()
        self._cancel_flags[turn.generation_id] = cancel
        try:
            await self._generation_slot.acquire()
        except BaseException:
            self._retire(turn.generation_id, timing, release_slot=False)
            raise
        try:
            timing.admitted_at = time.monotonic()
            timing.phase = "prefill"
            # `aclosing` is load-bearing, not tidiness. Closing this generator
            # does not close the one it is iterating: Python leaves that to
            # async-generator finalization, which runs whenever the object is
            # collected — and a traceback or a live reference can hold it for
            # the life of the process. Meanwhile the engine keeps decoding into
            # a socket that was never closed. Close it here, deterministically.
            async with aclosing(self._generate(turn, timing, cancel)) as events:
                async for event in events:
                    yield event
        finally:
            self._retire(turn.generation_id, timing)

    async def _generate(
        self, turn: CompiledTurn, timing: GenerationTiming, cancel: asyncio.Event
    ) -> AsyncIterator[ModelEvent]:
        # A generation gets its own connection. See `_open_streams`.
        client = self._new_client()
        splitter = _IncrementalReasoningSplitter(thinking_open=False)
        accumulator = _ToolCallAccumulator()
        finish_reason: str | None = None
        usage: dict[str, Any] | None = None
        saw_reasoning_field = False
        stream = client.stream("POST", "/v1/chat/completions", json=self._request_body(turn))
        try:
            response = await stream.__aenter__()
        except httpx.HTTPError as exc:
            # The connection this generation owns is not in the registry yet, so
            # nothing else will close it.
            await client.aclose()
            raise self._unavailable(
                f"The Muse engine refused the connection: {exc}"
            ) from exc
        self._open_streams[turn.generation_id] = (response, client)
        try:
            if response.status_code >= 400:
                await response.aread()
                raise self._engine_error(response)
            async for line in response.aiter_lines():
                if cancel.is_set():
                    yield ModelEvent(kind="finish", finish_reason="cancelled")
                    return
                if not line.startswith("data:"):
                    continue
                payload = line[5:].strip()
                if not payload or payload == "[DONE]":
                    continue
                try:
                    chunk = json.loads(payload)
                except ValueError:
                    continue
                if isinstance(chunk.get("usage"), dict):
                    usage = chunk["usage"]
                if isinstance(chunk.get("error"), dict):
                    raise ResponsesError(
                        "muse_engine_error",
                        str(chunk["error"].get("message") or "The engine failed."),
                        502,
                        error_type="server_error",
                    )
                choices = chunk.get("choices") or []
                if not choices:
                    continue
                choice = choices[0]
                delta = choice.get("delta") or {}
                reasoning = delta.get("reasoning_content")
                if isinstance(reasoning, str) and reasoning:
                    saw_reasoning_field = True
                    self._mark_token(timing)
                    yield ModelEvent(kind="reasoning_delta", delta=reasoning)
                content = delta.get("content")
                if isinstance(content, str) and content:
                    self._mark_token(timing)
                    if saw_reasoning_field:
                        # The engine is already splitting thinking out, so
                        # content is answer text by construction.
                        yield ModelEvent(kind="text_delta", delta=content)
                    else:
                        for mode, text in splitter.feed(content):
                            yield ModelEvent(
                                kind="reasoning_delta"
                                if mode == "reasoning"
                                else "text_delta",
                                delta=text,
                            )
                tool_deltas = delta.get("tool_calls")
                if isinstance(tool_deltas, list) and tool_deltas:
                    self._mark_token(timing)
                    accumulator.feed(tool_deltas)
                if choice.get("finish_reason"):
                    finish_reason = str(choice["finish_reason"])
        except ResponsesError:
            raise
        except httpx.HTTPError as exc:
            if cancel.is_set():
                # `cancel()` closed this response out from under the reader.
                # That is the mechanism working, not a transport failure.
                yield ModelEvent(kind="finish", finish_reason="cancelled")
                return
            raise self._unavailable(
                f"The Muse engine stopped answering mid-turn: {exc}"
            ) from exc
        finally:
            self._open_streams.pop(turn.generation_id, None)
            await self._close_connection(response, client)
        if not saw_reasoning_field:
            for mode, text in splitter.flush():
                yield ModelEvent(
                    kind="reasoning_delta" if mode == "reasoning" else "text_delta",
                    delta=text,
                )
        for event in accumulator.events(turn.bindings):
            yield event
        yield self._usage_event(usage, timing)
        resolved = _FINISH_REASONS.get(finish_reason or "stop", "stop")
        if accumulator and resolved == "stop":
            resolved = "tool_call"
        yield ModelEvent(kind="finish", finish_reason=resolved)

    def _engine_error(self, response: httpx.Response) -> ResponsesError:
        detail = response.text[:500]
        try:
            body = response.json()
            detail = str((body.get("error") or {}).get("message") or detail)
        except ValueError:
            pass
        if response.status_code == 404:
            return self._unavailable(
                "The process on the engine port has no Chat Completions surface. "
                "Stop it and let Synth Desktop start the managed Muse engine."
            )
        return ResponsesError(
            "muse_engine_error",
            f"The Muse engine rejected the turn ({response.status_code}): {detail}",
            502,
            error_type="server_error",
        )

    def _usage_event(
        self, usage: dict[str, Any] | None, timing: GenerationTiming
    ) -> ModelEvent:
        """Token counts as the engine reported them, or zeros it can defend.

        Nothing here is derived from chunk counts: a delta is not a token, and
        a plausible number in a usage field is worse than an absent one.
        """
        usage = usage or {}
        input_tokens = int(usage.get("prompt_tokens") or 0)
        output_tokens = int(usage.get("completion_tokens") or 0)
        cached = int((usage.get("prompt_tokens_details") or {}).get("cached_tokens") or 0)
        timing.prompt_tokens = input_tokens
        timing.output_tokens = output_tokens
        timing.cached_tokens = cached
        return ModelEvent(
            kind="usage",
            input_tokens=input_tokens,
            output_tokens=output_tokens,
            # The engine reports no split between reasoning and answer tokens,
            # and re-tokenizing the reasoning text here would be a second
            # tokenizer's opinion about the first one's output.
            reasoning_tokens=0,
            metadata={"cached_tokens": cached},
        )

    def _mark_token(self, timing: GenerationTiming) -> None:
        now = time.monotonic()
        if timing.first_token_at is None:
            timing.first_token_at = now
            timing.phase = "decode"
        timing.last_token_at = now

    def _retire(
        self, generation_id: str, timing: GenerationTiming, *, release_slot: bool = True
    ) -> None:
        self._cancel_flags.pop(generation_id, None)
        # Last line of defence for the engine's slot. This method is synchronous
        # and runs on every exit path, so scheduling the close here cannot be
        # skipped by a cancellation the way an `await` in a cleanup block can.
        # A generation that reached here is over by definition; if its socket is
        # somehow still open, the engine is still generating for nobody.
        entry = self._open_streams.pop(generation_id, None)
        if entry is not None:
            try:
                asyncio.get_running_loop().create_task(_shutdown(*entry))
            except RuntimeError:
                pass
        if release_slot:
            self._generation_slot.release()
        self._inflight_generations -= 1
        timing.completed_at = time.monotonic()
        timing.phase = "complete"
        self._generations.pop(generation_id, None)
        self._recent_generations[generation_id] = timing
        while len(self._recent_generations) > self._max_recent_generations:
            self._recent_generations.popitem(last=False)
        self._last_used_at = time.time()

    async def _close_connection(
        self, response: httpx.Response, client: httpx.AsyncClient
    ) -> None:
        """Close the engine connection even while this task is being cancelled.

        Closing the socket is the whole cancellation mechanism: llama.cpp stops
        decoding when the client it is writing to goes away. On the ordinary
        client-disconnect path this coroutine is *already* cancelled, so a bare
        `await response.aclose()` returns without doing anything and the engine
        keeps generating into a socket nobody reads — a full GPU slot burning
        through max_tokens after the user pressed Stop. Shielding an
        independently scheduled close makes the socket's lifetime survive the
        cancellation that requested it.
        """
        closing = asyncio.ensure_future(_shutdown(response, client))
        try:
            await asyncio.shield(closing)
        except asyncio.CancelledError:
            # The close runs to completion on its own; the cancellation still
            # belongs to the caller.
            raise
        except httpx.HTTPError:
            # A connection that failed on the way down is already gone.
            pass

    async def cancel(self, generation_id: str) -> None:
        flag = self._cancel_flags.get(generation_id)
        if flag is not None:
            flag.set()
        # Setting the flag alone would only take effect at the next chunk, and
        # a cancel that arrives during a long prefill would wait for a token
        # that has not been produced yet. Closing ends it now.
        entry = self._open_streams.pop(generation_id, None)
        if entry is not None:
            await self._close_connection(*entry)

    async def close(self) -> None:
        for flag in self._cancel_flags.values():
            flag.set()
        self._cancel_flags.clear()
        for entry in list(self._open_streams.values()):
            await self._close_connection(*entry)
        self._open_streams.clear()
        if self._client is not None:
            await self._client.aclose()
            self._client = None
        self._ready = False

    # -- residency and telemetry ----------------------------------------------

    def memory_bytes(self) -> int | None:
        """Unmeasurable from here, and therefore null.

        The weights are resident in another process. This daemon has no
        allocator counter for them, and reporting the GGUF's size on disk would
        answer a question about the filesystem under a name that promises a
        fact about memory.
        """
        return None

    def residency(self, idle_unload_after_seconds: int) -> dict[str, Any]:
        last_used_at = int(self._last_used_at * 1000)
        return {
            "loaded": self._ready,
            "idle_seconds": max(0, int(time.time() - self._last_used_at)),
            "last_used_at": last_used_at,
            # No idle deadline exists for weights this daemon cannot free; a
            # timestamp here would promise an eviction that never comes.
            "free_at": None,
        }

    async def unload_if_idle(self, idle_unload_after_seconds: int) -> bool:
        """No-op: the engine stays resident while Muse is the selected model.

        The alternative — releasing on idle — needs a lazy restart on the next
        turn, and the daemon cannot start a process. Releasing without that
        path would trade a warm 17 GB for turns that fail until the user
        reselects the model. Memory comes back when Muse is deselected or
        Synth Desktop quits, both of which the supervisor already handles.
        """
        return False

    async def unload(self) -> bool:
        raise ResponsesError(
            "engine_release_not_supported",
            "Muse Glimmer's weights are resident in the Desktop-supervised "
            "llama.cpp engine, which this daemon does not own and cannot "
            "unload. Select a different model to release them.",
            409,
            error_type="invalid_request_error",
        )

    def diagnostics(self) -> dict[str, Any]:
        phases = {
            generation_id: timing.phase
            for generation_id, timing in self._generations.items()
        }
        return {
            "loaded": self._ready,
            "loading": self._loading,
            "inflight_generations": self._inflight_generations,
            "max_inflight_generations": self._max_inflight_generations,
            "generation_slot_available": not self._generation_slot.locked(),
            "queued_generations": sum(phase == "queued" for phase in phases.values()),
            "generation_phases": phases,
            "engine": {
                "url": self.engine_url,
                "state": self._status["state"],
                "detail": self._status["detail"],
                # The KV cache belongs to the engine's own slots; this daemon
                # has no cache to bound, so the setting does not apply here.
                "prompt_cache": "engine_owned",
                "idle_policy": "resident_while_selected",
            },
            "last_error": self._last_error,
        }

    def generation_metrics(self, generation_id: str) -> GenerationTiming | None:
        timing = self._generations.get(generation_id)
        if timing is not None:
            return timing
        return self._recent_generations.get(generation_id)

    def active_generation(self) -> GenerationTiming | None:
        for timing in self._generations.values():
            if timing.phase != "queued":
                return timing
        return next(iter(self._generations.values()), None)

    def queue_state(self) -> dict[str, int]:
        return {
            "depth": self._inflight_generations,
            "capacity": self._max_inflight_generations,
        }
