# 02. Player of the day

Status: to do. Priority: high. Effort: S.

## Objective

Highlight the best predictor of the previous day, every day. Public recognition creates a reason to come back to see "who won" and rewards players without monopolizing the general leaderboard.

## User stories

- As a player, I see at the top of the dashboard who scored the most points on the previous day's matches.
- As the player of the day, I see my avatar highlighted and a "Player of the day" label.
- The digest email mentions the player of the day.

## Definition

- Period: the `FINISHED` matches whose `kickoff_at` falls within the previous calendar day (Amsterdam timezone, like the existing digest).
- Day score: sum of the user's `bets.points` on those matches.
- Winner: highest day score. Ties broken by number of exact scores, then alphabetical order of the name.
- If no match finished the previous day: no player of the day.

## Data model

```sql
-- migrations/2026xxxx_player_of_the_day.sql
CREATE TABLE player_of_the_day (
    tenant_id   UUID NOT NULL REFERENCES tenants(id),
    day         DATE NOT NULL,
    user_id     UUID NOT NULL REFERENCES users(id),
    points      INT  NOT NULL,
    exact_count INT  NOT NULL,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, day)
);
```

Primary key `(tenant_id, day)` makes the computation idempotent, like `daily_digests`.

## Backend

- Computation in `src/streaks.rs` or a new `src/highlights.rs`, function `compute_player_of_the_day(pool, tenant, day)`.
- Wired into the same scheduler as the daily digest in [src/notifications.rs](../src/notifications.rs): the computation precedes the digest send, so the email can cite the winner.
- Read route: helper called by [src/routes/today.rs](../src/routes/today.rs) to load the current day's entry (which reflects the previous day).

## UI

- [templates/today.html](../templates/today.html): "Player of the day" banner at the top, with avatar ([src/characters.rs](../src/characters.rs)), name, points scored. Festive style reusing `manga-burst.svg`.
- [templates/emails/daily_digest.html](../templates/emails/daily_digest.html): a line "Player of the day: X (N pts)".

## i18n

- "Joueur du jour" / "Player of the day", "a marque" / "scored".

## Edge cases

- Tie: deterministic tiebreak (exacts then name) to avoid a winner that changes between two computations.
- Single-player space: they are player of the day as soon as they score, acceptable.
- No match the previous day: display nothing, do not insert a row.

## Acceptance criteria

- The player of the day matches the highest points total of the previous day.
- The computation is idempotent (replaying does not create a duplicate).
- The banner disappears on days with no finished matches.
