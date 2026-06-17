use askama::Template;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::PrivateCookieJar;
use chrono::{Duration, Utc};
use serde::Deserialize;
use uuid::Uuid;

use crate::error::AppResult;
use crate::i18n::Locale;
use crate::state::AppState;
use crate::tenant::{Tenant, TenantCtx};

const SESSION_TTL_DAYS: i64 = 30;

struct DevUser {
    email: String,
    display_name: String,
    points: i64,
}

#[derive(Template)]
#[template(path = "dev.html")]
struct DevTpl<'a> {
    loc: Locale,
    tenant: &'a Tenant,
    users: Vec<DevUser>,
}

pub async fn index(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    loc: Locale,
) -> AppResult<Response> {
    if !state.cfg.dev_mode {
        return Ok((StatusCode::NOT_FOUND, "Dev mode disabled.").into_response());
    }

    let users: Vec<(String, String, Option<i64>)> = sqlx::query_as(
        r#"
        SELECT u.email, u.display_name, COALESCE(SUM(b.points), 0)::BIGINT
        FROM users u
        LEFT JOIN bets b ON b.user_id = u.id AND b.tenant_id = u.tenant_id
        WHERE u.tenant_id = $1
        GROUP BY u.email, u.display_name
        ORDER BY u.display_name ASC
        "#,
    )
    .bind(tenant.id)
    .fetch_all(&state.pool)
    .await?;

    let users = users
        .into_iter()
        .map(|(email, display_name, points)| DevUser {
            email,
            display_name,
            points: points.unwrap_or(0),
        })
        .collect();

    Ok(Html(DevTpl { loc, tenant: &tenant, users }.render()?).into_response())
}

#[derive(Deserialize)]
pub struct LoginQuery {
    email: String,
}

pub async fn login_as(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    jar: PrivateCookieJar,
    Query(q): Query<LoginQuery>,
) -> AppResult<Response> {
    if !state.cfg.dev_mode {
        return Ok((StatusCode::NOT_FOUND, "Dev mode disabled.").into_response());
    }

    let email = q.email.trim().to_lowercase();

    let user_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM users WHERE tenant_id = $1 AND email = $2",
    )
    .bind(tenant.id)
    .bind(&email)
    .fetch_optional(&state.pool)
    .await?;

    let Some(user_id) = user_id else {
        return Ok((StatusCode::NOT_FOUND, "Utilisateur introuvable. Lance d'abord `cargo run -- seed`.").into_response());
    };

    // sync admin flag from the tenant's admin_emails on each dev login
    let is_admin = tenant.is_admin(&email);
    sqlx::query("UPDATE users SET is_admin = $1 WHERE id = $2 AND tenant_id = $3")
        .bind(is_admin)
        .bind(user_id)
        .bind(tenant.id)
        .execute(&state.pool)
        .await?;

    let session_id = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(SESSION_TTL_DAYS);
    sqlx::query(
        "INSERT INTO sessions (id, tenant_id, user_id, expires_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(session_id)
    .bind(tenant.id)
    .bind(user_id)
    .bind(expires_at)
    .execute(&state.pool)
    .await?;

    let jar = crate::routes::auth::set_session_cookie(jar, &state.cfg, session_id);

    Ok((jar, Redirect::to("/today")).into_response())
}
