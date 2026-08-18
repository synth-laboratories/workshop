#!/usr/bin/env node
// First-party QA control plane client for the eval loopback driver
// (synth.eval-driver.v1). Drives a packaged Workshop instance over
// authenticated loopback HTTP only — no Computer Use, no OS automation,
// no system dialogs. Dependency-free by design: global fetch only.
//
// Usage:
//   workshop-qa-driver.mjs [--descriptor <path>] [--prompt <text>|--prompt-file <path>]
//                          [--workflow banking77-smoke] [--model <id>]
//                          [--timeout-ms <n>] [--export <path>]
//
// The descriptor defaults to $SYNTH_DESKTOP_DATA_ROOT/eval-driver.json.
// Exit code 0 only when the session reaches a terminal state and exports.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";

const WORKFLOWS = {
  "banking77-smoke": [
    "Run the exact eval.banking77.baseline.v1 workflow using your available tools.",
    "The QA-owned Banking77 service is explicitly at http://127.0.0.1:8099; register",
    "that URL as task family banking77 if it is not already registered, then probe it.",
    "Start the product-owned optimizer workflow with its live chat visual, poll the",
    "optimizer run to terminal, and report the run id, completed rollout count, and",
    "numeric final score. Do not substitute a hand-built evaluation or stop after admission.",
    "Do not ask for confirmation; this session runs under the unattended QA profile.",
  ].join(" "),
  "banking77-gepa": [
    "Run the exact gepa.banking77.luna.v1 workflow using your available tools.",
    "The QA-owned Banking77 service is explicitly at http://127.0.0.1:8099; register",
    "that URL as task family banking77 if needed, then probe it. Verify the recipe",
    "advertises 10 proposals per generation before starting it. Open its live chat visual,",
    "poll the optimizer run all the way to terminal, call get_result, and report the run id,",
    "number of candidates attempted, seed score, winning train score, heldout score, numeric",
    "lift, rollout usage, and visual id. Do not claim uplift unless the heldout evidence proves it.",
    "Do not ask for confirmation; this session runs under the unattended QA profile.",
  ].join(" "),
  "healthbench-smoke": [
    "Run the exact eval.healthbench.smoke.v1 workflow using your available tools.",
    "The QA-owned HealthBench service is explicitly at http://127.0.0.1:8114; register",
    "that URL as task family healthbench if needed, then probe it. Start the product-owned",
    "optimizer workflow with its live chat visual, poll the run all the way to terminal,",
    "and report the run id, required and completed rollout counts, final score, separate",
    "policy and grader usage lanes, cost, and visual id. Do not substitute a hand-built eval.",
    "Do not ask for confirmation; this session runs under the unattended QA profile.",
  ].join(" "),
  "craftax-eval": [
    "Run only the exact eval.craftax.code-policy.smoke.v1 workflow using your available product tools and the QA-owned",
    "Craftax service at http://127.0.0.1:8097. Register and probe that URL as task family",
    "craftax. Stage exactly one baseline code-policy candidate. If that exact recipe is unavailable, stop with its readiness blocker. Never substitute",
    "gepa.craftax.*, another recipe family, a fixture, or a hand-built rollout loop. Run its rollouts concurrently, open its",
    "chat-owned live visual, and poll all work to terminal. Report the candidate count, exact",
    "rollout count (exactly 10), reward distribution, trace count (exactly 10 retained traces), concurrency evidence, elapsed time, and",
    "visual id. Do not substitute fixtures or a hand-written summary. Do not ask for confirmation.",
  ].join(" "),
};

function validateWorkflow(workflow, terminal) {
  if (!workflow) return;
  const item = terminal?.event?.payload?.outcome?.item;
  const text = typeof item?.text === "string" ? item.text : "";
  if (workflow === "banking77-smoke") {
    if (/could not run|blocked before|no (?:final )?score|unhealthy/i.test(text)) {
      throw new Error(`banking77-smoke failed: ${text.replace(/\s+/g, " ").trim()}`);
    }
    if (!/(?:final(?:\s+\w+){0,2}\s+score|score\s*[:=])[^\n]*\d/i.test(text)) {
      throw new Error("banking77-smoke reached terminal without a numeric final score");
    }
  }
  if (workflow === "banking77-gepa") {
    if (/could not run|blocked before|unhealthy|failed(?: to start| during)|status:\s*`?failed/i.test(text)) {
      throw new Error(`banking77-gepa failed: ${text.replace(/\s+/g, " ").trim()}`);
    }
    for (const required of [/candidate/i, /heldout/i, /lift[^\n]*[-+]?\d/i, /visual/i]) {
      if (!required.test(text)) {
        throw new Error(`banking77-gepa terminal summary is missing ${required}`);
      }
    }
  }
  if (workflow === "healthbench-smoke") {
    // Missing credentials are an expected *admission* outcome in the
    // credential-readiness lane. Treat it as green only when the response is
    // structured and identifies the owning HealthBench lane; generic prose or
    // a partially-created run remains red.
    if (/credential_missing/i.test(text)) {
      if (!/healthbench\.(?:policy|grader)/i.test(text)) {
        throw new Error("healthbench credential blocker is missing its lane owner");
      }
      if (/run id\s*[:=]\s*[`\w-]+/i.test(text)) {
        throw new Error("healthbench created a run despite credential preflight failure");
      }
      return;
    }
    if (/could not run|blocked before|unhealthy|failed(?: to start| during)|status:\s*`?failed/i.test(text)) {
      throw new Error(`healthbench-smoke failed: ${text.replace(/\s+/g, " ").trim()}`);
    }
    for (const required of [/score[^\n]*\d/i, /policy/i, /grader/i, /visual/i]) {
      if (!required.test(text)) throw new Error(`healthbench-smoke terminal summary is missing ${required}`);
    }
  }
  if (workflow === "craftax-eval") {
    if (/could not run|blocked before|unhealthy|failed(?: to start| during)|status:\s*`?failed/i.test(text)) {
      throw new Error(`craftax-eval failed: ${text.replace(/\s+/g, " ").trim()}`);
    }
    for (const required of [/rollout/i, /distribution/i, /trace/i, /visual/i]) {
      if (!required.test(text)) throw new Error(`craftax-eval terminal summary is missing ${required}`);
    }
    if (!/eval\.craftax\.code-policy\.smoke\.v1/.test(text)) {
      throw new Error("craftax-eval terminal summary does not attest the exact requested recipe id");
    }
    if (!/\b10\b[\s\S]{0,40}rollout|rollout[\s\S]{0,40}\b10\b/i.test(text)) {
      throw new Error("craftax-eval terminal summary does not attest exactly ten rollouts");
    }
    if (!/\b10\b[\s\S]{0,40}trace|trace[\s\S]{0,40}\b10\b/i.test(text)) {
      throw new Error("craftax-eval terminal summary does not attest ten retained traces");
    }
    if (/\bgepa\.craftax\./.test(text)) {
      throw new Error("craftax-eval substituted a GEPA recipe");
    }
  }
}

function parseArgs(argv) {
  const args = { timeoutMs: 900_000 };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const next = () => {
      i += 1;
      if (i >= argv.length) throw new Error(`${flag} requires a value`);
      return argv[i];
    };
    if (flag === "--descriptor") args.descriptor = next();
    else if (flag === "--prompt") args.prompt = next();
    else if (flag === "--prompt-file") args.prompt = readFileSync(next(), "utf8");
    else if (flag === "--workflow") args.workflow = next();
    else if (flag === "--model") args.model = next();
    else if (flag === "--timeout-ms") args.timeoutMs = Number(next());
    else if (flag === "--export") args.exportPath = next();
    else throw new Error(`unknown flag ${flag}`);
  }
  return args;
}

function loadDescriptor(args) {
  const path =
    args.descriptor ??
    (process.env.SYNTH_DESKTOP_DATA_ROOT
      ? join(process.env.SYNTH_DESKTOP_DATA_ROOT, "eval-driver.json")
      : null);
  if (!path) {
    throw new Error(
      "no descriptor: pass --descriptor or set SYNTH_DESKTOP_DATA_ROOT",
    );
  }
  const descriptor = JSON.parse(readFileSync(path, "utf8"));
  const url = new URL(descriptor.url);
  if (url.hostname !== "127.0.0.1" && url.hostname !== "localhost") {
    throw new Error(`descriptor url is not loopback: ${descriptor.url}`);
  }
  return descriptor;
}

async function call(descriptor, method, path, body) {
  const response = await fetch(`${descriptor.url}${path}`, {
    method,
    headers: {
      authorization: `Bearer ${descriptor.token}`,
      "x-synth-eval-driver": descriptor.schemaVersion,
      "content-type": "application/json",
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    const detail = payload.error ?? JSON.stringify(payload);
    throw new Error(`${method} ${path} -> ${response.status}: ${detail}`);
  }
  return payload;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const prompt = args.prompt ?? WORKFLOWS[args.workflow];
  if (!prompt) {
    throw new Error(
      `no prompt: pass --prompt/--prompt-file or --workflow (${Object.keys(WORKFLOWS).join(", ")})`,
    );
  }
  const descriptor = loadDescriptor(args);
  const receipt = {
    driver: descriptor.schemaVersion,
    instance: descriptor.instanceName ?? null,
    startedAt: new Date().toISOString(),
  };

  const health = await call(descriptor, "GET", "/v1/health");
  if (health.ok !== true) throw new Error("driver health check failed");
  receipt.preflight = await call(descriptor, "GET", "/v1/preflight");

  const sessionId = `qa_${Date.now().toString(36)}`;
  await call(descriptor, "POST", "/v1/sessions", { sessionId });
  receipt.sessionId = sessionId;

  const message = { body: prompt };
  if (args.model) message.model = args.model;
  await call(descriptor, "POST", `/v1/sessions/${sessionId}/messages`, message);

  // Keep each HTTP exchange shorter than client/proxy header timeouts. GEPA
  // and SFT commonly run for many minutes; one request spanning the whole run
  // makes a healthy app look dead when Node's fetch transport closes it.
  const deadline = Date.now() + args.timeoutMs;
  let terminal;
  do {
    const remainingMs = deadline - Date.now();
    if (remainingMs <= 0) {
      throw new Error(`session ${sessionId} did not reach terminal within ${args.timeoutMs}ms`);
    }
    terminal = await call(
      descriptor,
      "POST",
      `/v1/sessions/${sessionId}/wait_terminal`,
      { timeoutMs: Math.min(20_000, remainingMs) },
    );
  } while (terminal?.terminal !== true);
  receipt.terminal = terminal;

  const exported = await call(
    descriptor,
    "GET",
    `/v1/sessions/${sessionId}/export`,
  );
  if (args.exportPath) {
    writeFileSync(args.exportPath, JSON.stringify(exported, null, 2));
    receipt.exportPath = args.exportPath;
  } else {
    receipt.exportEvents = Array.isArray(exported.events)
      ? exported.events.length
      : null;
  }
  // Always persist the durable evidence before applying the workflow-specific
  // release assertion. A red validator is most useful when its transcript and
  // tool receipts survive for diagnosis.
  validateWorkflow(args.workflow, terminal);
  receipt.finishedAt = new Date().toISOString();
  process.stdout.write(`${JSON.stringify(receipt)}\n`);
}

main().catch((error) => {
  process.stderr.write(`workshop-qa-driver: ${error.message}\n`);
  process.exit(1);
});
