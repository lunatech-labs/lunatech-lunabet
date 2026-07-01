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

## Phase 3 — Implement
- **T1 — critic PASS.** `score_bet` helper + `shootout_after_a_draw_scores_the_120_minute_draw`
  added to `football_data.rs` test module. Critic verified via `git diff HEAD`
  (32 additive lines, only the test module; `scoring.rs`/`main.rs` untouched, no
  new `pub` fn) and own run: 42 passed, 0 failed. AC1, AC5 satisfied.
- **T2 — critic PASS.** `goalless_shootout_scores_the_nil_nil_not_the_penalty_count`
  added (reuses `score_bet`). Red-capable guard proven empirically: critic flipped
  `settled_score` to return `fullTime`, test panicked (`left: 0, right: 3` at
  football_data.rs:288), then reverted cleanly — 43 passed, diff shows only the
  additive test. AC2, AC3, AC5 satisfied.
