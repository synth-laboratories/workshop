from __future__ import annotations

"""Reasoning/answer/tool-envelope classification over streamed model text.

Shared by every local backend: the native MLX backend feeds it raw sampled
text, and the llama.cpp backend feeds it the engine's `content` deltas for
templates that emit `<think>` spans inline instead of splitting them into
`reasoning_content`. Keeping one implementation is what makes "no chain of
thought in assistant text" a single guarantee rather than a per-backend habit.
"""

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
