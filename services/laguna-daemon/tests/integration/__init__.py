"""Integration suites that require a live daemon.

These are discovered by the normal test run but skip themselves unless
`SYNTH_LAGUNA_LIVE_BASE_URL` is set, so the deterministic suite stays fast and
hermetic while the live checks remain one environment variable away.
"""
