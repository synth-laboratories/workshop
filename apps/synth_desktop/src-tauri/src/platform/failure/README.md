# Failure platform

Owns failure identity, lifecycle, persistence, causality, query, redaction, and delivery.

Does **not** own:

- container, evaluation, or session meaning (domain authorities)
- renderer presentation policy
- VictoriaLogs indexing (diagnostics)
- executable recovery callbacks

Public facade: `crate::platform::failure`. Persistence goes through `FailureRepository` only.
