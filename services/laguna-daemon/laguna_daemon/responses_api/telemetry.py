from __future__ import annotations

import time
from collections import deque
from dataclasses import dataclass, field
from typing import Any


# A sub-10ms wall-clock span frequently means a batched callback reported
# several tokens together. It is not a meaningful per-token sample.
_MIN_DECODE_INTERVAL_SECONDS = 0.01

#: Prompt-size buckets for the prefill histogram, aligned with the benchmark
#: buckets so real workloads can be read against measured baselines.
PREFILL_BUCKETS: tuple[tuple[str, int], ...] = (
    ("<=1k", 1_000),
    ("<=5k", 5_000),
    ("<=10k", 10_000),
    ("<=25k", 25_000),
    ("<=50k", 50_000),
    ("<=150k", 150_000),
)
PREFILL_OVERFLOW_BUCKET = ">150k"


def prefill_bucket(prompt_tokens: int) -> str:
    for label, bound in PREFILL_BUCKETS:
        if prompt_tokens <= bound:
            return label
    return PREFILL_OVERFLOW_BUCKET


def percentile(values: list[float], fraction: float) -> float | None:
    """Nearest-rank percentile. Returns None rather than inventing a value."""
    if not values:
        return None
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, round(fraction * (len(ordered) - 1))))
    return round(ordered[index], 3)


@dataclass(slots=True)
class GenerationTiming:
    """Real timestamps for one generation, taken where the events happen.

    Every field is a monotonic timestamp or a counter the backend actually
    observed. Nothing here is estimated: a metric that was never measured stays
    None so the surface can say "Unavailable" instead of showing a plausible
    number that is not true.
    """

    generation_id: str
    queued_at: float
    admitted_at: float | None = None
    compiled_at: float | None = None
    first_token_at: float | None = None
    last_token_at: float | None = None
    completed_at: float | None = None
    prompt_tokens: int = 0
    cached_tokens: int = 0
    output_tokens: int = 0
    measured_decode_tps: float | None = None
    phase: str = "queued"

    def ttft_ms(self) -> float | None:
        if self.first_token_at is None:
            return None
        return round((self.first_token_at - self.queued_at) * 1000, 3)

    def prefill_tokens_per_second(self) -> float | None:
        start = self.admitted_at
        if start is None or self.first_token_at is None or not self.prompt_tokens:
            return None
        elapsed = self.first_token_at - start
        if elapsed <= 0:
            return None
        # Cached prefix tokens are not recomputed, so counting them would
        # overstate throughput on a warm cache.
        computed = max(0, self.prompt_tokens - self.cached_tokens)
        if not computed:
            return None
        return round(computed / elapsed, 3)

    def decode_tokens_per_second(self) -> float | None:
        if self.measured_decode_tps is not None and self.measured_decode_tps > 0:
            return round(self.measured_decode_tps, 3)
        if self.first_token_at is None or self.output_tokens <= 1:
            return None
        end = self.last_token_at or time.monotonic()
        elapsed = end - self.first_token_at
        if elapsed < _MIN_DECODE_INTERVAL_SECONDS:
            return None
        # The first token's cost belongs to prefill, not decode.
        return round((self.output_tokens - 1) / elapsed, 3)

    def record_decode_progress(
        self,
        *,
        sampled_at: float,
        output_tokens: int,
        prompt_tokens: int,
        cached_tokens: int,
        measured_decode_tps: float | None,
    ) -> None:
        """Record a source-side generation update, whether or not it has text.

        Token sources sometimes emit a generation update before they can decode
        a displayable text delta (notably structured/tool turns). Those updates
        still establish real throughput, so the inference monitor must not tie
        its timing to presentation text.
        """
        output_tokens = max(0, int(output_tokens))
        self.prompt_tokens = max(self.prompt_tokens, int(prompt_tokens))
        self.cached_tokens = max(self.cached_tokens, int(cached_tokens))
        if measured_decode_tps is not None and measured_decode_tps > 0:
            self.measured_decode_tps = measured_decode_tps

        if output_tokens > self.output_tokens:
            self.output_tokens = output_tokens
            if self.first_token_at is None:
                self.first_token_at = sampled_at
                self.phase = "decode"
            self.last_token_at = sampled_at

    def cache_hit_ratio(self) -> float:
        if not self.prompt_tokens:
            return 0.0
        return round(self.cached_tokens / self.prompt_tokens, 4)

    def elapsed_ms(self, now: float | None = None) -> float:
        end = self.completed_at or now or time.monotonic()
        return round((end - self.queued_at) * 1000, 3)


@dataclass(slots=True)
class InferenceTelemetry:
    """Bounded rolling aggregates across every wire surface.

    Both Responses and Chat run on the same `TurnRunner`, so both feed this.
    Aggregates are in-memory only and reset when the daemon restarts; the
    snapshot says so explicitly rather than implying lifetime totals.
    """

    window: int = 256
    requests_completed: int = 0
    requests_failed: int = 0
    requests_cancelled: int = 0
    input_tokens: int = 0
    output_tokens: int = 0
    cached_tokens: int = 0
    started_at: float = field(default_factory=time.time)
    _ttft_ms: deque[float] = field(default_factory=lambda: deque(maxlen=256))
    _decode_tps: deque[float] = field(default_factory=lambda: deque(maxlen=256))
    _latency_ms: deque[float] = field(default_factory=lambda: deque(maxlen=256))
    #: Per-generation prefill facts (prompt_tokens, cached_tokens, ttft_ms,
    #: prefill_tps) over the same rolling window as the percentile series.
    _prefill_samples: deque[tuple[int, int, float | None, float | None]] = field(
        default_factory=lambda: deque(maxlen=256)
    )

    @staticmethod
    def _prefill_tps(timing: GenerationTiming) -> float | None:
        """Computed prefill throughput over the real compile→first-token span.

        Distinct from `GenerationTiming.prefill_tokens_per_second`, which
        measures from admission: the histogram calibrates prefill itself, so
        compilation time is excluded. Unmeasurable stays None — never 0.
        """
        if timing.compiled_at is None or timing.first_token_at is None:
            return None
        computed = max(0, timing.prompt_tokens - timing.cached_tokens)
        elapsed = timing.first_token_at - timing.compiled_at
        if not computed or elapsed <= 0:
            return None
        return round(computed / elapsed, 3)

    def record_completed(self, timing: GenerationTiming | None, latency_ms: float) -> None:
        self.requests_completed += 1
        self._latency_ms.append(latency_ms)
        if timing is None:
            return
        self.input_tokens += timing.prompt_tokens
        self.output_tokens += timing.output_tokens
        self.cached_tokens += timing.cached_tokens
        ttft = timing.ttft_ms()
        if ttft is not None:
            self._ttft_ms.append(ttft)
        decode = timing.decode_tokens_per_second()
        if decode is not None:
            self._decode_tps.append(decode)
        self._prefill_samples.append(
            (
                timing.prompt_tokens,
                timing.cached_tokens,
                ttft,
                self._prefill_tps(timing),
            )
        )

    def prefill_histogram(self) -> dict[str, dict[str, Any]]:
        """Rolling prompt-size buckets; every bucket is always present.

        Deliberately not part of `snapshot()`: the legacy inference payload's
        field set is pinned, so this is served only by the control surface.
        """
        labels = [label for label, _ in PREFILL_BUCKETS] + [PREFILL_OVERFLOW_BUCKET]
        grouped: dict[str, list[tuple[int, int, float | None, float | None]]] = {
            label: [] for label in labels
        }
        for sample in self._prefill_samples:
            grouped[prefill_bucket(sample[0])].append(sample)
        histogram: dict[str, dict[str, Any]] = {}
        for label in labels:
            samples = grouped[label]
            prompt_sum = sum(sample[0] for sample in samples)
            cached_sum = sum(sample[1] for sample in samples)
            ttfts = [sample[2] for sample in samples if sample[2] is not None]
            rates = [sample[3] for sample in samples if sample[3] is not None]
            histogram[label] = {
                "count": len(samples),
                "cached_token_share": (
                    round(cached_sum / prompt_sum, 4) if prompt_sum else None
                ),
                "ttft_p50_ms": percentile(ttfts, 0.50),
                "prefill_tps_p50": percentile(rates, 0.50),
            }
        return histogram

    def record_failed(self) -> None:
        self.requests_failed += 1

    def record_cancelled(self) -> None:
        self.requests_cancelled += 1

    def snapshot(self) -> dict[str, Any]:
        ttft = list(self._ttft_ms)
        decode = list(self._decode_tps)
        latency = list(self._latency_ms)
        return {
            "requestsCompleted": self.requests_completed,
            "requestsFailed": self.requests_failed,
            "requestsCancelled": self.requests_cancelled,
            "inputTokens": self.input_tokens,
            "outputTokens": self.output_tokens,
            "cachedTokens": self.cached_tokens,
            "ttftP50Ms": percentile(ttft, 0.50),
            "ttftP95Ms": percentile(ttft, 0.95),
            "decodeTpsP50": percentile(decode, 0.50),
            "decodeTpsP95": percentile(decode, 0.95),
            "latencyP50Ms": percentile(latency, 0.50),
            "latencyP95Ms": percentile(latency, 0.95),
            # These are process-lifetime aggregates, not durable history.
            "resetsOnRestart": True,
            "windowSize": self.window,
        }
