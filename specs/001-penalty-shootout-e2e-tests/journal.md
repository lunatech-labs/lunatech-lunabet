# Journal — 001-penalty-shootout-e2e-tests

Append-only record of decisions, drift, and critic verdicts.

## Gate 1 (SPECIFIED)
- Spec approved. Added AC6 (full suite `cargo test --bin lunatech-betting` stays
  green, no existing test modified) at user request.

## Gate 2 (PLANNED)
- Plan approved. Seam decision: **option (a)** — tests live inside
  `football_data.rs`'s `#[cfg(test)] mod tests`, calling
  `crate::scoring::compute_points` across the module boundary. Zero production
  surface added. Rejected the `pub` bridge fn as a production addition in tension
  with the "no production behavior changes" boundary.
- Resolved AC1 fullTime discrepancy (7-6 → 6-4) to match the committed fixture;
  dropped the now-stale open question at user request.
- Recorded deferred follow-up: no parity guard between `compute_points` and
  `recompute_all`'s SQL (true payout = base × joker multiplier). Critic to note,
  not fail on, this residual seam gap.
