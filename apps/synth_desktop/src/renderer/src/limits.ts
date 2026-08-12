/** Named renderer timeouts / poll intervals — keep in sync with Rust `limits.rs` intent. */

/** Default eval wait_for_terminal budget when callers omit timeoutMs. */
export const EVAL_WAIT_TERMINAL_TIMEOUT_MS = 600_000;

/** Default poll cadence for wait_for_terminal. */
export const EVAL_WAIT_TERMINAL_POLL_MS = 500;

/** Inventory container status poll. */
export const CONTAINER_POLL_MS = 15_000;

/** Model observability auto-refresh. */
export const MODEL_OBSERVABILITY_REFRESH_MS = 15_000;

/** Toast auto-dismiss. */
export const TOAST_DISMISS_MS = 2_200;

/** Account summary quiet re-fetch after auth transitions. */
export const ACCOUNT_REFRESH_DELAY_MS = 4_000;

/** Clipboard "copied" affordance. */
export const COPY_ACK_MS = 1_600;
