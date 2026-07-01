# Plan: Penalty-shootout scoring end-to-end regression tests

> For spec: specs/001-penalty-shootout-e2e-tests/spec.md
> Status: PLANNED (awaiting Gate 2 approval)

## Technical approach

- The chain under test is a two-seam pipeline that exists in production but is
  not wired together anywhere: a real football-data.org JSON payload is
  deserialized into `ApiScore` (serde), reduced to the settled 120-minute score
  by the private `ApiScore::settled_score()` in `src/football_data.rs`, and that
  settled `(Option<i32>, Option<i32>)` is fed with a user's bet into the public
  `compute_points()` in `src/scoring.rs`, which returns base points 3 / 1 / 0.
  The tests reproduce that chain and assert the points a user actually receives.
- Only one file is touched: `src/football_data.rs`, inside its existing
  `#[cfg(test)] mod tests` block (which already owns the inline-JSON decode
  style and the `settled(json)` helper). No production code, no new files, no
  fixtures, no HTTP mocking.
- A small test-only helper composes the chain so each test reads as
  payload plus bet plus expected literal:
  `score_bet(json, bet_home, bet_away) -> i32`, implemented as
  `let (h, a) = settled(json); crate::scoring::compute_points(bet_home, bet_away, h.unwrap(), a.unwrap())`.
  It reuses the existing `settled` helper (real serde decode) and calls the
  public `compute_points` across the module boundary. Expected point totals are
  written as bare literals (3 / 1 / 0), never derived from the code under test.
- Reuse the four documented Euro 2024 payloads already present in the test
  module (England 1-1 Switzerland shootout, Portugal 0-0 Slovenia goalless
  shootout, an EXTRA_TIME 2-1, and a REGULAR 0-2) so the assumptions about the
  feed's real semantics stay pinned to values that are already asserting the
  documented contract. New assertions layer the bet-plus-points dimension on
  top of the already-verified settled values.
- Red-capability is structural, not incidental: the goalless-shootout case
  asserts that a 0-0 bet scores 3 and a 3-0 bet scores 0. If `settled_score`
  regressed to returning `fullTime` (3-0), those two assertions invert (0-0 bet
  drops to 0, 3-0 bet jumps to 3), so the test fails loudly. The same case also
  guards against a future caller wiring `full_time` into `compute_points`
  instead of the settled score.

## Test-seam decision (the delegated Gate 2 call)

**Decision: Option (a) — the e2e test lives inside `football_data.rs`'s existing
`#[cfg(test)] mod tests`, calling `crate::scoring::compute_points` across the
module boundary.** Rejected: option (b), adding a thin `pub` bridge function in
production.

Justification:

- `settled_score()` is private to `football_data.rs`. A test inside that
  module's `tests` submodule sees it via `use super::*` (already in place).
  `compute_points()` is `pub` in `src/scoring.rs`, and both are sibling modules
  under the crate root (`mod football_data;` / `mod scoring;` in `main.rs`), so
  `crate::scoring::compute_points(...)` resolves cleanly from the test. This is
  the only location that can reach **both** seams without touching production.
- It adds zero production surface. Option (b) would introduce a new `pub`
  function in production purely to let a test reach across the boundary. Per
  spec section 3, "No production behavior changes" is out of scope; adding a new
  public function is a behavior *addition*, not merely a change, and would sit in
  direct tension with that constraint. It would also invert the dependency: the
  bug lived in the decode-to-settled seam, and a bridge fn risks becoming a
  second place that "knows" the wiring, which could itself drift.
- The chain stays a true payload to settled to points flow: real serde decode
  via the existing `settled` helper, real private `settled_score`, real public
  `compute_points`. Nothing is stubbed or hand-built.

Trade-off / residual: the test now reaches across a module boundary
(`crate::scoring`) from within `football_data`'s tests, a mild coupling of the
two modules' test code. This is acceptable and idiomatic for a binary crate; the
alternative (an integration test in `tests/`) cannot see the private
`settled_score`, and a `pub` bridge violates scope. If the wiring ever gains a
real production home (e.g. a settle-and-score function used by `recompute_all`),
these tests should move to cover that function directly. That is the deferred
SQL/payout parity follow-up already recorded in spec section 7 and is out of
scope here.

## Files touched

- `src/football_data.rs` — additions to the `#[cfg(test)] mod tests` block only:
  one `score_bet` helper and the new e2e test functions. No changes above the
  test module; no other file modified.

## Acceptance-criteria verification

- **AC1 (shootout after a draw):** In the shootout payload (`fullTime` 6-4,
  `regularTime` 1-1, `extraTime` 0-0, settles 1-1), assert
  `score_bet(json, 1, 1) == 3`, `score_bet(json, 2, 2) == 1`,
  `score_bet(json, 2, 1) == 0`. Note: the spec section 5 AC1 prose says
  `fullTime` 7-6, while the committed payload and spec section 3 say 6-4; both
  settle to 1-1, so the points are identical. Flagged as an open question; the
  plan uses the committed 6-4 payload.
- **AC2 (goalless shootout):** In the goalless payload (`fullTime` 3-0,
  settles 0-0), assert `score_bet(json, 0, 0) == 3` and
  `score_bet(json, 1, 0) == 0` (proving 3-0 was not used as the score).
- **AC3 (red-capable guard):** Same goalless payload, assert both
  `score_bet(json, 0, 0) == 3` and `score_bet(json, 3, 0) == 0`. If
  `settled_score` reverted to `fullTime` (3-0), the first inverts to 0 and the
  second to 3, failing the test. Verified by a critic temporarily flipping
  `settled_score` to return `full_time` and observing the test go red, then
  reverting.
- **AC4 (non-shootout paths unaffected):** EXTRA_TIME payload (`fullTime` 2-1,
  settles 2-1): `score_bet(json, 2, 1) == 3`. REGULAR payload (`fullTime` 0-2,
  settles 0-2): `score_bet(json, 0, 2) == 3`, and an outcome-only bet earns 1
  (e.g. `score_bet(json, 0, 3) == 1`) to prove the fullTime route is live.
- **AC5 (real decode):** `score_bet` decodes via the existing `settled` helper,
  which is `serde_json::from_str::<ApiScore>(json).unwrap().settled_score()`.
  Every payload is a JSON string literal, never a Rust struct; every expected
  total is a bare integer literal.
- **AC6 (no regression):** `cargo test --bin lunatech-betting` (source
  `~/.cargo/env` first) passes with the new tests and no existing test modified.

## Risks & pitfalls

- **AC1 payload discrepancy (7-6 vs 6-4).** Spec section 5 says `fullTime` 7-6;
  spec section 3 and the committed test payload say 6-4. Both settle to 1-1 so
  point assertions are unaffected, but the prose should be reconciled. Raised in
  spec section 7 / reported at Gate 2. Plan proceeds with the committed 6-4.
- **`compute_points` takes `i32`, `settled_score` returns `Option<i32>`.** The
  helper must unwrap the options. For all four in-scope payloads the settled
  values are `Some`, so `.unwrap()` in test code is safe and keeps the helper
  readable. A `None` would panic the test, which is an acceptable loud failure
  for these fully-populated payloads.
- **Independent-literal discipline.** Expected points must be hand-written
  literals, not `POINTS_EXACT` / `POINTS_OUTCOME` constants nor anything derived
  from `compute_points`, or the test would restate the code under test rather
  than pin the outcome.
- **Do not delete or rewrite the existing `settled`-only tests.** They pin the
  first seam independently; the new tests are additive.
- **Scope guard:** if any assertion appears to need a change to `settled_score`
  or `compute_points`, stop and escalate as spec drift (spec section 3). The
  correct expected values are all derivable from the documented payloads.
