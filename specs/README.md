# LunaBet, product specs: engagement and growth

This folder gathers the specs for features targeting two objectives:

1. Make the app more fun and more addictive (reward loops, social, urgency).
2. Open a new way to create a space based on invitation, without dependency on the email domain ("friends" mode).

Each spec is self-contained: objective, data model, backend, UI, i18n, edge cases, acceptance criteria. The specs are written for the current stack: Rust + Axum, SQLx + PostgreSQL, Askama templates, htmx, multi-tenant.

## Current state (existing)

Already in place and reusable as a foundation:

- Exact score prediction, automatic scoring 3 / 1 / 0 every 5 min ([src/scoring.rs](../src/scoring.rs)).
- Leaderboard with real prize pool and winnings estimate ([src/stakes.rs](../src/stakes.rs)).
- Deterministic Captain Tsubasa avatars ([src/characters.rs](../src/characters.rs)).
- Easter eggs ([static/easter-eggs.js](../static/easter-eggs.js)) and "I feel lucky" button ([static/lucky.js](../static/lucky.js)).
- Emails: match reminder, daily digest, matches of the day ([src/notifications.rs](../src/notifications.rs)).
- Multi-tenant with resolution by subdomain, cached registry ([src/tenant.rs](../src/tenant.rs)).
- Magic link auth, gating by `allowed_email_pattern` ([src/routes/auth.rs](../src/routes/auth.rs)).
- Self-serve space creation via `pending_tenants` ([src/routes/signup.rs](../src/routes/signup.rs)).
- Bilingual FR / EN ([src/i18n.rs](../src/i18n.rs)).

## List of specs

| # | Spec | Theme | Priority | Effort |
|---|------|-------|----------|--------|
| 01 | [Streaks](01-streaks.md) | Reward / retention | High | S |
| 02 | [Player of the day](02-player-of-the-day.md) | Social recognition | High | S |
| 03 | [Achievements and badges](03-achievements-badges.md) | Progression | Medium | M |
| 04 | [Private leagues among friends](04-private-leagues.md) | Social / retention | High | M |
| 05 | [Jokers and multipliers](05-confidence-multipliers.md) | Strategy | Medium | M |
| 06 | [Real-time score celebration](06-realtime-celebration.md) | Dopamine | Medium | S |
| 07 | [Countdown and urgency](07-countdown-urgency.md) | Conversion | High | S |
| 08 | [PWA and push notifications](08-pwa-push.md) | Re-engagement | Medium | L |
| 09 | [Profile, stats and rivalries](09-profile-rivalries.md) | Attachment | Medium | M |
| 10 | [Weekly challenges](10-weekly-challenges.md) | Short goals | Low | M |
| 11 | [Invite-based spaces (friends mode)](11-invite-based-orgs.md) | Growth / onboarding | High | M |
| 12 | [Mobile client iOS and Android (Tauri)](12-mobile-tauri.md) | Distribution / re-engagement | Medium | L |

Effort: S = 1 to 2 days, M = 3 to 5 days, L = 1 to 2 weeks (indicative estimates, one developer).

## Recommended phasing

### Phase 1, high-visibility quick wins (1 to 2 weeks)
Built on already-computed data, immediate impact.

- 01 Streaks
- 02 Player of the day
- 06 Real-time celebration
- 07 Countdown and urgency

### Phase 2, growth and social (2 to 3 weeks)
The strongest retention levers.

- 11 Invite-based spaces (friends mode)
- 04 Private leagues among friends

### Phase 3, game depth (2 to 4 weeks)
To be done once the social base is in place.

- 03 Achievements and badges
- 09 Profile, stats and rivalries
- 05 Jokers and multipliers

### Phase 4, advanced re-engagement
- 08 PWA and push notifications
- 10 Weekly challenges

### Phase 5, native applications
To be launched once the PWA and web push are validated, the Tauri client reuses this foundation.

- 12 Mobile client iOS and Android (Tauri)

## Cross-cutting principles

- **No regression on existing scoring.** Any additional points mechanic (jokers, multipliers) must be opt-in per tenant and reversible.
- **Everything is tenant-scoped.** Each new table carries a `tenant_id` and respects the RLS already in place.
- **Bilingual by default.** Every visible text goes through `loc.f("Francais", "English")`.
- **No heavy JS.** We stay on htmx + vanilla JS, one file per feature in `static/`.
- **Idempotent jobs.** Periodic computations (streaks, player of the day) follow the pattern of the existing idempotency tables (`daily_digests`, `today_matches_emails`).
