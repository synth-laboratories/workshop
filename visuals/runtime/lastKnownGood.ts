/**
 * Last-known-good projection selection for a rendered visual.
 *
 * Live rendering can fail at the same identity and revision. The host must keep
 * showing the last successful projection rather than blanking the pane, and a
 * retry remounts the same visual instead of requiring a new revision.
 */

export type ProjectionSource = "live" | "lastKnownGood";

export type SelectedProjection<T> = {
  projection: T | null;
  source: ProjectionSource | null;
  stale: boolean;
};

export function selectRenderedProjection<T>(args: {
  live: T | null;
  lastKnownGood: T | null;
  liveFailed: boolean;
}): SelectedProjection<T> {
  if (args.live != null && !args.liveFailed) {
    return { projection: args.live, source: "live", stale: false };
  }
  if (args.lastKnownGood != null) {
    return { projection: args.lastKnownGood, source: "lastKnownGood", stale: true };
  }
  return {
    projection: args.live,
    source: args.live != null ? "live" : null,
    stale: false
  };
}

/** Keep a successful live projection; never replace it with a failed live value. */
export function rememberLastKnownGood<T>(current: T | null, live: T | null, liveFailed: boolean): T | null {
  if (!liveFailed && live != null) return live;
  return current;
}
