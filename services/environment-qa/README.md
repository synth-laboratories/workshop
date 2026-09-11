# Environment QA

## Current Codex profiles

Use `--profile tbench-hitl`, `--profile tbench-non-hitl`, or
`--profile k3-non-hitl` on the `run` command, or select the same named profile
in Workshop/standalone. The profile pins mode. The sections below describing
`pipeline: targeted` and direct-provider variables are historical compatibility
paths, not the current profile launch contract (`profile_id` in the API).

Each AI gate owns a Codex app-server process and isolated configuration/history.
The only model is GPT-5.6 Luna via OpenRouter. An authenticated local transport
guard restricts model/tools, caps input bytes and output/reasoning tokens, and
reserves every upstream request against the run allowance (including retries).
The real provider key remains in the service/Harbor host worker, never in the
app-server or task container. Native shell, web tools and ambient MCP are not
forwarded to the model. Only explicitly scoped dynamic tools are published.
The installed app-server wraps them in its Code Mode `exec`/`wait` protocol;
these orchestration calls are not shell commands. Nested dynamic calls are
checked against the gate grant, and the thread has no default host environment.

Start using `scripts/serve_authorized.py --help`; choose an authorized project
`.env`, an exact Codex executable, allowed task roots and an aggregate allowance.
`scripts/runtime_smoke.py` is a separately bounded real model/tool compatibility
test, not a benchmark score. No Keychain or implicit account login is used.

Pause stops new gate/turn admission while bounded in-flight work may finish.
The paused panel displays outstanding gates. Live permission and clarification
requests have separate evidence-bound response contracts. `agent-cua` decisions
never count as independent human adjudication, including post-seal review.
After a restart, workflow checkpoints remain usable; lost live RPC requests are
superseded, never blindly replayed. The recovery panel can retry proven unstarted
capacity failures or explicitly selected interrupted source-only reviews. It
does not replay container side effects, completed gates or refund uncertain charges.

Transport bounds are not invoices. Completed gate token totals may reconcile
unused capacity at conservative configured rate ceilings; failed attempts retain
their original bounds. The inspector and persisted events retain original attempts.
Final quality acceptance still requires the expanded frozen cohorts and independent
adjudication described in the takeover plan; fixture success is not acceptance.

The TBench v1.0.2 profiles have 31 gates; K3 v1.0.3 has 15, including an explicit
candidate-plan gate consumed by its shortcut agent. TBench uses source reviews
and at most two targeted experiments, not repeated full-task solves:

```text
snapshot -> admission -> source/dependency reviews (parallel)
                                  |
                             probe planning
                                  |
                   HITL approval / automatic policy
                                  |
                    bounded Harbor experiments (0–2)
                                  |
                  analysis -> attribution -> critic
                                  |
                   HITL reviews / automatic policy
                                  |
                       sealed findings + coverage
```

The shared HTML interface runs
independently or in Workshop's Environment QA pane beside Evals and Optimizers.
Python 3.11+, PyYAML, Harbor and Docker are required for full runs.

## Historical full-trial pipeline (not the current TBench profiles)

```text
snapshot → admission
             ├─ structure → build ─┬─ oracle ×3
             │                     ├─ no-op
             │                     └─ repeat verifier
             ├─ specification ─┐
             └─ verifier ──────┴─ probe plan → approval
build + approval → negative / alternative / independent solver / cheat
all trials → trajectory analysis → targeted plan → approval
           → targeted cheat → analysis → attribution
                               ├─ critic → technical review ─┐
                               └─ domain review ─────────────┴─ disposition → seal
private reference scorer ← sealed predictions only
```

AI-only uses policy authorization at probe gates and AI technical/domain judgments.
HITL pauses at four explicit interaction gates. Silence never means approval;
expiry is inconclusive. Independent branches continue while a human reviews an
immutable interaction context. Decisions are durable and idempotent.
Neither mode automatically repairs tasks or publishes PRs.

## Run

Configure QA_PROVIDER_URL, QA_PROVIDER_MODEL, QA_PROVIDER_KEY,
QA_INPUT_USD_PER_MILLION and QA_OUTPUT_USD_PER_MILLION through an authorized
project environment or secrets proxy. Full policy pins openai/gpt-5.6-luna;
Claude is prohibited. No Keychain access occurs. Credentials remain host-side,
outside task containers and command arguments.

```sh
PYTHONPATH=services/environment-qa python3 -m environment_qa \
  --store /absolute/path/to/qa-state run /absolute/path/to/task \
  --full --reviewer ai --mode automated --budget-usd 5
```

Use --mode hitl for human gates, --charter reb-systems or reb-visuals, and
--task-goals for task-specific intent. --policy loads trusted declarative JSON;
task files cannot modify policy. --enqueue submits to a service-owned store.
Legacy source-only runs remain available without --full and are labelled as such.

```sh
PYTHONPATH=services/environment-qa python3 -m environment_qa \
  --store /absolute/path/to/qa-state serve --task-root /absolute/path/to/tasks
```

Open http://127.0.0.1:7338/ or Workshop's Environment QA pane. Multiple task roots
can include REB suites. Paid UI runs additionally require explicitly authorized
--provider-budget-usd and unique --allowance-id. This aggregate allowance persists
and does not replenish on restart. Each run commits its full maximum upfront.
Independent CLI runs must be bounded together by their caller. These flags do not
bypass product-enforced paid-compute approvals.

## Run provenance

Every profile run must declare which client launched it — `workshop-embed`,
`standalone-web`, `cli` or `test`. A profile run that does not is refused rather
than recorded as unknown, because the acceptance plan requires each profile
exercised through both interfaces and a report that only asserts that in prose
cannot be checked afterwards. The service cross-checks the claim against the
`Referer` the browser set, which page script cannot author, and records claimed,
attested and whether they agree. It does not decide which side is right when they
disagree; it records the disagreement. The surface is inside the seal, so
relabelling which interface produced a run breaks verification.

Review decisions carry an explicit actor. There is no default: a caller that says
nothing used to be recorded as `local-human`. Start the service with
`--operator-token` to give the reviewing person a secret an automated harness is
not given; a `local-human` decision that presents it as `X-QA-Operator` is
recorded with assurance `operator-token`, and one that does not is recorded as
`unverified-client-claim`. The token cannot promote an `agent-cua` decision. The
certificate reports `human_decision_count` and `verified_human_decision_count`
separately.

This is not proof that a person pressed a button. The service is loopback-only
and unauthenticated, and `identity_assurance` stays `local-operator-only`. The
difference it buys is that a verified human decision requires something beyond
the request body to be true.

## Evidence and safety

- Content-addressed bundles exclude Git metadata, credentials, reference reviews
  and future snapshots. Symlinks and oversized inputs fail closed.
- Container trials retain hashed artifacts, rewards, trajectories and exact-project
  cleanup receipts. Task commands never execute directly on the host. Compose
  admission rejects privileged modes, host mounts and external resources.
- SQLite persists gate attempts, events, interactions and atomic provider budgets.
  Attempt tokens fence stale work; one scheduler owns a store.
- Pause stops dispatch; in-flight calls may finish. Cancel interrupts Harbor.
  Restart pauses ambiguous attempts as inconclusive instead of spending again.
- Transport failures are not retried. Invalid structured responses get at most one
  separately budgeted repair. Unknown usage stays unknown; reservations remain
  conservative upper bounds.
- AI findings require verbatim evidence; unsupported quotes are quarantined.
  Attribution preserves AI disposition history, separate from human decisions.
- Seals bind policy, snapshot, evidence, findings, decisions and budget. They detect
  modification but are not signatures against a writer with local database access.
- Post-seal compare uses a separate private scorer store. Unmatched predictions
  are not automatically false positives. AI matches are provisional; primary
  precision/recall stay unset until independent adjudication.

## Verification and boundaries

```sh
PYTHONPATH=services/environment-qa python3 -m unittest discover -s services/environment-qa/tests -v
node services/environment-qa/tests/browser-smoke.mjs
node services/environment-qa/tests/browser-dag.mjs
```

Browser DAG tests use synthetic executors, testing workflow rather than model
quality. Live calibration artifacts are in workshop-release/evals/environment-qa-terminal-bench;
validation runs freeze their engine source.

This local integration is not production certification. Native service supervision,
multi-user reviewer authentication, signed certificates and change-trigger scheduling
remain separate work. Service startup is explicit. Docker via Harbor is the admitted
backend; unsupported multi-step tasks fail closed. Other backends need conformance
adapters. The targeted loop has one follow-up round, not autonomous repair.
Oracle count and DAG nodes are trusted policy configuration. Cancellation retains
partial records but currently does not issue a prediction seal.
# Targeted QA

Quality-loop revision 4.3 preserves complete causal allegations and independently
validated supporting quotes rather than squeezing explanations into titles. A
source-derived dependency-contract inventory reserves one of the two probe slots
for bounded API/input checks when justified. An explicit backward assertion audit
requires coverage of each Python verifier assertion (up to 80, with omissions
recorded). An unavailable specialist response does not cancel safe probes, but
admission, structural validation and probe approval remain hard prerequisites.
Incomplete evidence remains a limitation and cannot yield a passing certificate.
The inspector receives hash-checked planner evidence as well as the original task
source. Finishing an inspection requires observing the previous command result;
a zero-status wrapper cannot silently end a group of failed inner checks. Current
dependency contracts trace producer values through downstream consumers. Build
ownership checks flag unordered parent-directory deletion versus child writes as
conditional risks, not measured races. Long source quotes are referenced rather
than copied repeatedly in adjudication context; raw files and quotes remain sealed.
Provider-reported settled charges release unused reservations; unknown charges
retain their worst-case reservation. This never raises the run's spending limit.

An independent contract-analysis node compares expected properties with actual
command output, excluding the execution agent's success narrative. It reads the
hash-verified original trajectory instead of the UI preview. Command streams are
retained as separate artifacts; bounded previews preserve diagnostic measurements
and explicitly mark omissions. Adjudication receives flattened output references
to avoid repeatedly escaping the same transcript. Explicit Python build-frontend
invocations receive mandatory smoke coverage on a minimal substitute project,
not a full application build. Probe-imposed restrictions are not task defects.

Choose **Targeted QA** in the web interface, send `pipeline: "targeted"` to
the run API, or use `run TASK --targeted --reviewer ai --budget-usd LIMIT`.
The existing full policy remains available unchanged.

Targeted QA reviews sources first, proposes zero to two explicit hypotheses
with objectives and confirmation criteria, and preserves the probe approval
gate in HITL mode. Each selected experiment uses a fresh Harbor container,
at most eight agent actions, and a 120-second trial deadline (plus cleanup).
No reference solves or repeat trials are mandatory. Unselected slots explicitly
record that no experiment occurred; they are not runtime validation passes.
Deferred questions and timeouts remain limitations through adjudication.
Technical and domain review gates remain available in both modes.

Component probes explicitly skip Harbor grading; full-verifier probes retain it.
The reviewer receives privileged source access, labeled as instrumentation rather
than agent-visible leakage. Completion requires successful observations and an
explicit finish; failed commands and exhausted step budgets are not completed probes.
Eight distinct specialties each receive two independent source-review passes,
with at most four candidate mechanisms per pass. Read-only dependency reconnaissance
fetches bounded task-named upstream source and CRAN metadata, with hashes and timestamps.
Oracle logs retain diagnostic excerpts; trajectory previews retain final observations.
The evidence-cache adapter verifies task identity, prediction seals and original artifact
hashes before reusing oracle observations, never prior findings or reference labels.
The earliest artifact-backed oracle attempt is selected by index, not by its outcome.
Exact diagnostic findings are newly derived from its raw observations.
Warm-cache evaluation timings exclude original evidence acquisition.
Independent criticism includes dismissed findings; disagreements remain unresolved.
Duplicate links preserve original causal facets under a canonical finding; invalid
or cyclic links remain unresolved instead of deleting evidence. Required keyed
adjudication entries prevent missing or repeated finding IDs. Source excerpts retain
dependency calls and downstream uses, with complete originals in hashed artifacts.
Inspector preflight records read access to original image-staged files separately
from the deliberately injected QA source copy.
The policy requests high reasoning effort for reviews and medium for probe actions,
retaining requested effort, returned model and token usage in response records.
These settings follow the [Luna model documentation](https://developers.openai.com/api/docs/models/gpt-5.6-luna)
and [Chat Completions schema](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create).
Reasoning-token counts are observations, not evidence of review correctness.
This version still pays fresh-container setup costs and does not guarantee a total wall-time bound.
Provider budgets and the post-seal private reference matcher are unchanged.

## Reference comparison semantics

Detection receives no public QA labels. Only sealed predictions reach the separate
matcher. Declared reference clauses are scored independently as identified or not
identified; broad category overlap does not earn causal-fragment credit. Multiple
findings can jointly identify one reference, without reusing them across references.
Coverage measures mechanism identification. Conditional source reasoning versus
observed runtime reproduction is reported separately, so a Docker-only run is not
misrepresented as a Modal reproduction. Compound-clause coverage and original
candidate decisions remain visible. Provisional labels do not yield qualified
precision or recall without independent adjudication.

### Native provider lifecycle development route

Install `requirements-native.txt` into the same environment as Harbor 0.22.0. A
policy with `pipeline.backend: "docker"` and `pipeline.native_image` set to an
immutable `image@sha256:...` selects shared staged native execution for full
verifier trials. The image must correspond to the sealed task; registration and
phase staging retain and verify its digest. Optional
`pipeline.native_resource_ttl_minutes` defaults to 60; isolated Docker egress
requires `pipeline.native_docker_egress_image` as required by task admission.
No image is built or resolved from a mutable tag by this route.

The existing oracle, no-op, repeat-verifier transformation and QA agent
assessment remain authoritative. Lifecycle evidence lives beside Harbor jobs
in `*-native/`; cleanup is established by the shared exact-provider custody
receipt, never by process exit or Compose label inference. Cancellation stops
the child and attempts bounded independent cleanup; pending absence makes the
QA gate inconclusive. Native component-only probes use the generic observation lifecycle described below. Policies
without `native_image` retain the existing Dockerfile/component path. This is a
development candidate, with no provider qualification performed.

### Native component and Daytona QA

Native component probes now use the generic durable lifecycle and always pass
`--disable-verification`. Source verifier declarations remain intact; no
private/shared verifier substitution is made and no reward is inferred.
The verifier phase records observation custody, while the existing component
completion logic remains authoritative. Separate-verifier source tasks can be
observed without running their verifier. Full-verifier trials retain the staged
Harbor coordinator and its existing verifier compatibility admission.

A policy with `pipeline.backend: "daytona"` requires `native_image` as a
prepared registry digest reference and `native_environment_digest` matching
containers' `sha256:...` digest of the exact source `environment/` directory.
The task must explicitly declare CPU, memory and storage within shared native
admission. Prepared images must already contain the task environment. Runtime
credentials come only from the already authorized service environment's
`DAYTONA_API_KEY`; they remain in the host Harbor process, never command argv.
No image preparation, publication, credential discovery, or provider execution
is performed by planning. Both native component and full-verifier Daytona
trials use shared owned-provider adapters and independent cleanup receipts.
Default Dockerfile policies remain available without native selection.
