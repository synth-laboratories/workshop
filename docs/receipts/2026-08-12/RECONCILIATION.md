# Aug 12 receipt reconciliation

Two evidence waves exist and answer different questions. They must not overwrite each
other.

The committed `docs/receipts/2026-08-12` wave exercised the live app and produced the
first end-to-end passes, including dual GEPA. A later strict clean-root rerun correctly
found that the SFT single-accelerator proof did not demonstrate real occupancy and that
two checkpoint campaigns could collide on one Banking77 façade. It also could not
repeat paid GEPA because the clean process did not receive provider credentials.

Therefore:

- A3 remains a valid in-app functional receipt, but not a clean-tip paid
  re-certification.
- A4 is not accepted from the older receipt alone; the stricter queue/isolation defects
  are product work and are fixed only on the SFT/Containers completion branches.
- A6 remains incomplete until hosted checkpoint sampling produces non-null scores in a
  real campaign. A sampler refusal is still null, never zero.
- A2 and A8 remain external because Docker and the dig.bench token were unavailable.

Future receipts should be written by `scripts/modern_stack_dogfood.py`. It preserves the
requested stream, exact bound descriptor, cursor transcript, event-kind counts, run
manifest, cost truth, Trace V5 state, screenshots, and CUA findings in one directory.
Do not replace a failed or blocked receipt with prose; retain both waves and link the
newer result.
