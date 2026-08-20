# v0.7 test report (stub — rows owned by the integrator)

The Workshop stack integrator writes the rows at the end of the v0.7 merge train. This stub fixes the format only. Every row is a command that ran on a named SHA with counts; a compile-only pass, a skipped suite, or a count without a command is not a row.

## Row format

| Repo | SHA | Command (exact) | Passed | Failed | Skipped/ignored (reason) | Notes (pre-existing failures by name, flakes with measured rate) |
|---|---|---|---|---|---|---|

## Passed

(integrator fills)

## Observations

(integrator fills — e.g. the `synth_gepa` `service_ownership` flake, KNOWN_ISSUES K12, 4/30 on `d3c9edd`; the backend full-suite known-failure set of 25)

## External acceptance boundaries

(integrator fills — paid runs, deploys, and notarization that were not performed, each with the decision id D2/D4/D8 that gates it)
