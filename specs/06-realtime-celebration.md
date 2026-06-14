# 06. Real-time score celebration

Status: to do. Priority: medium. Effort: S.

## Objective

Provide rewarding feedback at the moment a bet is won. Today scoring runs silently every 5 min; we want a visual effect (confetti, manga burst, tiger) the first time the user sees again a match they got right, to anchor the reward loop.

## User stories

- As a player, when I come back to the app after one of my predictions has been validated, I see a celebration animation on the won matches.
- The animation does not replay on every reload: once seen, it turns off.

## Approach

Reuse the existing aesthetic: `manga-burst.svg`, the tiger from [static/easter-eggs.js](../static/easter-eggs.js). No websocket: we detect server-side the bets "newly seen as won".

## Data model

```sql
-- migrations/2026xxxx_bet_seen.sql
ALTER TABLE bets ADD COLUMN result_seen_at TIMESTAMPTZ;
```

A settled bet (`points` non null) with `result_seen_at IS NULL` is "to celebrate".

## Backend

- [src/routes/today.rs](../src/routes/today.rs) and [src/routes/matches.rs](../src/routes/matches.rs): on read, select the user's settled bets not yet seen, expose them to the template, then mark them `result_seen_at = NOW()` (after render, or via a small confirmation POST to avoid marking if the page is not actually displayed).
- Distinguish the celebration level: exact (3 pts, big effect), outcome (1 pt, light effect).

## UI

- [templates/match_card.html](../templates/match_card.html): `data-celebrate="exact|outcome"` attribute on the relevant cards.
- New `static/celebrate.js`: on load, scans the cards to celebrate and triggers confetti or tiger, relying on the existing easter egg helpers (factor out the tiger code).
- Optional sounds disabled by default.

## i18n

- Short messages "Score exact ! +3" / "Exact score! +3", "Bien vu ! +1" / "Nice! +1".

## Edge cases

- Many matches won at once (returning after several days): limit to an aggregated celebration ("5 predictions won, +11 pts") rather than five animations.
- Lost bet: no animation, just the existing display.
- The `result_seen_at` marking must be robust to double loading (idempotent).

## Acceptance criteria

- The animation appears only once per won match and per user.
- The intensity reflects exact vs outcome.
- No animation for bets already seen or lost.
