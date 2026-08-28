# GELO / Go-Ex

Use GELO for hosted exploration, theme/board evolution, and optional local-slot execution. There is no arbitrary local GELO launcher: start it through the hosted optimizer service, or import/reconcile an existing run.

1. Use `list_runs` to find a mirrored run or `reconcile_cloud` with its optimizer run ID.
2. Follow events from the persisted cursor.
3. Inspect `go-ex.board`, `go-ex.themes`, and `go-ex.data_engine`, plus `run.execution`, `run.usage`, and `run.artifacts`.
4. Report explored cells/themes, best observed objective, container/slot binding when present, status, usage, and artifacts.

Do not claim a hosted run is locally resumable. Reconciliation mirrors external truth; cancellation is a deliberate external mutation and requires the user's request.
