use askama::Template;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Form;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::AppResult;
use crate::i18n::Locale;
use crate::models::{self, Match};
use crate::routes::auth::AuthUser;
use crate::routes::matches::MatchView;
use crate::state::AppState;
use crate::tenant::TenantCtx;

#[derive(Deserialize)]
pub struct BetForm {
    home_score: i32,
    away_score: i32,
}

#[derive(Template)]
#[template(path = "match_card.html")]
struct MatchCardTpl {
    loc: Locale,
    v: MatchView,
}

pub async fn place_or_update(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    loc: Locale,
    AuthUser(user): AuthUser,
    Path(match_id): Path<i64>,
    headers: HeaderMap,
    Form(form): Form<BetForm>,
) -> AppResult<Response> {
    if !(0..=30).contains(&form.home_score) || !(0..=30).contains(&form.away_score) {
        return Ok((StatusCode::BAD_REQUEST, loc.f("Score invalide (0-30).", "Invalid score (0-30).")).into_response());
    }

    let row: Option<(DateTime<Utc>, String)> =
        sqlx::query_as("SELECT kickoff_at, status FROM matches WHERE id = $1")
            .bind(match_id)
            .fetch_optional(&state.pool)
            .await?;

    let Some((kickoff, status)) = row else {
        return Ok((StatusCode::NOT_FOUND, loc.f("Match introuvable.", "Match not found.")).into_response());
    };
    if !models::betting_open(kickoff, &status) {
        return Ok((StatusCode::FORBIDDEN, loc.f("Les paris sont fermés pour ce match.", "Bets are closed for this match.")).into_response());
    }

    sqlx::query(
        r#"
        INSERT INTO bets (tenant_id, user_id, match_id, home_score, away_score)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (user_id, match_id) DO UPDATE
        SET home_score = EXCLUDED.home_score,
            away_score = EXCLUDED.away_score,
            updated_at = NOW()
        "#,
    )
    .bind(tenant.id)
    .bind(user.id)
    .bind(match_id)
    .bind(form.home_score)
    .bind(form.away_score)
    .execute(&state.pool)
    .await?;

    // htmx submission: return just the updated match card so the form swaps
    // in place (no full reload, no scroll jump). Fall back to a redirect for
    // plain-HTML clients.
    if headers.get("hx-request").is_some() {
        return render_match_card(&state, &tenant, loc, user.id, match_id).await;
    }

    // Non-htmx fallback: redirect with a fragment so the browser at least
    // scrolls back to the card the user just edited.
    Ok(Redirect::to(&format!("/matches#match-{match_id}")).into_response())
}

/// Render just one match card for the given user, reflecting their current bet
/// (score, points, joker state). Shared by the bet form and the joker toggle so
/// both htmx swaps produce an identical, up-to-date card.
async fn render_match_card(
    state: &AppState,
    tenant: &crate::tenant::Tenant,
    loc: Locale,
    user_id: uuid::Uuid,
    match_id: i64,
) -> AppResult<Response> {
    let m: Match = sqlx::query_as(
        r#"
        SELECT id, competition, stage, group_name,
               home_team, away_team, home_team_code, away_team_code,
               kickoff_at, status, home_score, away_score
        FROM matches WHERE id = $1
        "#,
    )
    .bind(match_id)
    .fetch_one(&state.pool)
    .await?;

    let bet: Option<(i32, i32, Option<i32>, i32)> = sqlx::query_as(
        "SELECT home_score, away_score, points, multiplier FROM bets \
         WHERE user_id = $1 AND match_id = $2 AND tenant_id = $3",
    )
    .bind(user_id)
    .bind(match_id)
    .bind(tenant.id)
    .fetch_optional(&state.pool)
    .await?;

    let view = MatchView {
        open: m.is_open_for_bets(),
        finished: m.is_concluded(),
        bet_home: bet.map(|b| b.0),
        bet_away: bet.map(|b| b.1),
        points: bet.and_then(|b| b.2),
        is_joker: bet.map(|b| b.3 == 2).unwrap_or(false),
        jokers_enabled: tenant.jokers_enabled,
        m,
    };
    let tpl = MatchCardTpl { loc, v: view };
    Ok(Html(tpl.render()?).into_response())
}

/// Toggle the player's joker on a match. Rules (see spec 05):
/// - jokers must be enabled for the tenant;
/// - the target match must still be open and already have a bet to double;
/// - at most one joker per competition phase (`matches.stage`): toggling a
///   second match in the same phase MOVES the joker, but only while the
///   currently-jokered match hasn't kicked off yet (otherwise the choice is
///   locked). All in one transaction.
pub async fn toggle_joker(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    loc: Locale,
    AuthUser(user): AuthUser,
    Path(match_id): Path<i64>,
    headers: HeaderMap,
) -> AppResult<Response> {
    if !tenant.jokers_enabled {
        return Ok((
            StatusCode::FORBIDDEN,
            loc.f("Les jokers ne sont pas activés.", "Jokers are not enabled."),
        )
            .into_response());
    }

    // Target match must exist, be open for bets, and already carry a bet.
    let target: Option<(DateTime<Utc>, String, Option<String>)> =
        sqlx::query_as("SELECT kickoff_at, status, stage FROM matches WHERE id = $1")
            .bind(match_id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((kickoff, status, stage)) = target else {
        return Ok((StatusCode::NOT_FOUND, loc.f("Match introuvable.", "Match not found.")).into_response());
    };
    if !models::betting_open(kickoff, &status) {
        return Ok((
            StatusCode::FORBIDDEN,
            loc.f("Ce match est fermé.", "This match is closed."),
        )
            .into_response());
    }

    let mut tx = state.pool.begin().await?;

    let current: Option<i32> = sqlx::query_scalar(
        "SELECT multiplier FROM bets WHERE user_id = $1 AND match_id = $2 AND tenant_id = $3",
    )
    .bind(user.id)
    .bind(match_id)
    .bind(tenant.id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(current) = current else {
        return Ok((
            StatusCode::BAD_REQUEST,
            loc.f(
                "Place d'abord ton pari sur ce match.",
                "Place your bet on this match first.",
            ),
        )
            .into_response());
    };

    if current == 2 {
        // Already the joker → turn it off.
        sqlx::query("UPDATE bets SET multiplier = 1, updated_at = NOW() WHERE user_id = $1 AND match_id = $2 AND tenant_id = $3")
            .bind(user.id)
            .bind(match_id)
            .bind(tenant.id)
            .execute(&mut *tx)
            .await?;
    } else {
        // Setting a new joker: is there already one in this phase? `stage` may
        // be NULL; IS NOT DISTINCT FROM groups the unstaged matches together.
        let existing: Option<(i64, DateTime<Utc>)> = sqlx::query_as(
            r#"
            SELECT b.match_id, m.kickoff_at
            FROM bets b
            JOIN matches m ON m.id = b.match_id
            WHERE b.user_id = $1 AND b.tenant_id = $2 AND b.multiplier = 2
              AND m.stage IS NOT DISTINCT FROM $3
            "#,
        )
        .bind(user.id)
        .bind(tenant.id)
        .bind(&stage)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some((old_match_id, old_kickoff)) = existing {
            if old_kickoff <= Utc::now() {
                return Ok((
                    StatusCode::CONFLICT,
                    loc.f(
                        "Ton joker de cette phase est déjà verrouillé (le match a commencé).",
                        "Your joker for this phase is locked (its match has started).",
                    ),
                )
                    .into_response());
            }
            sqlx::query("UPDATE bets SET multiplier = 1, updated_at = NOW() WHERE user_id = $1 AND match_id = $2 AND tenant_id = $3")
                .bind(user.id)
                .bind(old_match_id)
                .bind(tenant.id)
                .execute(&mut *tx)
                .await?;
        }

        sqlx::query("UPDATE bets SET multiplier = 2, updated_at = NOW() WHERE user_id = $1 AND match_id = $2 AND tenant_id = $3")
            .bind(user.id)
            .bind(match_id)
            .bind(tenant.id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    if headers.get("hx-request").is_some() {
        return render_match_card(&state, &tenant, loc, user.id, match_id).await;
    }
    Ok(Redirect::to(&format!("/matches#match-{match_id}")).into_response())
}
