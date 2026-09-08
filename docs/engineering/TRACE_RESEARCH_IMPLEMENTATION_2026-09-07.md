# Eval trace research implementation

This change uses existing optimizer/eval job IDs. Source traces and evidence
remain Containers V5 authority; Workshop stores immutable query snapshots and
presents optional annotation and visualization consumers.

```text
Existing eval job ID
  |-- Optimizer trial rows: candidate, task, seed, status, reward, config axes
  |-- Imported V5 trace digest -> verified archive
  |     |-- actors / sessions / events / messages / recorded links
  |     |-- reward definitions and reward records
  |     `-- evidence revisions / optional annotations / exact selectors
  v
trace_manage query (synth.trace-query.v2)
  -> saved snapshot + result IDs + digest
     |-- page / source                    core, no inference
     |-- prepare_annotations              optional, no Jesterky dependency
     |     `-- annotation_manage          existing estimate/reservation/jobs
     |-- jesterky_prepare                 optional installed plugin
     |     `-- Luna low actor/window jobs -> all proposals + retry receipts
     `-- open_query / open trace           optional catalog / replay visuals
```

## Query contract

Call `trace_manage` with operation `query` and `arguments.query`:

```json
{
  "schemaVersion": "synth.trace-query.v2",
  "evalJobIds": ["existing-eval-job-id"],
  "grain": "episodes",
  "where": [{"field": "reward", "op": "lte", "value": 0.3}],
  "annotationWhere": [{"field": "label", "op": "contains", "value": "recovery.failure_not_detected"}],
  "limit": 100
}
```

`annotationWhere`, `entityWhere`, and `rewardWhere` are optional existence
filters. They do not multiply episode rewards. `where` applies to the selected
grain (`episodes`, `entities`, `annotations`, or `rewards`). Supported operators:
`eq`, `ne`, `in`, `gte`, `lte`, `contains`, `missing`. No SQL or paths are accepted.

`includeChildEvals` follows the optimizer's existing child eval references.
`relation: annotation_target` correlates selected findings with selected
entities. `recorded_link` uses recorded relationships; `repeated_failed_action`
means the next recorded action by the same actor/session repeats its prior
failed action. It makes no claim about unrecorded ordering or model intent.

`aggregate: count` groups records. `aggregate: reward` measures distinct
episodes, retaining missing values and separating reward definitions/units.
Unknown semantics remain separated by job. Invalid episodes do not contribute
measured rewards. `paired_reward` takes two job IDs and matches environment,
version, task, seed, repeat, reward definition/version/units. Missing match keys,
unknown semantics, ambiguous pairs, and unpaired episodes remain explicit.

Responses contain `snapshotId`, `resultDigest`, `resultIds`, `rows`, and
`nextOffset`. `page` takes the snapshot ID, offset and limit (1–200). The snapshot
freezes all results; limit controls the response page only. The bound is 100,000
results / 64 MiB; excessive queries fail instead of returning partial aggregates.
A refreshed query produces a new snapshot when source/review inputs change.

`source` takes `snapshot_id`, `result_id`, optional `selector` (one of that
result's recorded citations), `offset`, and `source_limit` (1–64,000 characters).
Workshop resolves the archive; Containers verifies its digest and resolves the
canonical selector. Previews in query results are capped; source readback is
paged with a digest. Agent arguments never accept filesystem paths.

`prepare_annotations` takes a snapshot ID and 1–200 result IDs. It deduplicates
trace/container targets while preserving selected evidence and starts no work.
Use `annotation_manage` to discover definitions, estimate and start jobs using
the existing reservation flow. Annotation absence, no findings, abstention,
failure and running state are separate facts.

## Optional Jesterky

Plugins → Jesterky provides download/update, enable/disable, removal and channel
selection using the same approval broker and receipt structure as Optimizers.
The runtime executes on demand. Installing it exposes `use-synth-jesterky` and
`synth_jesterky` MCP in newly prepared session homes. Stale homes remove the
skill on refresh; the host rejects preparation when the plugin is unavailable.
The core trace and annotation APIs remain independent.

`jesterky_prepare` wraps the generic snapshot selection with analysis defaults
`gpt-5.6-luna` / `low` and the required `jesterky` runner. The evaluated model is
unchanged. An installed host executable is also available through plugin
capabilities for local workflow development. A remote container must advertise
and contain its own compatible Jesterky runner; host install does not mutate it.

Only a catalog supplies artifact version/platform/URL/size/SHA-256. Downloads
are bounded, verified before replacement, and staged atomically. Removal only
deletes the runtime, retaining trace/annotation/query/visual evidence. Native
release packaging lives in Jesterky `scripts/package_workshop.py`.

**Release status:** the unauthenticated v0.1.3 GitHub release lookup returned
404 (private or unpublished); publication could not be verified. The official
catalog intentionally contains no artifacts. The native macOS arm64 artifact
has been built locally; its receipt is in Jesterky `dist/workshop`. Publishing
and filling the verified catalog remain release work. The UI reports this
unavailability instead of claiming a successful download.

Containers' runner partitions by actor/session and bounded event windows,
collects every terminal proposal, deduplicates identical findings while keeping
disagreements, and retains successful shard receipts for retry. Cancellation
and timeout kill the worker process group. Headless Jesterky writes a usage
sidecar; unknown usage/cost stay unknown. Real paid swarms require an enforcing
reservation proxy; post-hoc CLI budget telemetry is not cost enforcement.

## Repository responsibilities

- **Workshop:** job membership/configuration projection, annotation citation
  preservation, snapshots/paging/source/preparation IPC and MCP, optional
  Jesterky lifecycle/UI/skill, catalog visual supporting mixed query grains.
- **Containers:** typed V5 query/cache/source module, reward/annotation catalog
  facts, Jesterky partition/merge/retry/cancellation and Luna/low defaults.
- **Evals:** optional `research_context` TOML axes persist in existing
  `job_result.json`; missing axes stay missing. No new result store or job ID.
- **Jesterky:** headless telemetry sidecar, version reporting, native Workshop
  artifact packaging. Existing workflow/replay schemas remain unchanged.

## Acceptance evidence — 2026-09-07 implementation validation

The implementation is present across Workshop, Containers, Evals, and Jesterky.
This is **not a claim that all E1–E10 release gates are complete**. Existing
optimizer job identities and execution paths are reused; no new scheduler or
cohort database was added to Optimizers.

| Path | Evidence and qualification |
| --- | --- |
| Actual Craftax and DungeonGrid engines | Two seeds each; real isolated code-policy execution, sealed V5 archives, rewards and PNG frames. Annotations disabled during evaluation. `artifacts/trace-research-e2e/engines.json`. |
| Native Workshop launch/query/source/restart | `OptimizerService.start_recipe` launched the real Craftax and DungeonGrid servers (two seeds each), created its own jobs and collection rows, imported traces, queried by returned job IDs, resolved source, and reopened the saved query. No injected job rows; annotations/Jesterky/provider calls off. `native-launch/native-launch-acceptance.json` (36.47 s). |
| Live optional Jesterky | Explicit `openrouter/openai/gpt-5.6-luna`, low effort; one retained rollout from each environment, independent of an active annotated eval. Each sealed with three findings and six inspection-tool calls. Separate evidence bundles preserve source traces. `live-jesterky/receipt.json`. These factual smoke findings prove CLI/provider/tool wiring, not analytical quality. |
| Combined live evidence query | Six live findings, two matching reward-bearing episodes, exact citation readback, original snapshot unchanged. Production annotation projection and query authorities. `annotation-query-acceptance.json`. |
| Partial retry and cost enforcement | Completed actor/window outputs recovered from both final manifests and partial event journals; cross-job workspace retry reuses successful shards. Durable pre-request cost and token reservations persist across restart. Controlled failure-injection tests pass; the failed live attempts remain recorded. |
| Multi-agent authority | Live regression export preserves DungeonGrid `agent_0` and `agent_1` identities. Frame viewers remain subjects rather than invented actors. `actor-regression/engines.json`. |
| Retained scale | **450 synthetic sealed load fixtures**, including a 10,002-event trace with four action actors. Actual packaged CLI/native store yields 14,055 entities, bounded preparation batches, exact source read, and restart-stable snapshot. `scale-native-store/acceptance.json`. |
| Optional visual | Rebuilt isolated native custom-protocol app renders the four-result query catalog and retained DungeonGrid replay (`native-app-store/native-ui-acceptance.json`, screenshots). This exposed and fixed native snapshot loader and IPC dispatch gaps. Browser selection tests preserve exact targets. Native clicking follows the exact Craftax trace (166 events) and returns to the same four-result catalog. Catalog state restoration is covered in browser regression; the final native app additionally opens the exact independent finding and displays its producer/review state. Stale selectors and missing trace actions have controlled browser coverage. |
| Runtime distribution | Wheel-based Containers 0.4.2.dev20260903 and relocatable Python 3.12.11 staged with source/wheel hashes. Jesterky 0.1.3 macOS arm64 built with matching receipt. Official download, signing and other platform installation remain unverified. |
| RuneBench | Permanent reconstruction in `evals/containers/images/runebench-harbor-codex`. Nine reconstruction tests and bounded readiness regression pass. Final 18116 arms both completed real gameplay, scoring 225/188 **normalized XP/min**; the live harness passed in 475.60 s. Both sealed archives pass native import/query/source/selection/restart in 24.50 s. Typed reward pairing passes with a 37-point absolute difference, matching fixed-world repeat 780040 without inventing a random seed. `runebench/receipt.json`, `runebench/final-paired-query.json`, `runebench-final-native-store/acceptance.json`. A subsequent actual native Optimizers launch also passes; see the final native receipt below. Earlier failed and corrected-unit archives remain retained. |

The final scale run resumed 302 previously imported fixtures, imported the
remaining 148 with concurrency four in 50.2 seconds, queried all 450 episodes
in 8.50 seconds, and completed its query/source/reopen assertions in 74.2
seconds. This is **not** a clean-start 450-import benchmark. Earlier serial
imports and a disk-full failure are retained in the diagnostic record.

The 10,002-event inspector now uses a durable page index in the existing
projection cache, backed by verified immutable CAS chunks of at most 200 events.
Ingestion prepares this index for long traces. Reads load only the needed one or
two chunks; old explicit full-projection snapshots retain compatibility.

- Retained index preparation: 34.815 s; first page 36 ms, final page 30 ms after
  reopening; first response 162,356 bytes. Cross-page reads and damaged/missing
  pages, wrong identities, and read bounds have passing tests.
- A fresh isolated import took 39.973 s, including index construction. After
  closing/reopening the store, the first interaction took 30 ms.
- Final rebuilt native app: first usable 200-event page in 3.139 s, including
  open, rendering, capture and polling. Actual next/previous clicks passed with
  all four actors. Whole resource-coalition RSS, including WebKit helpers, started
  at 784,832 KiB and peaked at 1,017,600 KiB over 40 samples.
  This sums RSS and can count shared pages more than once; it is not physical
  footprint. A preceding run peaked at 1,788,608 KiB. Memory variability warrants
  follow-up despite bounded payloads and responsive paging.

`scale-native-store/window-measurement.json`, `cold-import-acceptance.json`, and
`native-window-acceptance.json` retain those measurements. Initial indexing still
materializes the verified projection once; it is moved out of first interaction
for newly imported long traces, not eliminated.

Native ordinary campaigns now persist their remote campaign/job identities for
reconciliation; previously standalone submission only handled reservations and
left free jobs invisible to that worker. Actual app IPC submits one selected
Craftax rollout and one selected DungeonGrid rollout, automatically reconciles
one deterministic finding each, and returns exactly those two episodes through
annotation/reward filtering. No provider calls or Jesterky. Unselected rollouts
and recorded rewards remain unchanged. See `native-launch/native-campaign-acceptance.json`.

Native grouped reward queries and their denominators, optional comparison visual,
annotation source lookup, and old saved snapshot preservation pass. The new Craftax/DungeonGrid exports include typed terminal reward definitions and
pinned engine versions. Four actual native job pairs now produce verified deltas.
Legacy archives without those facts still report unknown semantics; no
cross-environment deltas are invented. Matching also reads a unique current, valid, trace-wide typed reward definition
when it agrees exactly with the stored scalar score. Two retained real RuneBench
traces align by task/repeat/definition: either a seed or explicit repeat is
required. The retained failed execution remains invalid with no delta; the final
two completed arms produce a verified numeric delta. Eight focused research
query tests pass. Earlier ordinary annotation and
accept/reject review tests also preserve old answers/source across restart.

### Remaining release gates

1. Native Optimizers RuneBench launch is now verified. Actual job
   `opt_eval_runebench_676598085674` completed in 430.84 s, scoring 212 normalized
   XP/min. Query by that existing job ID returns the sealed trace, model/effort,
   exact runtime image digest, task `rb-woodcutting-xp-5m-780040`, repeat 780040,
   and explicit null seed. Source read and store reopen pass. No injected job rows,
   annotations or Jesterky were required. Receipt:
   `native-runebench/native-launch-acceptance.json`. Source remains permanently in
   `evals/containers/images/runebench-harbor-codex`, frozen launch revision 18118.
   Earlier failed assertions and their paid usage are retained, not overwritten.
2. Publish verified Jesterky artifacts, complete required signing, populate the
   official catalog only after URLs serve matching bytes, and smoke-test supported
   host/remote platforms. The release source, tag v0.1.3, and workflow were pushed using authorized SSH.
   Project-local GitHub App credentials only grant contents read, not release
   write. The workflow is registered, but no Actions run or release asset exists.
   Linux ARM64 and x86_64 tests, release packaging, hash/version/trace validation
   pass in pinned containers (x86_64 emulated). macOS ARM64 packaging passes but
   remains unsigned/unnotarized. No release PAT or Apple credential was found in
   65 project-local env files. Keychain access was not used; explicit approval
   for that release-only search is pending. The official catalog remains empty.
3. Complete installed-release acceptance after publication: official catalog download,
   verification, enable/disable and remote distribution on supported targets. Craftax/DungeonGrid typed exports and compatible native cross-job paired
   deltas now pass; legacy missing semantics remain unknown. Native independent campaigns, annotation/reward
   source queries, bounded native replay, and live RuneBench typed pairs pass.

Provider work used the authorized project-local `.env`, the explicit OpenRouter
route, and enforcing budget proxies; no Keychain access. The announced aggregate
maximum was $20, expected actual below $3. Actual account billing remains unknown.
The final successful RuneBench pair reports $0.030262 in proxy usage; earlier direct-facade
RuneBench attempts total $0.109609 in proxy-reported usage. Final
capabilities are revoked, and all recorded attempt capabilities are non-usable. Zero-use startup attempts have durable reservation-release receipts.
The current reservation ledger is authoritative and stays within the aggregate $20.
Proxy token/cost usage is debited after a response, so a final request can cross
its token cap before the next request is refused; call reservations happen before
requests. This is not a claim of a strict pre-response token limit.
Durable conservative reservation ledgers and the pinned provider price response
are retained; reservation maxima must not be presented as actual charges.

## Independent annotation of existing rollouts

Jesterky's page now has a durable **Analysis scope** setting: entire selected
rollouts (default), or selected events only. It controls `jesterky_prepare`,
which also accepts a per-request `annotation_scope` override. The setting can
be read with `jesterky_settings`; changing it starts no compute.

Selections may span existing eval jobs. Preparation resolves immutable query
result IDs, deduplicates the sealed traces, and returns explicit per-container
`campaignSelections` for whole-rollout analysis. No current annotated eval job,
eval rerun, or inherited live annotation protocol is required. Add compatible
annotators and bounded limits before estimating/starting the existing annotation
campaign API. Event-only analysis returns per-target event metadata and rejects
selections it cannot scope exactly. Luna/low remain independent defaults.

### Local validation record

- Containers: 31 focused research/Jesterky/bounds/portable-bundle tests passed.
- Workshop: six research tests, adapter path authority, native four-rollout
  import/query/reopen, live evidence projection/query, and two scale/long-visual
  native acceptance tests passed. Existing plugin/MCP/scope/projection suites
  also passed during implementation.
- Browser: 14 query/catalog/annotation/contract tests, the optional-plugin saved
  scope test, and the retained 10,002-item visual test passed.
- Evals: two context/provenance/missingness validation tests passed.
- Jesterky: core/model/proxy/quality/CLI suites passed; two durable budget
  regression tests passed. Live provider/tool smoke sealed both selected traces.
- Generated command bindings, TypeScript and frontend production build passed.
  The duplicate `HumanAnnotationSessionView` authority and fake quality-result
  schema failures previously listed here were fixed and rechecked.

Evidence root: `artifacts/trace-research-e2e`. Source dependencies are dirty local
checkouts, not a published release. `validation-manifest.json` records repository
heads and relevant source fingerprints. Jesterky binary receipt SHA-256:
`d3e8f3e4cd63f88cd6fe037bf6abb36a0271e9701811d1ab32e390007b804e53`.

### Additional final-build acceptance (September 8)

- Current production native Craftax/DungeonGrid launcher: four rollouts passed,
  followed by independent annotation campaigns, grouped comparison/source queries
  and actual app restart. No fabricated launch rows are used for this proof.
- Provider receipt accounting accepts accumulated per-request micro-dollar
  rounding but rejects material deficits. An absent grader lane remains absent,
  preserving authoritative provider cost in the visual. Four focused tests pass.
- Retained 10,002-event/four-actor Jesterky partitioning and interrupted-work
  recovery pass both manifest and truncated-journal paths: six controlled tests,
  complementing the separately retained live Luna/low runner smoke.
- All three Jesterky release targets pass their selected core/model/proxy/quality/CLI
  suites and packaged executable checks; these are local platform results, not
  proof that an end user can download official published assets.

- Stable pagination mutation acceptance passes over 450 retained results. Importing
  a real archive and adding controlled fixture membership between pages leaves
  the pinned 200/200/50 pages unchanged; refreshed query returns 451 results.
  Reopening the store preserves the original IDs and digest. This is explicitly
  a snapshot-isolation fixture, separate from actual native eval-launch proof.
- Final retained Chromium projection/selection test passes with 10,002 events
  and four actors. Native replay uses bounded 200-event transport windows.

### Independent annotations in retained replay

Final native interaction exposed a missing consumer connection: queryable findings
from independent campaigns were absent from sealed replay. Replay now adds a
bounded, separately identified annotation overlay without changing sealed trace
bytes or rewards. Opening a view captures findings and current review decisions
in its CAS snapshot. Paging keeps that annotation state; reopening refreshes it.
The overlay preserves exact selectors, producer IDs and supersession, merges with
embedded findings, and explicitly reports partial coverage at 200 findings or
256 KiB. Target resolution is limited to the displayed event window.

Two native regressions pass for review changes across pinned pages and damaged
chunk rejection; browser overlay/window tests, TypeScript and frontend production
build pass. The actual native app displays one current independent annotation; clicking its marker opens the expected text, producer and review state. `native-launch/final-native-overlay-acceptance.json` records this final-build check.

Final native RuneBench run: 23 provider calls, $0.014922 proxy-reported usage.
All four native attempts total $0.064384 proxy-reported usage; their capabilities
are revoked. Native maximum reservations are $1.25, within the announced aggregate
$20. These are distinct from earlier direct-facade usage and are not account billing.

### Local installation completed

User authorized a local installation instead of waiting for release credentials.
Registered the verified macOS ARM64 artifact in the development-build catalog,
selected that channel through native Workshop settings, and installed through the
normal plugin lifecycle. Jesterky 0.1.3 is enabled and Ready in the verified
Workshop Trace Acceptance app, using the retained native-launch store. Executable
SHA/version and trace validation pass. Actual stdio MCP advertises preparation
and settings, and prepares selections from two existing eval jobs. Capabilities
expose the skill, trace catalog and rollout inspector, Luna/low defaults, and
whole-rollout scope. Disable/enable gating and actual app restart pass.
No provider calls or Keychain access were needed. `local-jesterky-install.json`
and `local-jesterky-mcp.json` retain evidence. This does not publish official
downloads or install a runtime in remote containers.

### Completed remaining local scope (September 8)

Craftax and DungeonGrid now declare separate, versioned terminal reward definitions
in their target specifications. Containers seals the actual aggregate as a typed
trace-wide evaluation with provenance; missing rewards remain unscored. Rollout
pins persist the environment version across restart instead of borrowing the
current server's version. The native harness hashes the engine executable (and
DungeonGrid scenarios), and runs its own permanently built DungeonGrid executable
under `evals/workshop/runtime-builds/dungeongrid`, leaving the existing user server
untouched. Paired comparisons require an explicit environment version.

Four actual native Optimizers jobs, eight rollouts and four paired comparisons
pass reward-definition, environment-version, denominator, scalar/typed agreement,
source-read and optional visual checks (`native-launch/typed-pair-acceptance.json`).
Native UI displays both DungeonGrid seed pairs as matched. Two independent
annotation campaigns on these new rollouts pass annotation filtering and preserve
rewards (`typed-reward-campaign.log`). These runs used no provider calls.

Recorded acknowledgement queries resolve to exact coordination records. Repeated
failed-action relationships require recorded actor, session and ordering evidence;
missing relationships explicitly mean unknown. The focused query and portable
bundle suites pass 21 tests (`remaining-scope-query-tests.log`).

The Jesterky container installer accepts an explicitly supplied local artifact
while still validating platform, receipt size and SHA before atomic installation.
Actual installations, executable version and trace validation pass in Linux ARM64
and emulated x86_64 containers (`jesterky-platforms/*/local-install.log`). This
complements the native macOS plugin lifecycle and MCP acceptance. Official public
release distribution remains deferred; it is not required for these local paths.

Actual app restart also passes the typed native pair checks, saved snapshot reads,
grouped reward queries and annotation-source resolution (`typed-pairs-restart.log`
and `typed-comparison-restart.log`). All owned native test engines are stopped.
