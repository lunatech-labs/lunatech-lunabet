# 11. Invite-based spaces (friends mode)

Status: to do. Priority: high. Effort: M.

## Objective

Add a second way to create and join a space, aimed at a general audience ("friends"), where membership no longer depends on the email domain but on explicit invitations. The creator becomes the administrator, can invite whomever they want, and each invited member can in turn invite other people. An invitee joins the space by clicking a link received by email.

The current mode based on the email domain ("company") remains available and unchanged.

## Existing context

- A space is a `tenant` ([src/tenant.rs](../src/tenant.rs)). Membership is implicit: there is a `users (tenant_id, email)` row.
- On login ([src/routes/auth.rs](../src/routes/auth.rs), `tenant_request_magic_link`), access is filtered by `allowed_email_pattern` on the email domain.
- Self-serve creation goes through `pending_tenants` and then a verification link ([src/routes/signup.rs](../src/routes/signup.rs)), which derives `allowed_email_pattern` from the owner's domain.

Friends mode replaces domain gating with invitation gating, without touching the rest of the mechanics (bets, scoring, ranking, pot).

## Concepts

- **membership_mode** on the tenant: `domain` (current, company) or `invite` (friends).
- **Invitation**: a record (tenant, invited email, inviter, token, status, expiration).
- **Member**: a `users` row in the tenant. Inviting does not create the member; accepting creates it.

## Unified gating rule

A login attempt for email E on tenant T is allowed if one of the following conditions is true:

1. A `users (tenant_id = T, email = E)` already exists (established member), OR
2. `T.membership_mode = 'domain'` AND `T.allowed_email_pattern` matches the domain of E (company auto-join), OR
3. There is a non-expired **pending** invitation for (T, E).

This rule unifies the two modes:
- In `domain` mode, condition 2 keeps the current behavior; invitations remain possible in addition (inviting someone outside the domain).
- In `invite` mode, `allowed_email_pattern` matches nothing (a "match nothing" pattern), so only conditions 1 and 3 grant access.

Implementation in `tenant_request_magic_link` ([src/routes/auth.rs](../src/routes/auth.rs)): replace the single pattern test with this `is_login_allowed(pool, tenant, email)` function.

## Data model

```sql
-- migrations/2026xxxx_invite_mode.sql

-- Membership mode of the space.
ALTER TABLE tenants ADD COLUMN membership_mode TEXT NOT NULL DEFAULT 'domain'
    CHECK (membership_mode IN ('domain', 'invite'));

-- Whether non-admin members are allowed to invite (friends mode: TRUE by default).
ALTER TABLE tenants ADD COLUMN members_can_invite BOOLEAN NOT NULL DEFAULT TRUE;

CREATE TABLE invitations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID NOT NULL REFERENCES tenants(id),
    email           TEXT NOT NULL,          -- invitee, in lowercase
    inviter_user_id UUID REFERENCES users(id),  -- NULL if generated at space creation
    token_hash      TEXT NOT NULL UNIQUE,   -- we store only the hash
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'accepted', 'revoked', 'expired')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at      TIMESTAMPTZ NOT NULL,
    accepted_at     TIMESTAMPTZ,
    accepted_user_id UUID REFERENCES users(id)
);

-- A single live invitation per (tenant, email).
CREATE UNIQUE INDEX invitations_pending_uidx
    ON invitations (tenant_id, email)
    WHERE status = 'pending';

CREATE INDEX invitations_tenant_idx ON invitations (tenant_id);
CREATE INDEX invitations_expires_idx ON invitations (expires_at)
    WHERE status = 'pending';
```

"Match nothing" convention: for a tenant in `invite` mode, store `allowed_email_pattern = '(?!)'` (a regex that never matches). The `Tenant::try_from` function already compiles the pattern, and `(?!)` is valid.

## Flow 1: create a space in friends mode

Extend the existing signup ([src/routes/signup.rs](../src/routes/signup.rs)).

1. The `/signup` form ([templates/signup.html](../templates/signup.html)) gains a space type choice:
   - "Company (email domain)": current behavior, asks for the domain via the owner's email.
   - "Friends (by invitation)": no domain; any owner email is accepted.
2. `SignupForm` receives a `space_kind` field (`domain` or `invite`).
3. In `invite` mode:
   - the stored `allowed_email_pattern` is `(?!)`.
   - `pending_tenants` must remember the chosen mode: add a column.

```sql
ALTER TABLE pending_tenants ADD COLUMN membership_mode TEXT NOT NULL DEFAULT 'domain'
    CHECK (membership_mode IN ('domain', 'invite'));
```

4. At verification time (`verify`), the tenant INSERT sets `membership_mode` and `members_can_invite = TRUE` for friends mode. The owner is created as an admin member (already the case).

No invitation is required for the owner: they are the founding admin.

## Flow 2: invite people

New routes module `src/routes/invitations.rs`.

- `GET /members`: lists the space's members and the pending invitations; invitation form. Accessible to any member if `members_can_invite`, otherwise admin only.
- `POST /invitations`: creates one or more invitations (email field, possibly a list). For each email:
  - normalize to lowercase,
  - if already a member: do not create an invitation, report "already a member",
  - if a pending invitation already exists: return the existing link (do not duplicate),
  - otherwise create the invitation (random token, stored hash, `expires_at = NOW() + 7 days`), send the email.
- `POST /invitations/:id/revoke`: sets `status = 'revoked'` (inviter or admin).
- `POST /invitations/:id/resend`: regenerates the token if expired or resends the email.

Permissions:
- Any member can invite if `tenants.members_can_invite = TRUE`.
- An admin can always invite and can toggle `members_can_invite` in [templates/admin_settings.html](../templates/admin_settings.html).
- Revocation: the inviter of the invitation or an admin.

Anti-abuse:
- Rate limit per user (for example 20 invitations / day) by reusing the mechanism in [src/rate_limit.rs](../src/rate_limit.rs).
- Cap on pending invitations per tenant (configurable, generous default).
- Honeypot not needed here (authenticated action), but log the inviter + email.

Invitation email: new template [templates/emails/invitation.html](../templates/emails/invitation.html), bilingual according to the inviter's locale or the tenant's default locale. Content: who is inviting, the space name, a "Join" button, mention of the expiration. The link points to the tenant's apex or subdomain: `{tenant_public_url}/invite/accept?token=...`.

## Flow 3: accept an invitation

Route `GET /invite/accept?token=...` (in `src/routes/invitations.rs`), served on the target tenant.

1. Hash the token, load the invitation by `token_hash` and the current `tenant_id`.
2. Rejections: unknown token (invalid link), `status != 'pending'` (already used or revoked), expired (`expires_at < NOW()`, mark as `expired`).
3. If valid, in a single transaction:
   - upsert the member: `INSERT INTO users (tenant_id, email, display_name) ... ON CONFLICT (tenant_id, email) DO NOTHING`, the `display_name` derived from the email as in `callback`.
   - mark the invitation `accepted`, set `accepted_at` and `accepted_user_id`.
   - create a session and set the `lb_session` cookie (same logic as `auth::callback`, including `cookie_domain` in multi-tenant).
4. Redirect to `/today`.

The invitation token therefore acts as the first authentication: the invitee does not need a separate magic link for their first access. Subsequent logins go through the normal magic link, now allowed by the gating rule (condition 1, established member).

Transitivity: once a member, the user sees `/members` and can invite in turn if `members_can_invite`. This is what realizes "an invitee can invite other people".

## Maintenance job

Extend the existing hourly cleanup job ([src/main.rs](../src/main.rs)): move expired `pending` invitations to `expired`. Idempotent.

## UI

- [templates/signup.html](../templates/signup.html): space type selector, contextual help. Hide the domain field in friends mode.
- [templates/members.html](../templates/members.html) (new): members, pending invitations (status, expiration), invitation form, copy link button.
- [templates/emails/invitation.html](../templates/emails/invitation.html) (new).
- [templates/_nav.html](../templates/_nav.html): "Members" entry visible according to permissions.
- [templates/admin_settings.html](../templates/admin_settings.html): "Allow members to invite" toggle, and a membership mode selector (see Admin settings below).

## Admin settings: mode switch

In [src/routes/tenant_settings.rs](../src/routes/tenant_settings.rs), expose:

- A `membership_mode` selector: "Company (email domain)" / "Friends (by invitation)".
- When `domain` is chosen: editable `allowed_email_pattern` field (allowed domain).
- When `invite` is chosen: domain field hidden; on save, the server forces `allowed_email_pattern = '(?!)'`.
- Warning shown before confirmation: "In Friends mode, only invited people will be able to join. Current members remain members."
- After the write, invalidate the tenant cache via `TenantRegistry::invalidate` (already the pattern used after other settings edits) so the gating rule takes effect immediately.

## i18n

- "Inviter" / "Invite", "Membres" / "Members", "Invitation en cours" / "Pending invitation", "Rejoindre l'espace" / "Join the space", "Cette invitation a expire" / "This invitation has expired", "Type d'espace" / "Space type", "Entreprise" / "Company", "Amis" / "Friends".

## Security

- Tokens: 32 random bytes, only the SHA-256 hash is stored, like magic links and signup.
- Invitations are strictly tenant-scoped; a token from one tenant cannot grant entry into another (join on `tenant_id`).
- The existing RLS must cover `invitations` (add the per-tenant policy, cf. [migrations/20260525000007_rls.sql](../migrations/20260525000007_rls.sql)).
- No disclosure: the acceptance page does not reveal whether an email is already a member.

## Edge cases

- Email that is already a member invited again: no new invitation, we can resend them a regular login link.
- Invitation accepted from another device / broken email: the cookie is set for the session of the browser that opens the link, consistent with the magic link.
- Space in friends mode with no invitation at all: only the owner can enter, which is normal.
- Mode switch `domain` <-> `invite` by an admin: **allowed** (decision made). Existing members always remain members regardless of direction. When switching to `invite`, domain auto-join stops, we replace `allowed_email_pattern` with `(?!)`; newcomers then come in through invitation. When switching back to `domain`, the admin re-enters a domain pattern and auto-join resumes. The toggle lives in [templates/admin_settings.html](../templates/admin_settings.html) with clear warning text about the effect (cf. the Admin settings section below).
- Invitation to a domain that would already match a company tenant: no effect, each tenant has its own `users` rows.

## Acceptance criteria

- Creating a space in friends mode requires no domain and makes the owner an admin.
- In friends mode, an uninvited email cannot obtain a valid magic link; an invited email can, or enters directly via the acceptance link.
- Any member (if allowed) can invite, and an invitee who has become a member can invite in turn.
- An invitation is unique while it is pending, expires after 7 days, and can be revoked.
- The existing company mode works exactly as before.
