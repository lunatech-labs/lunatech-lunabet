//! Import predictions from another space (tenant) you also belong to.
//!
//! A person can be a member of several spaces for the same tournament (same
//! `football_competition`). Matches are global (one row per football-data
//! fixture, shared by every tenant), so a prediction in space A and the same
//! prediction in space B differ only by `(tenant_id, user_id)` -- the
//! `match_id` is identical. The two memberships are linked by the user's
//! verified email.
//!
//! This lets a member copy their predictions across, instead of typing them
//! twice. To stay fair we only ever copy predictions for matches still open
//! for bets (kick-off in the future): importing a prediction is then exactly
//! like typing it in time. We never overwrite a prediction already placed in
//! the current space (fill-the-blanks only, via `ON CONFLICT DO NOTHING`).
//!
//! "Same tournament" is decided by the two tenants sharing the same
//! `football_competition`, NOT by comparing it to `matches.competition`: the
//! tenant value is the football-data competition *code* (e.g. `WC`) while the
//! match column stores the *name* the API returns (e.g. `FIFA World Cup`), so
//! the two never match in production. Matches are global anyway, so tenant
//! equality is the right guarantee.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect, Response};

use crate::error::AppResult;
use crate::models::User;
use crate::routes::auth::AuthUser;
use crate::state::AppState;
use crate::tenant::{Tenant, TenantCtx};

/// Another space the signed-in member belongs to, with how many of its
/// predictions could still be imported into the current space.
pub struct ImportSource {
    pub slug: String,
    pub name: String,
    pub count: i64,
}

/// Find the other spaces (tenants) the member belongs to -- matched by their
/// verified email -- that run the same tournament and hold predictions still
/// importable here: a future match they bet on there but haven't bet on here.
pub async fn detect_sources(
    state: &AppState,
    tenant: &Tenant,
    user: &User,
) -> AppResult<Vec<ImportSource>> {
    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        r#"
        SELECT t.slug, t.name, COUNT(*) AS n
        FROM users su
        JOIN tenants t ON t.id = su.tenant_id
        JOIN bets sb   ON sb.user_id = su.id AND sb.tenant_id = su.tenant_id
        JOIN matches m ON m.id = sb.match_id
        WHERE su.email = $1
          AND su.tenant_id <> $2
          AND t.football_competition = $3
          AND m.kickoff_at > NOW()
          -- Open for bets: not yet started. Mirrors models::has_kicked_off so a
          -- pre-match status other than SCHEDULED/TIMED is still importable.
          AND m.status NOT IN
              ('IN_PLAY', 'PAUSED', 'EXTRA_TIME', 'PENALTY_SHOOTOUT', 'FINISHED', 'AWARDED', 'SUSPENDED')
          AND NOT EXISTS (
                SELECT 1 FROM bets tb
                WHERE tb.user_id = $4 AND tb.tenant_id = $2 AND tb.match_id = sb.match_id
          )
        GROUP BY t.slug, t.name
        ORDER BY t.name ASC
        "#,
    )
    .bind(&user.email)
    .bind(tenant.id)
    .bind(&tenant.football_competition)
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(slug, name, count)| ImportSource { slug, name, count })
        .collect())
}

/// Copy the member's predictions from `source_slug` into the current space.
///
/// One self-contained statement: it resolves the source space by slug and the
/// source membership by the signed-in email, and copies only predictions for
/// matches of the same tournament that are still open. `ON CONFLICT DO NOTHING`
/// keeps any prediction already placed here untouched. Redirects back to the
/// predictions screen with the number actually imported.
pub async fn import(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    AuthUser(user): AuthUser,
    Path(source_slug): Path<String>,
) -> AppResult<Response> {
    let inserted = sqlx::query(
        r#"
        INSERT INTO bets (tenant_id, user_id, match_id, home_score, away_score)
        SELECT $1, $2, sb.match_id, sb.home_score, sb.away_score
        FROM bets sb
        JOIN users su   ON su.id = sb.user_id AND su.tenant_id = sb.tenant_id
        JOIN tenants st ON st.id = su.tenant_id
        JOIN matches m  ON m.id = sb.match_id
        WHERE st.slug = $3
          AND st.slug <> $4
          AND su.email = $5
          AND st.football_competition = $6
          AND m.kickoff_at > NOW()
          -- Open for bets: not yet started. Mirrors models::has_kicked_off so a
          -- pre-match status other than SCHEDULED/TIMED is still importable.
          AND m.status NOT IN
              ('IN_PLAY', 'PAUSED', 'EXTRA_TIME', 'PENALTY_SHOOTOUT', 'FINISHED', 'AWARDED', 'SUSPENDED')
        ON CONFLICT (user_id, match_id) DO NOTHING
        "#,
    )
    .bind(tenant.id)
    .bind(user.id)
    .bind(&source_slug)
    .bind(&tenant.slug)
    .bind(&user.email)
    .bind(&tenant.football_competition)
    .execute(&state.pool)
    .await?
    .rows_affected();

    Ok(Redirect::to(&format!("/matches?imported={inserted}")).into_response())
}
