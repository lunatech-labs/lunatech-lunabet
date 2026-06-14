# 03. Achievements and badges

Status: to do. Priority: medium. Effort: M.

## Objective

Offer visible, collectible progression beyond the leaderboard. Badges provide micro-goals ("12 more points before the next tier") and reward varied behaviors, not just being first.

## User stories

- As a player, I earn a badge when I achieve a feat (first exact score, perfect day, points tiers).
- As a player, I see my badges on my profile and the next badge to reach.
- As a player, I see a discreet notification when I unlock a badge.

## Initial catalog

| Code | Name | Condition |
|------|-----|-----------|
| first_exact | First flawless | First exact score |
| perfect_day | Perfect day | All of a day's predictions exact (min 2 matches) |
| pts_50 / pts_100 / pts_250 | Tiers | Reach 50 / 100 / 250 cumulative points |
| streak_5 / streak_10 | On fire | Streak of 5 / 10 (see [01-streaks](01-streaks.md)) |
| marathon | Marathoner | Bet on all matches of a stage |
| underdog | Underdog | Correctly predict a win by a non-favorite team (ranking gap) |

The catalog is static in code (no definitions table), only the awards are persisted.

## Data model

```sql
-- migrations/2026xxxx_achievements.sql
CREATE TABLE achievements (
    tenant_id  UUID NOT NULL REFERENCES tenants(id),
    user_id    UUID NOT NULL REFERENCES users(id),
    code       TEXT NOT NULL,
    earned_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, user_id, code)
);
```

The primary key prevents double awarding and makes the evaluation idempotent.

## Backend

- Module `src/achievements.rs`: one function per rule, plus `evaluate_user(pool, tenant, user_id)` that inserts the missing badges (`INSERT ... ON CONFLICT DO NOTHING`).
- Call after scoring in [src/main.rs](../src/main.rs), for the users whose bet has just been settled.
- The "underdog" and "marathon" badges need a notion of favorite and a list of a stage's matches: derive from `matches` (stage, group_name) without any new external data.

## UI

- New profile page (see [09-profile-rivalries](09-profile-rivalries.md)) that lists the earned badges and grays out the upcoming ones, with the condition.
- Small badge row on [templates/leaderboard.html](../templates/leaderboard.html) next to the name (3 max, the rest as "+N").
- "Badge unlocked" htmx toast on the next page load after earning it.
- Dedicated SVG icons in `static/badges/`, style consistent with the existing avatars.

## i18n

Name and description of each badge in both languages, mapping table in Rust.

## Edge cases

- Historical recomputation: `evaluate_user` must be able to run over the entire history without creating duplicates.
- Adding a badge to the catalog: a one-time sweep awards the badge retroactively to those eligible.
- Perfect day with a single match: excluded to avoid triviality.

## Acceptance criteria

- A badge can only be earned once.
- Adding a rule does not affect the badges already awarded.
- The profile displays earned badges and next tiers with numeric progression.
