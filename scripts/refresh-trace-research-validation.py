#!/usr/bin/env python3
"""Refresh local source/evidence hashes without claiming unfinished release gates."""
from pathlib import Path
import datetime,hashlib,json,subprocess
repo=Path(__file__).resolve().parents[1];root=repo/'artifacts/trace-research-e2e'
p=root/'validation-manifest.json';manifest=json.loads(p.read_text())
def sha(path):return hashlib.file_digest(path.open('rb'),'sha256').hexdigest()
extra={
 'containers':['tests/test_trace_v5_annotation_jesterky.py','src/synth_containers/platform/state.py','tests/test_isolated_advertised_policy.py','src/synth_containers/tracing/research.py','tests/test_trace_research.py'],
 'jesterky':['Cargo.lock','.github/workflows/workshop-release.yml','scripts/package_workshop.py','scripts/verify_workshop_package.py'],
 'workshop':['apps/synth_desktop/src-tauri/src/optimizers/kernel/algorithms/eval.rs','apps/synth_desktop/src-tauri/src/trace_research.rs','apps/synth_desktop/src-tauri/src/optimizers/container_eval.rs','apps/synth_desktop/src-tauri/src/secrets/mod.rs','apps/synth_desktop/src-tauri/src/data.rs','apps/synth_desktop/src-tauri/src/annotations_ipc.rs','apps/synth_desktop/src-tauri/src/optimizers/native_research_e2e.rs','apps/synth_desktop/src-tauri/tests/trace_research_e2e.rs','scripts/trace-research-native-launch-servers.py','scripts/trace-research-native-campaign.py','scripts/trace-research-native-comparison.py','scripts/trace-research-retained-pairs.py','scripts/trace-research-capability-audit.py','scripts/trace-research-ordinary-annotations.py','apps/synth_desktop/src-tauri/src/visuals_ipc.rs','apps/synth_desktop/src-tauri/src/lib.rs','apps/synth_desktop/src-tauri/src/storage/content_store.rs','apps/synth_desktop/src-tauri/src/secrets/research_e2e.rs','apps/synth_desktop/src/renderer/src/components/VisualsPage.tsx','visuals/families/analysis/trace.rollout_inspector.v1/shell.tsx','visuals/families/analysis/trace.catalog.v1/shell.tsx','visuals/tests/trace_window.test.mjs','scripts/trace-research-native-window.py','scripts/trace-research-native-ui.py','scripts/trace-research-runebench-receipts.py','docs/engineering/TRACE_RESEARCH_IMPLEMENTATION_2026-09-07.md'],
 'evals':['containers/images/runebench-harbor-codex/'+name for name in ['SOURCE.json','start.py','Dockerfile','runtime.Dockerfile','harbor.amd64.compose.yaml','README.md','agents/codex_adapter.py','agents/policy_bundle.py','agents/model_route.py','src/runebench_harbor_container/app.py','scripts/build_trace_bundle.py','scripts/correct_retained_reward_units.py','scripts/test_readiness.mjs','shared/workshop_readiness.ts','tests_reconstruction.py']],
}
extra['containers'] += ['src/synth_containers/platform/targets.py','src/synth_containers/platform/app.py','src/synth_containers/platform/trace_bundle.py','tests/test_harbor_trace_bundle.py']
extra['evals'] += ['containers/images/craftax-gamebench-rust/craftax_gold/targets.py','containers/images/dungeongrid-gold/dungeongrid_gold/targets.py']
extra['jesterky'] += ['scripts/install_container_runtime.py']
extra['workshop'] += ['scripts/trace-research-native-typed-pairs.py']
for name,record in manifest['repositories'].items():
 checkout=repo.parent/name
 record['head']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=checkout,text=True).strip()
 record['dirty']=bool(subprocess.check_output(['git','status','--porcelain'],cwd=checkout,text=True))
 for relative in set(record['fileSha256'])|set(extra.get(name,[])):
  source=checkout/relative
  if source.is_file():record['fileSha256'][relative]=sha(source)
receipts=set(manifest['receipts'])|{
 'native-launch/native-launch-acceptance.json','native-launch/native-campaign-acceptance.json','native-launch/native-comparison-acceptance.json','runebench/retained-paired-query.json','retained-pairs-test.log','jesterky-public-release-check.json','final-containers-research-tests.log','reward-definition-query-tests.log','comparison-native-restart-test.log','scale-native-store/cold-import-acceptance.json','window-chunk-integrity.log','window-index-test.log','window-cold-import-test.log','native-launch-test.log','isolated-policy-tests.log','campaign-native-test.log','native-app-store/native-ui-acceptance.json','native-app-store/review-query-acceptance.json',
 'scale-native-store/window-measurement.json','scale-native-store/native-window-acceptance.json',
 'runebench-proxy/final-capability-audit.json','runebench/recovered-receipt.json','runebench/receipt.json','runebench/corrected-units-receipt.json','ordinary-annotations/receipt.json','ordinary-annotation-query-acceptance.json',
 'runebench-recovered-native-store/acceptance.json','runebench-ready-native-store/acceptance.json',
 'window-native-final.log','window-browser.log','window-typecheck.log','window-frontend-build.log',
 'runebench-readiness-retry-live.log','review-query-native.log',
 'runebench-final-live.log','runebench-final-native.log','final-pairs-test.log',
 'runebench/final-paired-query.json','runebench/engines-final.json','runebench-final-native-store/acceptance.json',
}
receipts.update({'local-jesterky-install.json','local-jesterky-mcp.json','final-acceptance-status.json','annotation-overlay-native-tests.log','annotation-overlay-browser-tests.log','annotation-overlay-typecheck.log','annotation-overlay-frontend-build.log','native-launch/final-native-catalog.json','native-launch/final-native-overlay-acceptance.json','jesterky-platforms/acceptance.json','null-seed-regression.log','null-seed-query-regression.log','native-runebench-null-seed-live.log','platform-annotation-visual-tests.log','native-runebench/final-capability-audit.json','native-runebench/native-launch-acceptance.json','native-runebench/provider-usage.json','native-runebench-verified-live.log','provider-rounding-tests.log','native-launch-current-test.log','campaign-final-native-test.log','comparison-final-native-test.log','comparison-final-native-restart-test.log','long-jesterky-recovery-tests.log','platform-visual-tests.log','platform-retained-visual-tests.log','pagination-mutation-tests.log','scale-mutation-native-store/acceptance.json','final-native-build-receipt.json','release-access-audit.json','jesterky-platforms/candidate-catalog.json','jesterky-platforms/amd64/build.log','jesterky-platforms/arm64/build-v2.log'})
receipts.update(str(x.relative_to(root)) for x in (root/'runebench-proxy').glob('*.unused-reservation.json'))
receipts.update({'native-launch/typed-pair-acceptance.json','native-typed-pairs.log','typed-pairs-restart.log','typed-comparison-restart.log','typed-reward-campaign.log','remaining-scope-query-tests.log','typed-terminal-reward-tests.log','coordination-query-acceptance.log','dungeongrid-owned-build.log','jesterky-platforms/arm64/local-install.log','jesterky-platforms/amd64/local-install.log'})
receipts.update(str(x.relative_to(root)) for pattern in ['typed-reward-arm-*/*.json','*-environment-version.json'] for x in (root/'native-launch').glob(pattern))
for relative in receipts:
 evidence=root/relative
 if evidence.is_file():manifest['receipts'][relative]={'sha256':sha(evidence)}
rune=repo.parent/'evals/containers/images/runebench-harbor-codex'
manifest['runebenchSource']={'permanentRoot':str(rune),'launchedRevision':json.loads((rune/'.workshop/instances/18118/runtime-receipt.json').read_text()),'reconstructionTestsSha256':sha(rune/'.workshop/reconstruction-tests.log'),'readinessRegressionSha256':sha(rune/'.workshop/readiness-regression.log')}
manifest['recordedAt']=datetime.datetime.now(datetime.timezone.utc).isoformat()
manifest['releaseStatus']='dirty local implementation; complete E1-E10 release acceptance not yet established'
manifest['providerBudget']['nativeRunebenchReservedMicros']=json.loads((root/'native-runebench/budget.json').read_text())
manifest['providerBudget']['runebenchReservedMicros']=json.loads((root/'runebench-proxy/reconstruction-budget.json').read_text())
p.write_text(json.dumps(manifest,indent=2)+'\n');print('Updated local source and retained evidence manifest')
