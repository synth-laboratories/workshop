#!/usr/bin/env python3
"""Long-context benchmark for the managed Muse Glimmer engine.

One invocation = one engine process = one measured configuration. The script
refuses to start while any other model-serving process is alive, spawns exactly
one `llama-server` with production-parity arguments (overridable per flag),
runs the long-context workload, and writes one JSON artifact whose numbers all
come from the engine's own `timings` counters — nothing is synthesized.

    python3 scripts/muse/bench.py --label baseline --out results/baseline.json

Measured, per run:
  - cold engine start to /health ready (model load + warmup)
  - uncached prefill tokens/sec over a >=17K-token prompt (engine `timings`)
  - cached re-ingestion of the identical prompt (engine `cache_n` proves reuse)
  - time to first streamed token, client-observed and engine-reported
  - decode tokens/sec, with the draft's proposed/accepted counters when a
    draft model is loaded (`draft_n` / `draft_n_accepted`)
  - peak resident memory of the engine process (sampled)

The workload prompt is deterministic (seeded), sized against the engine's own
/tokenize endpoint, and never truncated to flatter the numbers.
"""

from __future__ import annotations

import argparse
import atexit
import json
import os
import random
import re
import shlex
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

HOME = Path.home()
DEFAULT_RUNTIME = (
    HOME / ".synth-desktop/muse/runtime/llama-dd1ea524333b1e697489067d7a4c39c60d32beee/llama-server"
)
DEFAULT_MODEL_DIR = HOME / ".synth-desktop/models/meta-models/Muse-Glimmer-30B-GGUF"
MAIN_GGUF = "muse-glimmer-30B-kquant-17gb.gguf"
MMPROJ_GGUF = "mmproj-kquant.gguf"
DFLASH_GGUF = "dflash-kquant.gguf"

# Processes that hold (or can hold) model memory. The benchmark refuses to run
# beside any of them: two engines contending for the GPU corrupts every number.
BLOCKING_PATTERNS = ("llama-server", "laguna_daemon", "laguna-daemon", "mlx_lm", "mlx-serve")

# Memory admission. A resident engine is a machine-wide hazard: loading it when
# the system cannot afford it takes the whole machine down with it, which is
# strictly worse than a benchmark that refuses to run. The footprint number is
# the planning estimate for Muse-30B + draft + mmproj at 131K ctx; the watchdog
# enforces reality against it while the engine lives.
DEFAULT_FOOTPRINT_GB = 26.0
DEFAULT_HEADROOM_GB = 8.0
WATCHDOG_MIN_FREE_PCT = 12
PID_FILE = Path(os.environ.get("TMPDIR", "/tmp")) / "muse-bench-engine.pid"


def system_free_pct() -> int | None:
    out = subprocess.run(["/usr/bin/memory_pressure", "-Q"], capture_output=True, text=True)
    m = re.search(r"free percentage:\s*(\d+)", out.stdout)
    return int(m.group(1)) if m else None


def cpu_speed_limit_pct() -> int | None:
    """macOS thermal throttle indicator (100 = unthrottled).

    Back-to-back 20K-token prefills heat the SoC enough to throttle it; a run
    that starts throttled measures the heatsink, not the configuration. The
    benchmark records this before and after so cross-run comparisons can be
    read honestly.
    """
    out = subprocess.run(["/usr/bin/pmset", "-g", "therm"], capture_output=True, text=True)
    m = re.search(r"CPU_Speed_Limit\s*=\s*(\d+)", out.stdout)
    return int(m.group(1)) if m else None


def swap_used_gb() -> float | None:
    out = subprocess.run(["/usr/sbin/sysctl", "-n", "vm.swapusage"], capture_output=True, text=True)
    m = re.search(r"used\s*=\s*([\d.]+)M", out.stdout)
    return float(m.group(1)) / 1024 if m else None


def total_memory_bytes() -> int:
    out = subprocess.run(["/usr/sbin/sysctl", "-n", "hw.memsize"], capture_output=True, text=True)
    return int(out.stdout.strip())


def reap_stale_bench_engine() -> None:
    """Kill a llama-server left behind by a previous crashed benchmark run."""
    if not PID_FILE.is_file():
        return
    try:
        pid = int(PID_FILE.read_text().strip())
    except ValueError:
        PID_FILE.unlink(missing_ok=True)
        return
    out = subprocess.run(["/bin/ps", "-o", "command=", "-p", str(pid)], capture_output=True, text=True)
    if out.returncode == 0 and "llama-server" in out.stdout:
        log(f"reaping stale benchmark engine pid {pid}")
        os.kill(pid, signal.SIGKILL)
        time.sleep(1)
    PID_FILE.unlink(missing_ok=True)

WORDS = (
    "harbor lattice ember quorum saline drift meridian clause tundra pivot "
    "cinder aperture forage lucent gully mantle prism verge alloy cadence "
    "burrow estuary fathom garnet hollow inlet jetty kelp loam mesa nectar "
    "onyx pumice quarry russet shale talus umber vellum wharf yonder zephyr "
    "basalt copse dune eyrie fjord grove heath islet knoll ledge marsh notch"
).split()


def log(msg: str) -> None:
    print(f"[muse:bench] {msg}", flush=True)


def fail(msg: str) -> "sys.NoReturn":
    print(f"[muse:bench] ERROR: {msg}", file=sys.stderr, flush=True)
    sys.exit(1)


# -- preflight -----------------------------------------------------------------


def serving_processes(ignore_pid: int | None = None) -> list[str]:
    out = subprocess.run(["/bin/ps", "-axo", "pid=,rss=,command="], capture_output=True, text=True)
    hits = []
    for line in out.stdout.splitlines():
        parts = line.strip().split(None, 2)
        if len(parts) < 3:
            continue
        pid_s, rss_s, cmd = parts
        if ignore_pid is not None and pid_s == str(ignore_pid):
            continue
        if "bench.py" in cmd:
            continue
        if not any(pat in cmd for pat in BLOCKING_PATTERNS):
            continue
        # A daemon that has not loaded weights holds tens of MB and is only a
        # supervisor; one holding model memory is the hazard. Engines are a
        # hazard at any size (they map weights immediately).
        rss_gb = int(rss_s) * 1024 / 2**30 if rss_s.isdigit() else 0.0
        if "llama-server" in cmd or "mlx" in cmd or rss_gb > 2.0:
            hits.append(f"{pid_s} rss={rss_gb:.1f}GB {cmd[:140]}")
    return hits


def port_free(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        return s.connect_ex(("127.0.0.1", port)) != 0


# -- engine lifecycle ----------------------------------------------------------


class Engine:
    """One llama-server process that cannot be lost track of.

    The pid is written to a pidfile before the model starts mapping, cleanup is
    registered with atexit *and* SIGINT/SIGTERM, and a watchdog thread kills the
    engine outright the moment system free memory crosses the floor or the
    process outgrows its declared footprint. A dead benchmark is recoverable; a
    machine buried under memory pressure is not.
    """

    def __init__(self, argv: list[str], log_path: Path, rss_cap_bytes: int):
        self.argv = argv
        self.log_path = log_path
        self.rss_cap_bytes = rss_cap_bytes
        self.proc: subprocess.Popen | None = None
        self.peak_rss_bytes = 0
        self.aborted_reason: str | None = None
        self._stop = threading.Event()
        self._watchdog: threading.Thread | None = None

    def start(self) -> None:
        logfile = open(self.log_path, "ab")
        self.proc = subprocess.Popen(
            self.argv, stdin=subprocess.DEVNULL, stdout=logfile, stderr=logfile
        )
        PID_FILE.write_text(str(self.proc.pid))
        atexit.register(self.kill)
        for sig in (signal.SIGINT, signal.SIGTERM):
            signal.signal(sig, self._on_signal)
        self._watchdog = threading.Thread(target=self._watch, daemon=True)
        self._watchdog.start()

    def _on_signal(self, signum, frame) -> None:
        self.kill()
        sys.exit(128 + signum)

    def _watch(self) -> None:
        while not self._stop.is_set() and self.proc and self.proc.poll() is None:
            out = subprocess.run(
                ["/bin/ps", "-o", "rss=", "-p", str(self.proc.pid)],
                capture_output=True,
                text=True,
            )
            try:
                rss = int(out.stdout.strip()) * 1024
                self.peak_rss_bytes = max(self.peak_rss_bytes, rss)
            except ValueError:
                rss = 0
            free_pct = system_free_pct()
            if free_pct is not None and free_pct < WATCHDOG_MIN_FREE_PCT:
                self.aborted_reason = (
                    f"watchdog: system free memory {free_pct}% < {WATCHDOG_MIN_FREE_PCT}% floor"
                )
            elif rss > self.rss_cap_bytes:
                self.aborted_reason = (
                    f"watchdog: engine RSS {rss / 1e9:.1f} GB exceeded the "
                    f"{self.rss_cap_bytes / 1e9:.1f} GB cap"
                )
            else:
                foreign = serving_processes(ignore_pid=self.proc.pid)
                if foreign:
                    self.aborted_reason = (
                        "watchdog: another model process appeared mid-benchmark: "
                        + "; ".join(foreign)
                    )
            if self.aborted_reason:
                log(f"KILLING ENGINE — {self.aborted_reason}")
                self.kill()
                return
            self._stop.wait(2.0)

    def kill(self) -> None:
        """Immediate, unconditional teardown. Safe to call more than once."""
        self._stop.set()
        if self.proc and self.proc.poll() is None:
            self.proc.kill()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                pass
        PID_FILE.unlink(missing_ok=True)

    def stop(self) -> None:
        self._stop.set()
        if self.proc and self.proc.poll() is None:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=10)
        PID_FILE.unlink(missing_ok=True)

    def alive(self) -> bool:
        return self.proc is not None and self.proc.poll() is None


# -- HTTP ----------------------------------------------------------------------


def http_json(url: str, body: dict | None = None, timeout: float = 60.0) -> tuple[int, dict]:
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(
        url, data=data, headers={"Content-Type": "application/json"}, method="POST" if data else "GET"
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        try:
            return e.code, json.loads(e.read().decode())
        except Exception:
            return e.code, {}
    except (urllib.error.URLError, TimeoutError, ConnectionError) as e:
        raise ConnectionError(str(e)) from e


def wait_ready(base: str, engine: Engine, timeout_s: float) -> float:
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout_s:
        if not engine.alive():
            fail(f"engine exited during load; see {engine.log_path}")
        try:
            status, _ = http_json(f"{base}/health", timeout=3.0)
            if status == 200:
                return time.monotonic() - t0
        except ConnectionError:
            pass
        time.sleep(0.25)
    fail(f"engine did not become ready within {timeout_s:.0f}s; see {engine.log_path}")


# -- workload ------------------------------------------------------------------


def build_document(base: str, target_tokens: int) -> tuple[str, int]:
    """Deterministic prose sized to >= target_tokens by the engine tokenizer."""
    rng = random.Random(target_tokens)

    def prose(n_words: int) -> str:
        out, line = [], []
        for i in range(n_words):
            line.append(rng.choice(WORDS))
            if len(line) >= rng.randint(9, 15):
                out.append(" ".join(line) + ".")
                line = []
            if i and i % 130 == 0:
                out.append("\n\n")
        if line:
            out.append(" ".join(line) + ".")
        return " ".join(out)

    words = int(target_tokens * 0.72)
    for _ in range(8):
        text = prose(words)
        _, body = http_json(f"{base}/tokenize", {"content": text}, timeout=120.0)
        n = len(body.get("tokens", []))
        if n >= target_tokens:
            return text, n
        words = int(words * (target_tokens / max(n, 1)) * 1.03) + 50
    fail("could not calibrate prompt size against /tokenize")


def chat_request(document: str, question: str, max_tokens: int, sampling: dict) -> dict:
    return {
        "model": "meta-models/Muse-Glimmer-30B-GGUF",
        "stream": True,
        "stream_options": {"include_usage": True},
        "max_tokens": max_tokens,
        "messages": [
            {
                "role": "user",
                "content": (
                    "Read the following survey field log carefully. You will be "
                    "asked about it.\n\n<log>\n" + document + "\n</log>\n\n" + question
                ),
            }
        ],
        **sampling,
    }


def stream_chat(base: str, body: dict, timeout: float = 1200.0) -> dict:
    """POST a streaming chat completion; return client + engine measurements."""
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        f"{base}/v1/chat/completions",
        data=data,
        headers={"Content-Type": "application/json", "Accept": "text/event-stream"},
        method="POST",
    )
    t_start = time.monotonic()
    t_first_token = None
    n_chunks = 0
    content_parts: list[str] = []
    reasoning_parts: list[str] = []
    timings = None
    usage = None
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        for raw in resp:
            line = raw.decode("utf-8", "replace").strip()
            if not line.startswith("data:"):
                continue
            payload = line[5:].strip()
            if not payload or payload == "[DONE]":
                continue
            try:
                chunk = json.loads(payload)
            except ValueError:
                continue
            if isinstance(chunk.get("timings"), dict):
                timings = chunk["timings"]
            if isinstance(chunk.get("usage"), dict):
                usage = chunk["usage"]
            for choice in chunk.get("choices") or []:
                delta = choice.get("delta") or {}
                content = delta.get("content") or ""
                reasoning = delta.get("reasoning_content") or ""
                if content:
                    content_parts.append(content)
                if reasoning:
                    reasoning_parts.append(reasoning)
                if content or reasoning:
                    if t_first_token is None:
                        t_first_token = time.monotonic()
                    n_chunks += 1
    t_end = time.monotonic()
    if timings is None:
        fail("engine stream carried no timings object; cannot report measured numbers")
    return {
        "client": {
            "wall_s": round(t_end - t_start, 3),
            "ttft_s": round((t_first_token - t_start), 3) if t_first_token else None,
            "chunks": n_chunks,
            "text_chars": len("".join(content_parts)) + len("".join(reasoning_parts)),
        },
        "engine_timings": timings,
        "engine_usage": usage,
        "message": {
            "content": "".join(content_parts),
            "reasoning_content": "".join(reasoning_parts),
        },
    }


# -- main ----------------------------------------------------------------------


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--label", default="run", help="name for this configuration")
    ap.add_argument("--out", default=None, help="output JSON path")
    ap.add_argument("--runtime", default=str(DEFAULT_RUNTIME), help="llama-server binary")
    ap.add_argument("--model-dir", default=str(DEFAULT_MODEL_DIR))
    ap.add_argument("--port", type=int, default=7434, help="benchmark engine port (not the production 7334)")
    ap.add_argument("--ctx-size", type=int, default=131072)
    ap.add_argument("--prompt-tokens", type=int, default=17033, help="minimum uncached prompt size; never reduced below this")
    ap.add_argument("--max-tokens", type=int, default=256, help="decode length for the measured generation")
    ap.add_argument("--n-batch", type=int, default=None, help="--batch-size override")
    ap.add_argument("--n-ubatch", type=int, default=None, help="--ubatch-size override")
    ap.add_argument("--flash-attn", default="on", choices=["on", "off", "auto"])
    ap.add_argument("--cache-type-k", default=None)
    ap.add_argument("--cache-type-v", default=None)
    ap.add_argument("--threads", type=int, default=None)
    ap.add_argument("--n-gpu-layers", default="999")
    ap.add_argument("--draft-max", type=int, default=None, help="--draft-max override (engine default: 3)")
    ap.add_argument("--draft-min", type=int, default=None)
    ap.add_argument("--no-draft", action="store_true", help="run without the DFlash draft model")
    ap.add_argument("--no-mmproj", action="store_true", help="run without the vision projector")
    ap.add_argument("--extra-arg", action="append", default=[], help="raw extra llama-server arg (repeatable)")
    ap.add_argument("--ready-timeout", type=float, default=300.0)
    ap.add_argument("--footprint-gb", type=float, default=DEFAULT_FOOTPRINT_GB,
                    help="expected engine footprint used for admission and the watchdog RSS cap")
    ap.add_argument("--headroom-gb", type=float, default=DEFAULT_HEADROOM_GB,
                    help="system memory that must remain free beyond the footprint")
    ap.add_argument("--cooldown", type=float, default=0.0,
                    help="seconds to wait before starting, letting the SoC shed heat from a previous run")
    args = ap.parse_args()

    if args.cooldown > 0:
        log(f"cooling down {args.cooldown:.0f}s before this run")
        time.sleep(args.cooldown)

    runtime = Path(args.runtime)
    model_dir = Path(args.model_dir)
    if not runtime.is_file():
        fail(f"llama-server not found at {runtime}")
    for f in (MAIN_GGUF,) + (() if args.no_draft else (DFLASH_GGUF,)) + (() if args.no_mmproj else (MMPROJ_GGUF,)):
        if not (model_dir / f).is_file():
            fail(f"missing model file {model_dir / f}")

    reap_stale_bench_engine()
    hits = serving_processes()
    if hits:
        fail("refusing to run: model-serving processes are alive:\n  " + "\n  ".join(hits))
    if not port_free(args.port):
        fail(f"port {args.port} is already bound")

    # Memory admission: the model does not get to start loading unless the
    # whole footprint plus system headroom is genuinely free right now.
    free_pct = system_free_pct()
    total_gb = total_memory_bytes() / 2**30
    free_gb = (free_pct / 100.0) * total_gb if free_pct is not None else None
    need_gb = args.footprint_gb + args.headroom_gb
    swap_gb = swap_used_gb()
    if free_gb is None:
        fail("could not measure free memory; refusing to load a model blind")
    if free_gb < need_gb:
        fail(
            f"insufficient memory: {free_gb:.1f} GB free of {total_gb:.0f} GB, need "
            f"{need_gb:.1f} GB ({args.footprint_gb:.0f} footprint + {args.headroom_gb:.0f} headroom). "
            "Close memory-heavy processes and retry."
        )
    if swap_gb is not None and swap_gb > 2.0:
        log(f"note: {swap_gb:.1f} GB swap already in use; watchdog floor is {WATCHDOG_MIN_FREE_PCT}% free")
    log(f"admission: {free_gb:.1f} GB free, footprint {args.footprint_gb:.0f} GB + headroom {args.headroom_gb:.0f} GB — OK")

    base = f"http://127.0.0.1:{args.port}"
    scratch = Path(os.environ.get("MUSE_BENCH_DIR", Path.cwd()))
    out_path = Path(args.out) if args.out else scratch / f"muse-bench-{args.label}.json"
    out_path.parent.mkdir(parents=True, exist_ok=True)
    engine_log = out_path.with_suffix(".engine.log")

    argv = [
        str(runtime),
        "--model", str(model_dir / MAIN_GGUF),
        "--alias", "meta-models/Muse-Glimmer-30B-GGUF",
        "--host", "127.0.0.1",
        "--port", str(args.port),
        "--ctx-size", str(args.ctx_size),
        "--parallel", "1",
        "--flash-attn", args.flash_attn,
        "--cache-prompt",
        "--jinja",
        "--reasoning-format", "deepseek",
        "--temp", "1.0",
        "--top-p", "0.95",
        "--top-k", "64",
        "--n-gpu-layers", str(args.n_gpu_layers),
    ]
    if not args.no_mmproj:
        argv += ["--mmproj", str(model_dir / MMPROJ_GGUF)]
    if not args.no_draft:
        argv += [
            "--model-draft", str(model_dir / DFLASH_GGUF),
            "--spec-type", "draft-dflash",
            "--n-gpu-layers-draft", "999",
        ]
        if args.draft_max is not None:
            argv += ["--spec-draft-n-max", str(args.draft_max)]
        if args.draft_min is not None:
            argv += ["--spec-draft-n-min", str(args.draft_min)]
    if args.n_batch is not None:
        argv += ["--batch-size", str(args.n_batch)]
    if args.n_ubatch is not None:
        argv += ["--ubatch-size", str(args.n_ubatch)]
    if args.cache_type_k:
        argv += ["--cache-type-k", args.cache_type_k]
    if args.cache_type_v:
        argv += ["--cache-type-v", args.cache_type_v]
    if args.threads is not None:
        argv += ["--threads", str(args.threads)]
    for extra in args.extra_arg:
        argv += shlex.split(extra)

    sampling = {"temperature": 1.0, "top_p": 0.95, "top_k": 64}
    result: dict = {
        "label": args.label,
        "engine_binary": str(runtime),
        "engine_argv": argv[1:],
        "sampling": sampling,
        "prompt_target_tokens": args.prompt_tokens,
        "thermal_cpu_speed_limit_start_pct": cpu_speed_limit_pct(),
        "measurements": {},
    }

    log(f"starting engine: {runtime.name} on :{args.port} ({args.label})")
    # RSS cap: footprint plus slack for measurement noise; the watchdog kills
    # the engine rather than let it grow past what admission budgeted for.
    engine = Engine(argv, engine_log, rss_cap_bytes=int((args.footprint_gb + 4.0) * 2**30))
    engine.start()
    try:
        cold_load_s = wait_ready(base, engine, args.ready_timeout)
        result["measurements"]["cold_load_s"] = round(cold_load_s, 2)
        log(f"engine ready in {cold_load_s:.1f}s; calibrating prompt")

        document, doc_tokens = build_document(base, args.prompt_tokens)
        result["document_tokens_raw"] = doc_tokens
        question = "Count how many distinct place-related nouns appear in the log, then summarize its overall structure in detail."
        body = chat_request(document, question, args.max_tokens, sampling)

        log(f"run 1/5: uncached prefill ({doc_tokens} raw document tokens)")
        uncached = stream_chat(base, body)
        result["measurements"]["uncached"] = uncached
        t = uncached["engine_timings"]
        log(
            f"  engine: prompt_n={t.get('prompt_n')} cache_n={t.get('cache_n')} "
            f"prefill={t.get('prompt_per_second', 0):.1f} tok/s "
            f"decode={t.get('predicted_per_second', 0):.1f} tok/s "
            f"draft={t.get('draft_n_accepted', 0)}/{t.get('draft_n', 0)}"
        )
        if t.get("cache_n", 0) > args.prompt_tokens * 0.05:
            log("  WARNING: uncached run reused a cached prefix; treat prefill number as tainted")

        log("run 2/5: identical prompt (cache reuse)")
        cached = stream_chat(base, body)
        result["measurements"]["cached"] = cached
        t2 = cached["engine_timings"]
        log(
            f"  engine: prompt_n={t2.get('prompt_n')} cache_n={t2.get('cache_n')} "
            f"prefill={t2.get('prompt_per_second', 0):.1f} tok/s"
        )

        log("run 3/5: decode-focused continuation")
        decode_body = chat_request(
            document,
            "Now write a long, detailed narrative report of the survey, section by section.",
            args.max_tokens,
            sampling,
        )
        decode = stream_chat(base, decode_body)
        result["measurements"]["decode"] = decode
        t3 = decode["engine_timings"]
        log(
            f"  engine: cache_n={t3.get('cache_n')} decode={t3.get('predicted_per_second', 0):.1f} tok/s "
            f"({t3.get('predicted_n')} tokens) draft={t3.get('draft_n_accepted', 0)}/{t3.get('draft_n', 0)}"
        )

        # Structured decode: transcription is the most predictable output this
        # workload offers, so it bounds what the draft model can achieve here.
        # Free-form prose at temp 1.0 bounds the other end; a draft config only
        # deserves to ship if it wins (or at least does not lose) at both ends.
        log("run 4/5: structured decode (draft best-case)")
        structured_body = chat_request(
            document,
            "Transcribe the first 80 words of the log exactly as written, one per line, no commentary.",
            args.max_tokens,
            sampling,
        )
        structured = stream_chat(base, structured_body)
        result["measurements"]["decode_structured"] = structured
        ts = structured["engine_timings"]
        log(
            f"  engine: decode={ts.get('predicted_per_second', 0):.1f} tok/s "
            f"({ts.get('predicted_n')} tokens) draft={ts.get('draft_n_accepted', 0)}/{ts.get('draft_n', 0)}"
        )

        # Multi-turn follow-up: history now contains an assistant message with
        # a reasoning trace. Whether the template re-renders that trace decides
        # whether the KV prefix survives — this is the agentic tool-turn shape,
        # and cache_n here is the honest measure of it.
        log("run 5/5: multi-turn follow-up (agentic cache shape)")
        followup_body = dict(decode_body)
        followup_body["messages"] = decode_body["messages"] + [
            {
                "role": "assistant",
                "content": decode["message"]["content"],
                "reasoning_content": decode["message"]["reasoning_content"],
            },
            {"role": "user", "content": "Good. Which section is weakest, and why?"},
        ]
        followup = stream_chat(base, followup_body)
        result["measurements"]["multiturn"] = followup
        t4 = followup["engine_timings"]
        log(
            f"  engine: prompt_n={t4.get('prompt_n')} cache_n={t4.get('cache_n')} "
            f"prefill={t4.get('prompt_per_second', 0):.1f} tok/s ttft={followup['client']['ttft_s']}s"
        )
    except (ConnectionError, urllib.error.URLError, TimeoutError) as exc:
        if engine.aborted_reason:
            fail(f"run aborted by the memory watchdog: {engine.aborted_reason}")
        fail(f"engine connection failed mid-run: {exc}; see {engine_log}")
    finally:
        engine.stop()
        result["measurements"]["peak_rss_bytes"] = engine.peak_rss_bytes
        result["measurements"]["peak_rss_gb"] = round(engine.peak_rss_bytes / 1e9, 2)
        if engine.aborted_reason:
            result["aborted_reason"] = engine.aborted_reason
        result["thermal_cpu_speed_limit_end_pct"] = cpu_speed_limit_pct()

    def spec_summary(t: dict) -> dict:
        d, a = t.get("draft_n", 0), t.get("draft_n_accepted", 0)
        return {
            "draft_proposed": d,
            "draft_accepted": a,
            "acceptance_pct": round(100.0 * a / d, 1) if d else None,
        }

    m = result["measurements"]
    result["summary"] = {
        "cold_load_s": m.get("cold_load_s"),
        "uncached_prompt_n": m["uncached"]["engine_timings"].get("prompt_n"),
        "uncached_prefill_tok_s": round(m["uncached"]["engine_timings"].get("prompt_per_second", 0), 1),
        "uncached_prefill_wall_s": round(m["uncached"]["engine_timings"].get("prompt_ms", 0) / 1000, 1),
        "uncached_ttft_client_s": m["uncached"]["client"]["ttft_s"],
        "cached_reused_tokens": m["cached"]["engine_timings"].get("cache_n"),
        "cached_ingest_tok_s": round(m["cached"]["engine_timings"].get("prompt_per_second", 0), 1),
        "cached_ttft_client_s": m["cached"]["client"]["ttft_s"],
        "decode_tok_s": round(m["decode"]["engine_timings"].get("predicted_per_second", 0), 1),
        "decode_tokens": m["decode"]["engine_timings"].get("predicted_n"),
        "speculation": spec_summary(m["decode"]["engine_timings"]),
        "peak_rss_gb": m.get("peak_rss_gb"),
    }
    if "decode_structured" in m:
        ts = m["decode_structured"]["engine_timings"]
        result["summary"]["decode_structured"] = {
            "decode_tok_s": round(ts.get("predicted_per_second", 0), 1),
            "decode_tokens": ts.get("predicted_n"),
            "speculation": spec_summary(ts),
        }
    if "multiturn" in m:
        t4 = m["multiturn"]["engine_timings"]
        result["summary"]["multiturn"] = {
            "prompt_n": t4.get("prompt_n"),
            "reused_tokens": t4.get("cache_n"),
            "ttft_client_s": m["multiturn"]["client"]["ttft_s"],
        }
    out_path.write_text(json.dumps(result, indent=2) + "\n")
    log(f"wrote {out_path}")
    log("summary: " + json.dumps(result["summary"]))


if __name__ == "__main__":
    main()
