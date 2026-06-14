# 10. Weekly challenges

Status: to do. Priority: low. Effort: M.

## Objective

Offer a short-term goal that renews every week, independent of the overall ranking. Players who are behind in the standings keep a reason to play ("challenge of the week: 5 exact predictions"), which sustains activity over the course of a long competition.

## User stories

- As a player, I see the challenge of the week and my progress.
- As a player, I earn a badge or a mention when I complete it.
- As an admin, the challenge is generated automatically, with no intervention.

## Data model

```sql
-- migrations/2026xxxx_challenges.sql
CREATE TABLE weekly_challenges (
    tenant_id   UUID NOT NULL REFERENCES tenants(id),
    week_start  DATE NOT NULL,
    kind        TEXT NOT NULL,
    target      INT  NOT NULL,
    PRIMARY KEY (tenant_id, week_start)
);

CREATE TABLE weekly_challenge_results (
    tenant_id   UUID NOT NULL REFERENCES tenants(id),
    week_start  DATE NOT NULL,
    user_id     UUID NOT NULL REFERENCES users(id),
    progress    INT  NOT NULL DEFAULT 0,
    completed_at TIMESTAMPTZ,
    PRIMARY KEY (tenant_id, week_start, user_id)
);
```

`kind` from a fixed catalog defined in code: `exact_count` (N exact scores), `points_total` (N points), `bet_all` (bet on all of the week's matches).

## Backend

- Module `src/challenges.rs`:
  - `ensure_week(pool, tenant, week_start)`: creates the challenge of the week if it does not exist (the `kind` is chosen by a deterministic rotation based on the week number, no randomness because it is not available in some contexts).
  - `recompute_progress(pool, tenant, week_start)`: recomputes each player's progress, marks `completed_at` when the threshold is crossed.
- Hook into the scoring loop in [src/main.rs](../src/main.rs).
- On completion, award a badge via [03-achievements-badges](03-achievements-badges.md) (`code = weekly_<kind>`), which reuses the entire display and notification mechanism.

## UI

- "Challenge of the week" card on [templates/today.html](../templates/today.html): label, progress bar, badge at stake.

## i18n

- Labels per `kind`: "Reussis 5 scores exacts cette semaine" / "Land 5 exact scores this week", etc.

## Edge cases

- Week with no match: no challenge, create nothing.
- Time zone change: align `week_start` on the Amsterdam Monday, consistent with the digest.
- `bet_all` challenge when the number of matches varies: target = number of matches in the week, computed at creation time.

## Acceptance criteria

- A single challenge per tenant and per week, generated automatically.
- Progress and completion are accurate and idempotent.
- Completion grants the corresponding badge.
