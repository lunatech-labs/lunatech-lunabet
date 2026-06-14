# 12. iOS and Android mobile client (Tauri)

Status: to do. Priority: medium. Effort: L.

## Objective

Distribute LunaBet as a native application on the App Store and the Play Store, with a Tauri 2 (mobile) client. The goal is not to rewrite the app but to wrap the existing web experience in a native shell, adding what the web alone does not do well on mobile: presence on the home screen via the stores, reliable native push notifications (especially on iOS), and system integration (icon, splash, sharing).

## Why Tauri

- The app is rendered server-side (Askama + htmx), so it is lightweight and well-suited to display in a webview.
- Tauri 2 supports iOS and Android and produces native binaries with a Rust core, consistent with the backend stack.
- Smaller footprint than Electron / React Native, and reuse of the team's Rust expertise.

## Architecture

**Decision made: remote shell.** The webview loads the deployed site directly (the tenant's apex or subdomain). The mobile binary does not embed the UI; it provides the native shell, push, deep links and session persistence. Key advantage: instant product updates on the server side, without resubmission to the store for a UI change. We reuse the server-rendered app as is, without having to extract a JSON API.

Alternative ruled out for launch: assets embedded in the bundle talking to a dedicated JSON API. High cost (we would have to split an API out of the server-rendered app) and loss of instant updates. To reconsider only if a strong offline need arises.

Space selection: on first launch, the user enters or chooses their space (slug), or logs in through the apex central login that redirects to the right tenant ([src/routes/auth.rs](../src/routes/auth.rs), central flow already existing). The chosen slug is persisted on the app side.

## Product-side prerequisites

- [08-pwa-push](08-pwa-push.md) must be delivered first: manifest, service worker and above all the push subscription infrastructure (`push_subscriptions`, VAPID keys, sending from [src/notifications.rs](../src/notifications.rs)). The Tauri client reuses this foundation and adds native push to it.

## Components

### Tauri project
- New `mobile/` folder (or a dedicated repository) with the Tauri 2 structure: `src-tauri/`, `tauri.conf.json` configuration, iOS and Android targets.
- Webview pointing to the space URL, with network and storage permissions.
- Persistence of the `lb_session` session cookie between launches (the webview must keep the cookies; otherwise store the slug and let the magic link recreate the session).

### Authentication and deep links
- Magic links and invitation links arrive by email and open in the system browser. Configure **universal links (iOS)** and **app links (Android)** on the domain so that `/auth/callback` and `/invite/accept` open the app rather than the browser.
- Server side: serve `apple-app-site-association` and `assetlinks.json` (a new static route, for example in [src/routes/seo.rs](../src/routes/seo.rs) or a dedicated module).
- On return from the deep link, the webview navigates to the authenticated target and the session is set as on the web.

### Native push notifications
- iOS: APNs. Android: FCM. The Tauri push plugin (or a community plugin) provides the device token.
- Reuse the `push_subscriptions` table from [08-pwa-push](08-pwa-push.md), distinguishing the channel:

```sql
-- migrations/2026xxxx_push_native.sql
ALTER TABLE push_subscriptions ADD COLUMN platform TEXT NOT NULL DEFAULT 'web'
    CHECK (platform IN ('web', 'ios', 'android'));
ALTER TABLE push_subscriptions ADD COLUMN device_token TEXT;
```

- On the backend, sending branches according to `platform`: web-push (VAPID) for `web`, APNs for `ios`, FCM for `android`. Encapsulate this in a `PushChannel` trait called by [src/notifications.rs](../src/notifications.rs), so that the triggering logic (reminders, rank alerts) stays shared.
- Registration of the native token goes through the same `POST /push/subscribe` route, with `platform` and `device_token`.

### System integration
- Icon and splash screen in LunaBet colors (reuse `favicon.svg`, default tenant palette).
- Status bar and safe areas (notch) handled by the Tauri config.
- Native share link for league codes ([04-private-leagues](04-private-leagues.md)) and invitations ([11-invite-based-orgs](11-invite-based-orgs.md)).

## Backend, impact

- Add the domain association files (`apple-app-site-association`, `assetlinks.json`).
- Generalize multi-channel push sending (trait + APNs / FCM / web-push implementations).
- No change to the game logic: bets, scoring, ranking and pot remain on the server.

## CI / distribution

- Separate mobile build pipeline (iOS signing via Apple Developer, Android keystore signing).
- Store accounts, listings, screenshots (reuse [docs/screenshots](../docs/screenshots)).
- Store policies: a prediction app with a real money stake (pot) can trigger "gambling" rules. Important: the LunaBet pot works on the honor system, the app handles no payment (cf. [src/stakes.rs](../src/stakes.rs)). To be documented clearly in the store listing and to be verified very early, because it is the main risk of rejection.

## Edge cases

- Session expired in the webview: fall back to the login screen, magic link via deep link.
- Multi-space: if the user belongs to several spaces, offer a selector (the central login already handles the multi-tenant case).
- Notifications refused: degrade to email, do not prompt again in a loop.
- Store review: provide a demo account and the dev mode ([src/routes/dev.rs](../src/routes/dev.rs)) for the reviewers.

## Acceptance criteria

- The app launches, remembers the space, and displays the web experience in standalone mode on iOS and Android.
- A magic link or an invitation link opens the app via deep link and authenticates the user.
- Native push notifications arrive on both platforms via the same trigger as the web.
- No server-side regression: the web continues to work identically.
