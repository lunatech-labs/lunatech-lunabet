# Feature Spec: Penalty-shootout scoring end-to-end regression tests

> Status: PLANNED
> Spec folder: specs/001-penalty-shootout-e2e-tests/

## 1. Mission / Why

Knockout / penalty-shootout scoring has been a recurring bug: football-data.org
folds extra time and penalties into the `fullTime` score, and earlier versions
paid users on that folded number instead of the 120-minute (after-extra-time)
result. The fix lives across two seams — `ApiScore::settled_score()` (decodes
the real JSON into the settled 120-minute score) and `compute_points()` (turns a
settled score plus a user's bet into points). Today each seam is unit-tested in
isolation, but nothing drives a real payload through *both* and asserts the
points a user actually receives. This spec adds permanent end-to-end regression
coverage of that wiring so the bug can never silently come back.

## 2. Outcome

Given a real football-data.org JSON score payload (penalty shootout, goalless
shootout, extra-time, and regular matches) and a user's bet, a test decodes the
payload through the real `ApiScore` types into `settled_score()`, feeds the
settled score into `compute_points()` alongside the bet, and asserts the exact
points the user receives (3 / 1 / 0). The tests are **red-capable**: each fails
if the wiring regresses — e.g. `settled_score` reverting to `fullTime`, or
`fullTime` being passed into `compute_points` instead of the settled score — not
merely a restatement of today's output.

## 3. Scope

### In scope

- End-to-end tests driving inline JSON payloads through `ApiScore` decode →
  `settled_score()` → `compute_points()`, asserting the points awarded.
- Use the documented real example values:
  - Penalty shootout after a draw: `fullTime` 6-4, `regularTime` 1-1,
    `extraTime` 0-0 → settles 1-1.
  - Goalless shootout: `fullTime` 3-0, `regularTime` 0-0, `extraTime` 0-0 →
    settles 0-0.
  - Extra-time decided (no shootout) and a regular 90-minute match, to prove the
    non-shootout path still routes through `fullTime`.
- Expected point totals expressed as **independent literals** (3 / 1 / 0), not
  derived from the same code under test.
- At least one explicitly red-capable assertion that would catch the historical
  regression (settling on the penalty-folded `fullTime`).
- Match the existing `football_data.rs` test style (inline JSON through the real
  decode), not the hand-typed-struct style.

### Out of scope

- **No production behavior changes.** Tests only. Do not modify `settled_score`,
  `compute_points`, or any scoring logic. If a test appears to require a logic
  change, stop and escalate as spec drift.
- **No DB / `recompute_all` coverage.** No Postgres, no SQL-path test in this
  run (see Open Questions — deferred parity follow-up).
- **No joker / multiplier coverage.** `compute_points` does not take the
  multiplier; it is outside the Rust seam.
- **No new external fixture files and no HTTP mocking.** Inline JSON only.
  Committed fixture files are an explicit non-goal here.

## 4. Constraints & Decisions

- Language / framework: Rust, standard `#[cfg(test)]` modules, `serde_json` for
  decode. Binary crate — tests run via `cargo test --bin lunatech-betting`
  (source `~/.cargo/env` first).
- Must use the **real** `ApiScore` JSON decode path — hand-built structs would
  bypass the deserialization where the bug lived (`fullTime` vs
  `regularTime`/`extraTime`).
- `settled_score()` is private to `football_data.rs`; `compute_points()` is
  public in `scoring.rs`. No production function wires the two today. The test
  seam (test-inside-`football_data.rs` vs. a thin `pub` bridge fn) is **delegated
  to the planner** at Gate 2, under the binding constraint that the chosen seam
  must keep the test red-capable and must not change scoring behavior.
- `compute_points` returns base points (3/1/0). The real user-facing payout seam
  is `recompute_all`'s SQL, which is `compute_points` × joker multiplier and is
  currently untested with no parity guard — explicitly noted, deferred.

## 5. Acceptance Criteria (how you'll verify it)

- [ ] AC1 (shootout after a draw): Given the `fullTime` 6-4 / `regularTime` 1-1
  / `extraTime` 0-0 PENALTY_SHOOTOUT payload decoded through `ApiScore`, when its
  `settled_score()` result is passed into `compute_points()` with a bet of 1-1,
  then the user receives **3** points; with a bet of 2-2, **1** point; with a bet
  of 2-1, **0** points.
- [ ] AC2 (goalless shootout): Given the `fullTime` 3-0 / `regularTime` 0-0 /
  `extraTime` 0-0 PENALTY_SHOOTOUT payload, when settled and scored against a 0-0
  bet, then the user receives **3** points; against a 1-0 bet, **0** points
  (proving the 3-0 penalty count was not used as the score).
- [ ] AC3 (red-capable guard): At least one assertion demonstrably fails if
  `settled_score` returned `fullTime` for a PENALTY_SHOOTOUT payload — e.g. for
  the goalless case, a 0-0 bet scoring 3 would drop to 0 (since `fullTime` 3-0 ≠
  0-0), and a 3-0 bet scoring 0 would jump to 3. The test asserts the settled-on
  values such that the folded-`fullTime` behavior produces a different, failing
  result.
- [ ] AC4 (non-shootout paths unaffected): Given an EXTRA_TIME payload
  (`fullTime` 2-1, decided in ET) and a REGULAR payload (`fullTime` 0-2), when
  settled and scored, the points reflect the `fullTime` score (e.g. exact-score
  bets earn 3), proving non-shootout matches still route through `fullTime`.
- [ ] AC5 (real decode): All payloads are decoded from JSON strings via
  `serde_json::from_str::<ApiScore>` (or the existing test helper), not
  constructed as Rust struct literals; expected point totals are independent
  literals.
- [ ] AC6 (no regression): `cargo test --bin lunatech-betting` passes with the
  new tests added and no existing test modified or broken.

## 6. Task Breakdown

> Seam decision (see plan.md): all tests live inside `football_data.rs`'s
> existing `#[cfg(test)] mod tests`, calling `crate::scoring::compute_points`
> across the module boundary. Zero production surface added. All tasks touch
> only `src/football_data.rs` test module; no production code changes.

- [x] **Task 1 — Composed payload-to-points helper + shootout-after-a-draw
  case.** Add a test-only helper
  `score_bet(json, bet_home, bet_away) -> i32` that runs the real chain
  (existing `settled` decode → private `settled_score` → public
  `crate::scoring::compute_points`, unwrapping the settled options). Using the
  committed England 1-1 Switzerland PENALTY_SHOOTOUT payload (`fullTime` 6-4,
  `regularTime` 1-1, `extraTime` 0-0, settles 1-1), assert a 1-1 bet scores 3,
  a 2-2 bet scores 1, a 2-1 bet scores 0, all as bare literals. Establishes the
  helper the later tasks reuse.
  `verifies: AC1, AC5` — `depends_on: none`

- [x] **Task 2 — Goalless-shootout case with red-capable guard.** Using the
  committed Portugal 0-0 Slovenia PENALTY_SHOOTOUT payload (`fullTime` 3-0,
  settles 0-0), assert a 0-0 bet scores 3, a 1-0 bet scores 0, and a 3-0 bet
  scores 0. The 0-0-scores-3 and 3-0-scores-0 pair is the red-capable guard: if
  `settled_score` regressed to returning `fullTime` (3-0) these invert and the
  test fails. Critic verifies by temporarily flipping `settled_score` to return
  `full_time`, observing red, then reverting.
  `verifies: AC2, AC3, AC5` — `depends_on: Task 1`

- [ ] **Task 3 — Non-shootout paths route through fullTime.** Using the
  committed EXTRA_TIME payload (`fullTime` 2-1, settles 2-1) assert a 2-1 bet
  scores 3; using the committed REGULAR payload (`fullTime` 0-2, settles 0-2)
  assert a 0-2 bet scores 3 and an outcome-only bet (0-3) scores 1, proving the
  non-shootout path still uses `fullTime`. Then run
  `cargo test --bin lunatech-betting` (source `~/.cargo/env` first) and confirm
  all tests pass with no existing test modified or broken.
  `verifies: AC4, AC5, AC6` — `depends_on: Task 1`

## 7. Open Questions

- **Deferred follow-up (not for this run): SQL/payout parity.** `compute_points`
  is a hand-maintained Rust twin of the real payout seam, `recompute_all`'s SQL,
  and the true payout is base points × joker multiplier. There is no parity guard
  today, so a future divergence between the SQL and the Rust twin (or a multiplier
  bug) would pay users wrong while these tests stay green. Recommended follow-up
  once a Postgres test harness exists: a parity test (`recompute_all` ≡
  `compute_points` × multiplier across inputs) or a DB integration test of
  `recompute_all`. The critic should note this residual seam gap rather than treat
  `compute_points` as the full user-facing seam.
