# v0.4 readiness

Status: **implementation complete; promotion held at explicit acceptance gates**.

The source consolidation, Banking77 producer contract, frozen-head automated suites, exact friends artifact, and website release catalog are complete. Do not merge, tag, upload, or activate the production catalog until both remaining CUA gates are recorded:

1. Launch and inspect the isolated installed artifact.
2. Approve and run the paid Banking77 GEPA acceptance with a declared spend cap.

After those pass, promote every v0.4 branch to `dev`, promote the reconciled state to `main`, tag the release, upload the ZIP at its source-owned URL, activate `SYNTH_DESKTOP_STABLE_VERSION=0.4.0`, verify production, and only then create the v0.5 implementation branch.
