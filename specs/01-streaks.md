# 01. Streaks

Status: to do. Priority: high. Effort: S.

## Objective

Reward consistency by counting the consecutive matches where the user scored points. The streak is the cheapest addiction driver: it is computed entirely from `bets.points`, which is already persisted, and the fear of "breaking your streak" brings the user back every day.

## User stories

- As a player, I see my current streak ("3 matches in a row with points") on the dashboard.
- As a player, I receive a "don't break your streak" reminder when I have an active streak and have not yet bet on the next match.
- As a player, I see the space's best streak on the leaderboard.

## Definition

- Current streak: number of consecutive finished matches (ordered by `kickoff_at`) on which the user had a bet with `points > 0`.
- A finished match on which the user did not bet, or scored 0 points, resets the streak to zero.
- Best streak: longest historical sequence.

## Data model

No mandatory table: the streak can be derived on the fly from `bets` and `matches`. To avoid recomputing on every page, we materialize it on `users`:

```sql
-- migrations/2026xxxx_streaks.sql
ALTER TABLE users ADD COLUMN current_streak INT NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN best_streak    INT NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN streak_updated_match_id BIGINT;
```

`streak_updated_match_id` keeps the last finished match already taken into account, to make the update idempotent.

## Backend

New module `src/streaks.rs`:

- `recompute_for_tenant(pool, tenant_id)`: for each user, walks through their finished matches by `kickoff_at`, updates `current_streak` / `best_streak`.
- Call wired into the existing scoring loop in [src/main.rs](../src/main.rs), right after `scoring::recompute_all`, so every 5 min. No new job.
- Helper `streak_of(user)` readable by the routes.

Computation query (sketch, per user):

```sql
SELECT m.id, b.points
FROM matches m
JOIN bets b ON b.match_id = m.id AND b.user_id = $1
WHERE m.status = 'FINISHED' AND b.tenant_id = $2
ORDER BY m.kickoff_at ASC;
```

We fold in Rust to compute the suffix streak (current) and the max (best).

## UI

- [templates/today.html](../templates/today.html): "Streak: 3 (best 5)" badge near the name, with a flame. Reuse the style of the existing tier badges.
- [templates/leaderboard.html](../templates/leaderboard.html): "Streak" column with flame icon, visually sortable.
- CSS in [static/style.css](../static/style.css): `.streak-badge` class that intensifies with length (3, 5, 10).

## i18n

- "Serie" / "Streak", "record" / "best", "Ne casse pas ta serie !" / "Don't break your streak!".

## Notifications

Extend the match reminder in [src/notifications.rs](../src/notifications.rs): if the recipient has `current_streak >= 3` and has not yet bet on the upcoming match, add a hook line to the email. No new email, just a content variant.

## Edge cases

- First match never bet on: streak 0, no badge.
- Match without a bet in the middle: breaks the streak.
- Replayed recomputation: idempotent thanks to the full walk, `best_streak` never decreases.

## Acceptance criteria

- The streak displays correctly after at least two consecutive finished matches with points.
- A 0-point score breaks the current streak but keeps the best.
- The leaderboard page lists the streaks without an N+1 query (a single aggregated SELECT or materialized columns).
