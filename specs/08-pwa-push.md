# 08. PWA and push notifications

Status: to do. Priority: medium. Effort: L.

## Objective

Make the app installable on the home screen and send web push notifications, which are far more immediate than email. Target: reminders before kickoff and winnings alerts ("you just won 3 pts, you move up to 4th"). This web foundation is also the prerequisite for the Tauri mobile client ([12-mobile-tauri](12-mobile-tauri.md)).

## User stories

- As a player, I install LunaBet like an app (PWA).
- As a player, I allow notifications and I receive a push "you have not bet yet, kickoff in 1h".
- As a player, I receive a push when my points change my rank.
- As a player, I manage my notification preferences.

## Components

### PWA
- `static/manifest.webmanifest`: name, icons (reuse `favicon.svg`), tenant colors, `display: standalone`, `start_url: /today`.
- `static/sw.js`: service worker, minimal application cache (offline shell) and reception of push messages.
- Tags in [templates/base.html](../templates/base.html): manifest link, service worker registration.
- The manifest can be served dynamically per tenant to pick up the colors and the logo (lightweight route or template).

### Web push (VAPID)
Rust crate: `web-push`.

```sql
-- migrations/2026xxxx_push_subscriptions.sql
CREATE TABLE push_subscriptions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id),
    user_id     UUID NOT NULL REFERENCES users(id),
    endpoint    TEXT NOT NULL,
    p256dh      TEXT NOT NULL,
    auth        TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (user_id, endpoint)
);

ALTER TABLE users ADD COLUMN notify_push BOOLEAN NOT NULL DEFAULT TRUE;
```

VAPID keys as environment variables ([src/config.rs](../src/config.rs)): `VAPID_PUBLIC_KEY`, `VAPID_PRIVATE_KEY`.

## Backend

- Routes `src/routes/push.rs`:
  - `POST /push/subscribe`: registers a subscription.
  - `POST /push/unsubscribe`.
  - `GET /push/public-key`: exposes the VAPID public key.
- [src/notifications.rs](../src/notifications.rs): alongside the existing emails, send a push when the channel is available. Reuse the per-match idempotency (`match_reminders`).
- Clean up invalid subscriptions (410 Gone) returned by the push service.

## UI

- "Enable notifications" button on [templates/today.html](../templates/today.html) or a settings page, which triggers the permission request and the subscription.
- Preferences section: checkboxes for match reminders, rank alerts.

## i18n

- Push titles and bodies localized based on `users.lang` (already persisted).

## Edge cases

- iOS Safari: web push supported only in an installed PWA (iOS 16.4+). Document the limit; the Tauri client ([12-mobile-tauri](12-mobile-tauri.md)) works around it via native push.
- Permission denied: fall back to email, do not keep prompting in a loop.
- Expired subscription: purge on the first send error.

## Acceptance criteria

- The app installs and opens in standalone mode.
- A reminder push arrives before kickoff for subscribers who have not yet bet.
- The preferences effectively turn off the corresponding push messages.
