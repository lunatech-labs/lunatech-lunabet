//! Invitations: let a member invite people to their space by email, and let an
//! invitee join by clicking the emailed link. Works in both membership modes;
//! in `invite` (friends) mode it is the only way in besides the founder.
//!
//! The accept link doubles as the invitee's first sign-in: clicking it creates
//! their account and a session, so they don't need a separate magic link.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use axum_extra::extract::cookie::PrivateCookieJar;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::AppResult;
use crate::i18n::Locale;
use crate::mail;
use crate::models::User;
use crate::rate_limit::client_ip;
use crate::routes::auth::AuthUser;
use crate::stakes;
use crate::state::AppState;
use crate::tenant::{Tenant, TenantCtx};

const SESSION_TTL_DAYS: i64 = 30;
const INVITE_TTL_DAYS: i64 = 7;

pub struct MemberRow {
    pub name: String,
    pub email: String,
    pub is_admin: bool,
}

pub struct InviteRow {
    pub id: Uuid,
    pub email: String,
    pub expires_label: String,
}

#[derive(Template)]
#[template(path = "members.html")]
struct MembersTpl<'a> {
    loc: Locale,
    tenant: &'a Tenant,
    user_name: &'a str,
    total_points: i32,
    is_admin: bool,
    nav_active: &'static str,
    can_invite: bool,
    members: Vec<MemberRow>,
    invites: Vec<InviteRow>,
    notice: Option<String>,
    error: Option<String>,
}

fn can_invite(tenant: &Tenant, user: &User) -> bool {
    user.is_admin || tenant.members_can_invite
}

async fn render_members(
    state: &AppState,
    tenant: &Tenant,
    loc: Locale,
    user: &User,
    notice: Option<String>,
    error: Option<String>,
) -> AppResult<Response> {
    let members: Vec<MemberRow> = sqlx::query_as::<_, (String, String, bool)>(
        "SELECT display_name, email, is_admin FROM users WHERE tenant_id = $1 \
         ORDER BY is_admin DESC, display_name ASC",
    )
    .bind(tenant.id)
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|(name, email, is_admin)| MemberRow { name, email, is_admin })
    .collect();

    let invites: Vec<InviteRow> = sqlx::query_as::<_, (Uuid, String, DateTime<Utc>)>(
        "SELECT id, email, expires_at FROM invitations \
         WHERE tenant_id = $1 AND status = 'pending' ORDER BY created_at DESC",
    )
    .bind(tenant.id)
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|(id, email, expires_at)| InviteRow {
        id,
        email,
        expires_label: expires_at.format("%d/%m/%Y").to_string(),
    })
    .collect();

    let board = stakes::load_leaderboard(&state.pool, tenant.id).await?;
    let tpl = MembersTpl {
        loc,
        tenant,
        user_name: &user.display_name,
        total_points: stakes::points_for(&board, user.id),
        is_admin: user.is_admin,
        nav_active: "members",
        can_invite: can_invite(tenant, user),
        members,
        invites,
        notice,
        error,
    };
    Ok(Html(tpl.render()?).into_response())
}

pub async fn members_page(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    loc: Locale,
    AuthUser(user): AuthUser,
) -> AppResult<Response> {
    render_members(&state, &tenant, loc, &user, None, None).await
}

#[derive(Deserialize)]
pub struct InviteForm {
    emails: String,
}

/// Split a free-text field into candidate emails, on commas, whitespace and
/// newlines. Lowercased, de-duplicated, only entries that contain an `@`.
fn parse_emails(raw: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for part in raw.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
        let e = part.trim().to_lowercase();
        if e.contains('@') && e.len() <= 120 && seen.insert(e.clone()) {
            out.push(e);
        }
    }
    out
}

pub async fn create(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    loc: Locale,
    AuthUser(user): AuthUser,
    headers: HeaderMap,
    Form(form): Form<InviteForm>,
) -> AppResult<Response> {
    if !can_invite(&tenant, &user) {
        return Ok((StatusCode::FORBIDDEN, "Not allowed.").into_response());
    }
    if let Some(ip) = client_ip(&headers) {
        if !state.endpoint_limiter.check_and_record("/invitations", ip) {
            let msg = loc
                .f(
                    "Trop d'invitations envoyées. Réessaie dans un moment.",
                    "Too many invitations sent. Try again in a moment.",
                )
                .to_string();
            return render_members(&state, &tenant, loc, &user, None, Some(msg)).await;
        }
    }

    let emails = parse_emails(&form.emails);
    if emails.is_empty() {
        let msg = loc
            .f("Aucune adresse email valide.", "No valid email address.")
            .to_string();
        return render_members(&state, &tenant, loc, &user, None, Some(msg)).await;
    }

    let tenant_url = tenant.public_url(&state.cfg);
    let (mut invited, mut already, mut failed) = (0usize, 0usize, 0usize);

    for email in emails {
        let is_member: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM users WHERE tenant_id = $1 AND email = $2)",
        )
        .bind(tenant.id)
        .bind(&email)
        .fetch_one(&state.pool)
        .await?;
        if is_member {
            already += 1;
            continue;
        }

        let token = random_token();
        let token_hash = hex_sha256(&token);
        let expires_at = Utc::now() + Duration::days(INVITE_TTL_DAYS);

        // Reuse a live pending invitation for this address if one exists
        // (refresh its token and deadline), otherwise create a new one.
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM invitations \
             WHERE tenant_id = $1 AND email = $2 AND status = 'pending'",
        )
        .bind(tenant.id)
        .bind(&email)
        .fetch_optional(&state.pool)
        .await?;

        if let Some(id) = existing {
            sqlx::query(
                "UPDATE invitations SET token_hash = $2, expires_at = $3, \
                 created_at = NOW(), inviter_user_id = $4 WHERE id = $1",
            )
            .bind(id)
            .bind(&token_hash)
            .bind(expires_at)
            .bind(user.id)
            .execute(&state.pool)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO invitations (tenant_id, email, inviter_user_id, token_hash, expires_at) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(tenant.id)
            .bind(&email)
            .bind(user.id)
            .bind(&token_hash)
            .bind(expires_at)
            .execute(&state.pool)
            .await?;
        }

        let link = format!("{}/invite/accept?token={}", tenant_url, token);
        match mail::send_invitation(
            &state.cfg,
            &tenant,
            loc,
            &tenant_url,
            &email,
            &user.display_name,
            &link,
        )
        .await
        {
            Ok(_) => invited += 1,
            Err(e) => {
                failed += 1;
                tracing::warn!("invitation email to {email} failed: {e:#}");
                tracing::info!("DEV invitation link for {email}: {link}");
            }
        }
    }

    let notice = match loc {
        Locale::Fr => format!(
            "{invited} invitation(s) envoyée(s), {already} déjà membre(s), {failed} échec(s).",
        ),
        Locale::En => {
            format!("{invited} invitation(s) sent, {already} already member(s), {failed} failed.")
        }
    };
    render_members(&state, &tenant, loc, &user, Some(notice), None).await
}

pub async fn revoke(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    loc: Locale,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    // The inviter or any admin may revoke. Scoped to this tenant.
    let cond = if user.is_admin {
        sqlx::query(
            "UPDATE invitations SET status = 'revoked' \
             WHERE id = $1 AND tenant_id = $2 AND status = 'pending'",
        )
        .bind(id)
        .bind(tenant.id)
    } else {
        sqlx::query(
            "UPDATE invitations SET status = 'revoked' \
             WHERE id = $1 AND tenant_id = $2 AND status = 'pending' AND inviter_user_id = $3",
        )
        .bind(id)
        .bind(tenant.id)
        .bind(user.id)
    };
    cond.execute(&state.pool).await?;
    let _ = loc;
    Ok(Redirect::to("/members").into_response())
}

#[derive(Deserialize)]
pub struct AcceptQuery {
    token: String,
}

pub async fn accept(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    loc: Locale,
    jar: PrivateCookieJar,
    Query(q): Query<AcceptQuery>,
) -> AppResult<Response> {
    let token_hash = hex_sha256(&q.token);

    let row: Option<(Uuid, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, email, status, expires_at FROM invitations \
         WHERE token_hash = $1 AND tenant_id = $2",
    )
    .bind(&token_hash)
    .bind(tenant.id)
    .fetch_optional(&state.pool)
    .await?;

    let Some((invite_id, email, status, expires_at)) = row else {
        return Ok((StatusCode::BAD_REQUEST, loc.f("Lien invalide.", "Invalid link.")).into_response());
    };
    if status != "pending" {
        return Ok((
            StatusCode::BAD_REQUEST,
            loc.f(
                "Cette invitation n'est plus valable.",
                "This invitation is no longer valid.",
            ),
        )
            .into_response());
    }
    if expires_at < Utc::now() {
        sqlx::query("UPDATE invitations SET status = 'expired' WHERE id = $1")
            .bind(invite_id)
            .execute(&state.pool)
            .await?;
        return Ok((
            StatusCode::BAD_REQUEST,
            loc.f("Cette invitation a expiré.", "This invitation has expired."),
        )
            .into_response());
    }

    let display_name = email
        .split('@')
        .next()
        .unwrap_or(&email)
        .replace('.', " ")
        .replace('_', " ");

    let mut tx = state.pool.begin().await?;

    // Create the member if missing; an existing account is fine (the invite
    // still signs them in).
    let user_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO users (tenant_id, email, display_name, is_admin, lang)
        VALUES ($1, $2, $3, FALSE, $4)
        ON CONFLICT (tenant_id, email) DO UPDATE SET email = EXCLUDED.email
        RETURNING id
        "#,
    )
    .bind(tenant.id)
    .bind(&email)
    .bind(&display_name)
    .bind(loc.code())
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE invitations SET status = 'accepted', accepted_at = NOW(), accepted_user_id = $2 \
         WHERE id = $1",
    )
    .bind(invite_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    let session_id = Uuid::new_v4();
    let session_expires = Utc::now() + Duration::days(SESSION_TTL_DAYS);
    sqlx::query("INSERT INTO sessions (id, tenant_id, user_id, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(session_id)
        .bind(tenant.id)
        .bind(user_id)
        .bind(session_expires)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    let jar = crate::routes::auth::set_session_cookie(jar, &state.cfg, session_id);
    Ok((jar, Redirect::to("/today")).into_response())
}

fn random_token() -> String {
    let mut raw = [0u8; 32];
    rand::rng().fill_bytes(&mut raw);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

fn hex_sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let out = hasher.finalize();
    let mut s = String::with_capacity(out.len() * 2);
    for b in out {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}
