export type {
  VisualBinding,
  VisualBindingKind,
  VisualBindings,
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

export { DEFAULT_CHROME } from "./types.ts";
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
