export type {
  VisualBinding,
  VisualBindingKind,
  VisualBindings,
  VisualComponentMeta,
  VisualInstance,
  VisualTemplate,
  VisualTemplateMeta,
  VisualTemplateSlot,
  LiveEvalEvent,
  RolloutStep,
  EvalMatrixPoint,
  TraceAnnotationMarker,
  VisualChromeTheme
} from "./types.ts";

export {
  DEFAULT_CHROME,
  bindingInputName,
  bindingList,
  resolveInputName,
  stampBindingInput,
  templateInputs
} from "./types.ts";
export {
  COMPOSE_COMPONENTS,
  COMPOSE_EVENT_STREAM_INPUTS,
  COMPOSE_EVENT_STREAM_SLOTS,
  COMPOSE_SPEC_SCHEMA,
  composeComponentEmitsCursor,
  composeConsumesOptimizerRun,
  composeConsumesStreamOrOptimizer,
  composeEventStreamSlot,
  composePlacementNeedsOptimizerRun,
  composePlacementNeedsStream,
  isComposeComponentId,
  isComposeEventStreamSlot,
  parseComposeSpec
} from "./composeSpec.ts";
export type {
  ComposeComponentId,
  ComposeEventStreamSlot,
  ComposePlacement,
  ComposeSpec,
  ComposeSpecResult
} from "./composeSpec.ts";
export {
  OPTIMIZER_EVENT_SCHEMA,
  looksLikeEvalTrace,
  optimizerEventsToLiveEval
} from "./optimizerCompose.ts";
export type { OptimizerComposeResult } from "./optimizerCompose.ts";
export {
  bindTemplateSlots,
  subscribeLiveSlot,
  asEvalMatrixPoints,
  asRolloutSteps,
  asLiveEvents,
  asAnnotationMarkers,
  createJsonFixtureLoader,
  isVisualBindings,
  bindingSlots,
  propsFromBindings,
  resolveVisualBindings
} from "./bind.ts";
export type { ResolvedVisualBindings, VisualBindingsStatus } from "./bind.ts";
export {
  selectRenderedProjection,
  rememberLastKnownGood
} from "./lastKnownGood.ts";
export type { ProjectionSource, SelectedProjection } from "./lastKnownGood.ts";
export {
  consumeInjectedRendererCrash,
  resetInjectedRendererCrashes
} from "./crashInject.ts";
export { presentRuntimeError, presentRuntimeErrorMessage } from "./presentError.ts";
export type { PresentedRuntimeError } from "./presentError.ts";
export { captureEvidenceKind, CAPTURE_REVIEW_PRODUCT_CLASSES } from "./captureEvidence.ts";
export type { CaptureEvidenceKind } from "./captureEvidence.ts";
export {
  decideVisualEvidence,
  visualEvidenceBlocksCompletion,
  VISUAL_EVIDENCE_STATES
} from "./visualEvidence.ts";
export type { VisualEvidence, VisualEvidenceState, VisualLifecycleFacets } from "./visualEvidence.ts";
export {
  createReplayClient,
  parseReplayPage,
  replayStreamsFromBindings,
  REPLAY_FIRST_RESPONSE_TIMEOUT_MS,
  REPLAY_PAGE_LIMIT,
  REPLAY_PAGE_LIMIT_MAX
} from "./replayClient.ts";
export type {
  LiveTemplateProps,
  ReplayClient,
  ReplayCursor,
  ReplayPage,
  ReplayStream,
  TransportState
} from "./replayClient.ts";
export {
  LIVE_EVAL_SLOT,
  FORBIDDEN_LIVE_EVAL_SLOTS,
  assertLiveEvalSlot,
  assertDeclaredStreamSource,
  ingestLiveEnvelope,
  ingestLiveEnvelopes,
  eventMatchesIncludeKinds,
  formatMissingNumber,
  formatMissingUsd,
  isGuessedStreamUrl,
  isNeverDeclaredStreamUrl
} from "./liveStream.ts";
export { projectLiveEval, displayReward, rewardFromEnvStatus } from "./liveEvalReducer.ts";
export { projectAgentTurns, reconcileCallSelection, callForSequence } from "./agentTranscript.ts";
export type { AgentTurnProjection, ModelCall, EvidenceField, EvidenceState } from "./agentTranscript.ts";
export {
  saveVisualInstanceTsx,
  renderInstanceTsx,
  markInstanceSaved
} from "./save_tsx.ts";
export {
  compileSourcedModule,
  isSourcedTemplate,
  sourcedInvalidShell,
  SOURCED_ALLOWED_IMPORTS,
  SOURCED_KIND,
  SOURCED_PROTOCOL,
  SOURCED_TEMPLATE_ID,
  validateSourcedSource
} from "./sourcedVisual.ts";
