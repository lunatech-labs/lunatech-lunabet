# 09. Profile, stats and rivalries

Status: to do. Priority: medium. Effort: M.

## Objective

Give each player a personal page that tells their story (accuracy, best and worst predictions, badges, streak) and introduce named rivalries ("you beat Marie 3-2 this week"). Personal stats and rivalry create an emotional attachment that goes beyond the raw leaderboard.

## User stories

- As a player, I have a profile page with my statistics: points, accuracy (% of exact scores), streak, badges.
- As a player, I see my best and my worst prediction.
- As a player, I can compare myself head to head with another member (weekly tally, competition tally).

## Data

No new table required: everything is derived from `bets`, `matches`, and the related features ([01-streaks](01-streaks.md), [03-achievements-badges](03-achievements-badges.md)).

Computed statistics:
- Total points, number of settled bets.
- Exact accuracy = exact scores / settled bets.
- Result accuracy = (exact + correct results) / settled bets.
- Best prediction: the exact score on the most "improbable" match (simple heuristic: large goal gap or posted odds).
- Worst prediction: bet with the biggest gap to the actual result.

## Backend

- Routes module `src/routes/profile.rs`:
  - `GET /profile`: my profile.
  - `GET /profile/:user_id`: public profile of a member of the same tenant (read only, data already public via the leaderboard).
  - `GET /h2h/:user_id`: head to head comparison with me.
- Aggregated queries on `bets` filtered by user and tenant. Reuse the helpers from [src/stakes.rs](../src/stakes.rs) where possible.

Head to head: across all finished matches, compare each player's points, count who did better match by match, and derive a "wins-losses-draws" score.

## UI

- [templates/profile.html](../templates/profile.html): avatar, name, key stats, badges, streak, best and worst prediction.
- [templates/h2h.html](../templates/h2h.html): two columns, tally, weekly trend.
- Links from [templates/leaderboard.html](../templates/leaderboard.html): clicking a name opens their profile; a "Challenge" button opens the head to head.

## i18n

- "Profil" / "Profile", "Precision" / "Accuracy", "Meilleur prono" / "Best call", "Tete-a-tete" / "Head to head".

## Edge cases

- Player with no settled bet: accuracy hidden, message "no results yet".
- Head to head between players who have not bet on any common match: show "not enough data".
- Respect the tenant scope: we never compare players from different tenants.

## Acceptance criteria

- The accuracy percentages are consistent with the leaderboard counts.
- The public profile only shows data already exposed publicly.
- The head to head correctly reflects the match by match tally.
