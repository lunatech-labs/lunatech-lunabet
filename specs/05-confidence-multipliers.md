# 05. Jokers and multipliers

Status: to do. Priority: medium. Effort: M.

## Objective

Add a layer of strategy and tension: allow staking a "joker" that doubles the points of a chosen match. A joker is a confidence stake: the player designates the match they are most sure about, and if that bet earns points (exact score or correct outcome), those points are doubled. This creates a dilemma before kickoff (where to place your confidence) and euphoria or regret afterwards.

Opt-in feature per space so as not to disrupt spaces that want to keep scoring simple.

## User stories

- As a player, I can mark an upcoming match as my "joker" for the current phase.
- As a player, my points on this match are doubled.
- As an admin, I enable or disable jokers for my space.

## Rules

- **One joker per competition phase.** The phase is carried by `matches.stage` (group phase, round of 16, quarter-finals, semi-finals, final). A player places at most one joker among the matches of the same phase.
- The joker must be placed before the match `kickoff_at`, like a bet.
- Multiplier applied at computation: `points_effectifs = points_base * multiplier`.
- Editable as long as the target match has not started and as long as no match of the phase has locked the choice (see edge cases).

## Data model

```sql
-- migrations/2026xxxx_multipliers.sql
ALTER TABLE tenants ADD COLUMN jokers_enabled BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE bets    ADD COLUMN multiplier INT NOT NULL DEFAULT 1
    CHECK (multiplier IN (1, 2));
```

The joker is carried by the bet itself (`bets.multiplier = 2`). Application constraint: at most one bet with `multiplier = 2` per user and per phase (`matches.stage`).

## Backend

- [src/scoring.rs](../src/scoring.rs): `compute_points` stays unchanged for the base, but `recompute_all` multiplies by `bets.multiplier` before writing `bets.points`. Keep a clear trace: store `points` as the final value (already x multiplier).
- [src/routes/bets.rs](../src/routes/bets.rs): new action `POST /bets/:match_id/joker` (toggle) which verifies:
  - jokers enabled for the tenant,
  - match still open,
  - no other joker already placed in the same phase (otherwise move it, with confirmation).
- Determine the phase via `matches.stage` of the target match, and look for any existing joker on the other matches of this phase for the same user.
- Validation of per-phase uniqueness in a transaction.

## UI

- [templates/match_card.html](../templates/match_card.html): "x2 joker" button on open matches if the feature is active. Distinct visual state when placed.
- Help banner the first time ("One joker per phase, double your points").
- [templates/admin_settings.html](../templates/admin_settings.html): "Enable jokers" switch.

## i18n

- "Joker" / "Joker", "Double tes points" / "Double your points", "Un joker par phase" / "One joker per phase".

## Edge cases

- Move a joker already placed in the phase: remove the old one, place the new one, in a transaction. Possible as long as the new target match has not started.
- Joker on a match that has already started: refused.
- Joker placed on a match of the phase that has already been played: the choice is fixed from the kickoff of that match; it can no longer be moved to another match of the same phase, otherwise it would allow changing your mind after the fact.
- Phase with a single match (final): the joker is trivial there but allowed.
- Disabling jokers by the admin during the competition: jokers already placed remain honored, no new one is possible.

## Acceptance criteria

- A single active joker per period and per player.
- The points of the joker match are effectively doubled in the leaderboard.
- Space without jokers enabled: no change in behavior or UI.
