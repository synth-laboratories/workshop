/** Shared Workshop v0.6 distribution, identity, telemetry, and usage contracts. */

export type IdentityState =
  | { kind: "local_only" }
  | { kind: "signed_out" }
  | { kind: "authenticating" }
  | {
      kind: "signed_in";
      accountId: string;
      workspaceId?: string;
      sessionExpiresAt: string;
      entitlements: EntitlementSummary[];
    }
  | { kind: "refresh_required"; reason: "expired" | "revoked" }
  | { kind: "unavailable"; retryable: boolean; errorClass: string };

export type EntitlementSummary = {
  service: string;
  capability: string;
  enabled: boolean;
};

export type EntitlementDecision = {
  allowed: boolean;
  service: string;
  capability: string;
  reason?: "signin_required" | "not_entitled" | "quota_exhausted";
  limit?: number;
  remaining?: number;
  resetAt?: string;
};

export const allowedTelemetryEvents = [
  "download_initiated",
  "download_served",
  "app_first_launch",
  "signup_completed",
  "signin_completed",
  "signout_completed",
  "workflow_started",
  "workflow_terminal",
  "local_activation_completed",
  "hosted_activation_completed",
  "artifact_created",
  "recipe_saved",
  "report_created",
  "report_published",
  "recovery_attempted",
  "recovery_succeeded",
] as const;

export type AllowedTelemetryEvent = (typeof allowedTelemetryEvents)[number];

export type TelemetryEvent = {
  schemaVersion: 1;
  eventId: string;
  eventName: AllowedTelemetryEvent;
  occurredAt: string;
  appVersion: string;
  releaseChannel: string;
  platform: string;
  architecture: string;
  installationId?: string;
  accountPseudonym?: string;
  workflowFamily?: string;
  durationMs?: number;
  outcome?: "success" | "failure" | "cancelled";
  errorClass?: string;
  collectionPolicyVersion: string;
};

export type UsageRecord = {
  schemaVersion: 1;
  recordId: string;
  idempotencyKey: string;
  accountId: string;
  workspaceId?: string;
  projectId?: string;
  runId?: string;
  service: string;
  provider?: string;
  model?: string;
  workflowFamily?: string;
  occurredAt: string;
  receivedAt: string;
  quantity: string;
  unit: "request" | "token" | "compute_second" | "job" | "usd";
  priceVersion?: string;
  costUsd?: string;
  state: "pending" | "finalized" | "corrected";
  correctsRecordId?: string;
};

export type UsageTotal = {
  service: string;
  workflowFamily?: string;
  provider?: string;
  model?: string;
  quantity: string;
  unit: UsageRecord["unit"];
  costUsd?: string;
  state: "pending" | "finalized";
};

export type UsageLimit = {
  service: string;
  capability: string;
  limit: string;
  remaining: string;
  unit: UsageRecord["unit"];
  resetAt: string;
};

export type UsageAggregate = {
  accountId: string;
  workspaceId?: string;
  period: { start: string; end: string; timezone: string };
  generatedAt: string;
  freshness: "fresh" | "delayed" | "stale";
  pendingRecords: number;
  totals: UsageTotal[];
  limits: UsageLimit[];
};

export type UsageAggregateResponse =
  | { kind: "available"; aggregate: UsageAggregate }
  | { kind: "unavailable"; retryable: boolean; errorClass: string };
