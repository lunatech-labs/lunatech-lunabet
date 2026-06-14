# 04. Private leagues among friends

Status: to do. Priority: high. Effort: M.

## Objective

Allow, within a space, the creation of mini-leagues bringing together a subset of players with their own leaderboard. Close social comparison (my friends, my teammates) is a far stronger retention lever than the global leaderboard, where stragglers quickly drop off.

Not to be confused with spaces (tenants): a league is a group internal to a space, which shares the same matches and the same bets. No new bet is created, we only filter the leaderboard.

## User stories

- As a player, I create a league and obtain a share code.
- As a player, I join a league with a code.
- As a player, I see a leaderboard filtered to the members of my league, reusing the existing points.
- As a creator, I rename or delete my league and I remove members.

## Data model

```sql
-- migrations/2026xxxx_leagues.sql
CREATE TABLE leagues (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id),
    name        TEXT NOT NULL,
    join_code   TEXT NOT NULL,
    owner_user_id UUID NOT NULL REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, join_code)
);

CREATE TABLE league_members (
    league_id  UUID NOT NULL REFERENCES leagues(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id),
    joined_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (league_id, user_id)
);
```

The `join_code` is short and readable (for example 6 base32 characters without ambiguities). Uniqueness per tenant.

## Backend

New routes module `src/routes/leagues.rs`:

- `GET /leagues`: my leagues + creation / join form.
- `POST /leagues`: create (generates `join_code`, adds the creator as a member).
- `POST /leagues/join`: join via code.
- `GET /leagues/:id`: league leaderboard.
- `POST /leagues/:id/leave`, `POST /leagues/:id/remove` (creator), `POST /leagues/:id/rename`, `DELETE /leagues/:id`.

The leaderboard reuses the logic of [src/stakes.rs](../src/stakes.rs) by adding a filter `user_id IN (SELECT user_id FROM league_members WHERE league_id = $1)`. Refactor `load_leaderboard` to accept an optional members filter.

Safeguards:
- All routes require `AuthUser` and verify that the league belongs to the current tenant.
- Only the creator deletes or removes members.
- A reasonable cap on leagues per user (for example 20) to limit abuse.

## UI

- New "Leagues" menu entry in [templates/_nav.html](../templates/_nav.html).
- [templates/leagues.html](../templates/leagues.html): list of my leagues, create button, code field to join, code sharing (copy to clipboard).
- [templates/league.html](../templates/league.html): filtered leaderboard, reuses the visual component of the global leaderboard.

## Stakes and pot

Leagues are purely for fun at the start: no pot per league. The pot stays at the space level ([src/stakes.rs](../src/stakes.rs)). A pot per league is a future extension, out of scope.

## i18n

- "Ligues" / "Leagues", "Creer une ligue" / "Create a league", "Code d'invitation" / "Join code", "Rejoindre" / "Join".

## Edge cases

- Code collision: regenerate until unique.
- Joining twice: `ON CONFLICT DO NOTHING`.
- Creator who leaves: transfer ownership to the oldest member, or delete if empty.
- Empty league after departure: optional automatic deletion.

## Acceptance criteria

- A league leaderboard only shows its members, with the same points as the global leaderboard.
- A code allows joining, a bad code returns a clear error.
- Creator rights are respected (rename, remove, delete).
