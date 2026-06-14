# 07. Countdown and urgency

Status: to do. Priority: high. Effort: S.

## Objective

Increase the betting rate by creating a sense of urgency before kickoff. A visible countdown ("only 23 min left to bet") drives immediate action, especially on matches where the user has not yet placed a bet.

## User stories

- As a player, I see on each open match the time remaining before kickoff.
- When less than an hour remains and I have not bet, the indicator switches to urgent mode (accent color, pulsing).
- On expiry, the card automatically flips to "locked" without a manual reload.

## Approach

Everything is client side, based on the `kickoff_at` already rendered in the HTML. No new field and no server request.

## Backend

- Make sure `match_card.html` exposes `kickoff_at` in ISO 8601 in a `data-kickoff` attribute (check the existing code, otherwise add it). [src/models.rs](../src/models.rs) already has `is_open_for_bets()`.
- No new route.

## UI

- [templates/match_card.html](../templates/match_card.html): `.countdown[data-kickoff]` element on open matches.
- New `static/countdown.js`: ticks every second, formats "2h 14m", "23 min", "less than 1 min". Below a threshold (60 min) it adds the `.countdown-urgent` class. At zero, it disables the form inputs and adds `.locked` without reloading.
- CSS in [static/style.css](../static/style.css): `.countdown`, `.countdown-urgent` (pulsing, tenant accent color), locked state.
- Rely on the existing [static/timezone-converter.js](../static/timezone-converter.js) for timezone consistency.

## i18n

- Formats "h" / "h", "min" / "min", "Coup d'envoi imminent" / "Kickoff imminent", "Paris clos" / "Bets closed". Pass the labels via `data-*` or a small dictionary injected based on `loc`.

## Edge cases

- Skewed client clock: acceptable tolerance, the server remains the authority for accepting or rejecting a bet (already the case via `is_open_for_bets`).
- Tab in the background for a long time: recalculate from the current time, no accumulation of drift.
- Match already started at load time: render directly as locked.

## Acceptance criteria

- The countdown is accurate to the minute and switches to urgent under 60 min.
- On expiry, the card locks without any user action.
- No regression on server-side acceptance of bets.
