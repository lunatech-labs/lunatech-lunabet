//! "Today" screen: a focused matchday view showing the day's matches (with the
//! user's bets), the previous day's results, and the top-10 leaderboard.
//!
//! Matchday windows follow CEST (UTC+2, the WC2026 timezone), 15:00 CEST to
//! 08:00 CEST the next morning (13:00 UTC to 06:00 UTC):
//! - "Today's matches": today 15:00 CEST -> tomorrow 08:00 CEST.
//! - "Yesterday's results": yesterday 15:00 CEST -> today 08:00 CEST.
//!
//! The window is defined once in [`crate::matchday`] so the daily emails agree
//! with this screen on which matches belong to a given day.

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use chrono::{NaiveDate, Utc};
use serde::Deserialize;

use serde_json::json;

use crate::characters;
use crate::error::AppResult;
use crate::highlights;
use crate::i18n::Locale;
use crate::matchday;
use crate::models::Match;
use crate::routes::auth::AuthUser;
use crate::routes::matches::MatchView;
use crate::scoring;
use crate::stakes;
use crate::state::AppState;
use crate::tenant::{Tenant, TenantCtx};

/// At most this many wins are celebrated one by one; beyond it we show a single
/// aggregate ("5 winning picks, +11 pts") instead of a flurry of animations.
const CELEBRATE_CAP: usize = 3;

pub struct Standing {
    pub rank: usize,
    pub name: String,
    pub points: i64,
    pub is_me: bool,
    pub current_streak: i32,
}

pub struct PlayerOfDay {
    pub name: String,
    pub points: i64,
    pub avatar: String,
    pub is_me: bool,
}

#[derive(Template)]
#[template(path = "today.html")]
struct TodayTpl<'a> {
    loc: Locale,
    tenant: &'a Tenant,
    user_name: &'a str,
    total_points: i32,
    current_streak: i32,
    best_streak: i32,
    is_admin: bool,
    nav_active: &'static str,
    today_label: String,
    yest_label: String,
    today: Vec<MatchView>,
    results: Vec<MatchView>,
    standings: Vec<Standing>,
    /// JSON payload (or `None`) consumed by `celebrate.js` to fire a one-shot
    /// animation for the user's freshly-won, not-yet-seen predictions.
    celebrate_json: Option<String>,
    player_of_day: Option<PlayerOfDay>,
}

pub async fn page(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    loc: Locale,
    AuthUser(user): AuthUser,
) -> AppResult<Response> {
    let today_cest = matchday::cest_date(Utc::now());
    let yesterday = today_cest.pred_opt().unwrap_or(today_cest);

    // "Today's" matchday: today 15:00 CEST -> tomorrow 08:00 CEST.
    let (today_start, today_end) = matchday::window(today_cest);
    // "Yesterday's" matchday: yesterday 15:00 CEST -> today 08:00 CEST.
    let (yest_start, yest_end) = matchday::window(yesterday);

    // One query covers both windows (yesterday's start to today's end).
    let matches: Vec<Match> = sqlx::query_as(
        r#"
        SELECT id, competition, stage, group_name,
               home_team, away_team, home_team_code, away_team_code,
               kickoff_at, status, home_score, away_score
        FROM matches
        WHERE kickoff_at >= $1 AND kickoff_at < $2
        ORDER BY kickoff_at ASC
        "#,
    )
    .bind(yest_start)
    .bind(today_end)
    .fetch_all(&state.pool)
    .await?;

    let ids: Vec<i64> = matches.iter().map(|m| m.id).collect();
    let bets: Vec<(i64, i32, i32, Option<i32>, i32)> = sqlx::query_as(
        "SELECT match_id, home_score, away_score, points, multiplier FROM bets \
         WHERE user_id = $1 AND tenant_id = $2 AND match_id = ANY($3)",
    )
    .bind(user.id)
    .bind(tenant.id)
    .bind(&ids)
    .fetch_all(&state.pool)
    .await?;
    let bet_map: HashMap<i64, (i32, i32, Option<i32>, i32)> = bets
        .into_iter()
        .map(|(m, h, a, p, mult)| (m, (h, a, p, mult)))
        .collect();

    let mut today = Vec::new();
    let mut results = Vec::new();
    for m in matches {
        let bet = bet_map.get(&m.id).copied();
        let view = MatchView {
            open: m.is_open_for_bets(),
            finished: m.has_final_result(),
            bet_home: bet.map(|(h, _, _, _)| h),
            bet_away: bet.map(|(_, a, _, _)| a),
            points: bet.and_then(|(_, _, p, _)| p),
            is_joker: bet.map(|(_, _, _, mult)| mult == 2).unwrap_or(false),
            jokers_enabled: tenant.jokers_enabled,
            m,
        };
        let ko = view.m.kickoff_at;
        if ko >= today_start && ko < today_end {
            today.push(view);
        } else if ko >= yest_start && ko < yest_end && !view.open {
            // Everything in the previous matchday that has already kicked off:
            // finished games and ones still in progress (e.g. an early-morning
            // match that is live, or just ended, when the page is opened).
            results.push(view);
        }
    }

    let board = stakes::load_leaderboard(&state.pool, tenant.id).await?;
    let standings: Vec<Standing> = board
        .iter()
        .enumerate()
        .take(10)
        .map(|(i, r)| Standing {
            rank: i + 1,
            name: r.display_name.clone(),
            points: r.points,
            is_me: r.user_id == user.id,
            current_streak: r.current_streak,
        })
        .collect();
    let me = board.iter().find(|r| r.user_id == user.id);
    let total_points = me.map(|r| r.points as i32).unwrap_or(0);
    let current_streak = me.map(|r| r.current_streak).unwrap_or(0);
    let best_streak = me.map(|r| r.best_streak).unwrap_or(0);

    // Player of the day reflects the previous completed calendar day (UTC),
    // matching the daily digest that records it. Read-only here: the digest job
    // is what computes and stores it.
    let potd_date = Utc::now().date_naive().pred_opt();
    let player_of_day = match potd_date {
        Some(d) => highlights::player_of_the_day(&state.pool, tenant.id, d)
            .await?
            .map(|p| PlayerOfDay {
                name: p.display_name,
                points: p.points,
                avatar: characters::path_for(p.user_id),
                is_me: p.user_id == user.id,
            }),
        None => None,
    };

    let celebrate_json = build_celebration(&state, &tenant, &user, loc).await?;

    let fmt = |d: NaiveDate| d.format("%d/%m").to_string();
    let tpl = TodayTpl {
        loc,
        tenant: &tenant,
        user_name: &user.display_name,
        total_points,
        current_streak,
        best_streak,
        is_admin: user.is_admin,
        nav_active: "today",
        today_label: fmt(today_cest),
        yest_label: fmt(yesterday),
        today,
        results,
        standings,
        celebrate_json,
        player_of_day,
    };
    Ok(Html(tpl.render()?).into_response())
}

#[derive(Template)]
#[template(path = "_match_card.html")]
struct MatchCardTpl {
    loc: Locale,
    v: MatchView,
    /// Drop the kickoff time from the label (the "Yesterday's results" column).
    date_only: bool,
}

#[derive(Deserialize)]
pub struct FragmentQuery {
    #[serde(default)]
    date_only: bool,
}

/// Re-render a single match card. Live cards on the Today screen poll this every
/// 30s and swap themselves in place, so the displayed score keeps up with the
/// background score sync instead of going stale until a manual reload. Once a
/// match is finished the rendered card no longer carries a poll trigger, so the
/// refresh loop stops on its own.
pub async fn match_fragment(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    loc: Locale,
    AuthUser(user): AuthUser,
    Path(id): Path<i64>,
    Query(q): Query<FragmentQuery>,
) -> AppResult<Response> {
    let m: Option<Match> = sqlx::query_as(
        r#"
        SELECT id, competition, stage, group_name,
               home_team, away_team, home_team_code, away_team_code,
               kickoff_at, status, home_score, away_score
        FROM matches
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(m) = m else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };

    let bet: Option<(i32, i32, Option<i32>, i32)> = sqlx::query_as(
        "SELECT home_score, away_score, points, multiplier FROM bets \
         WHERE user_id = $1 AND tenant_id = $2 AND match_id = $3",
    )
    .bind(user.id)
    .bind(tenant.id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;

    let view = MatchView {
        open: m.is_open_for_bets(),
        finished: m.is_concluded(),
        bet_home: bet.map(|(h, _, _, _)| h),
        bet_away: bet.map(|(_, a, _, _)| a),
        points: bet.and_then(|(_, _, p, _)| p),
        is_joker: bet.map(|(_, _, _, mult)| mult == 2).unwrap_or(false),
        jokers_enabled: tenant.jokers_enabled,
        m,
    };

    let tpl = MatchCardTpl {
        loc,
        v: view,
        date_only: q.date_only,
    };
    Ok(Html(tpl.render()?).into_response())
}

/// Build the celebration payload for `user`: their winning bets that have been
/// settled but not yet shown. Reads the unseen wins first, then marks every
/// settled-and-unseen bet (wins and losses) as seen so each result is
/// celebrated at most once. Returns `None` when there is nothing to celebrate.
async fn build_celebration(
    state: &AppState,
    tenant: &Tenant,
    user: &crate::models::User,
    loc: Locale,
) -> AppResult<Option<String>> {
    let unseen_wins: Vec<(String, String, i32)> = sqlx::query_as(
        r#"
        SELECT m.home_team, m.away_team, b.points
        FROM bets b
        JOIN matches m ON m.id = b.match_id
        WHERE b.user_id = $1 AND b.tenant_id = $2
          AND b.points IS NOT NULL AND b.points > 0
          AND b.result_seen_at IS NULL
        ORDER BY m.kickoff_at ASC
        "#,
    )
    .bind(user.id)
    .bind(tenant.id)
    .fetch_all(&state.pool)
    .await?;

    // Mark every settled-and-unseen bet as seen, wins and losses alike, so we
    // never replay them. Idempotent: a second load updates nothing.
    sqlx::query(
        "UPDATE bets SET result_seen_at = NOW() \
         WHERE user_id = $1 AND tenant_id = $2 \
           AND points IS NOT NULL AND result_seen_at IS NULL",
    )
    .bind(user.id)
    .bind(tenant.id)
    .execute(&state.pool)
    .await?;

    if unseen_wins.is_empty() {
        return Ok(None);
    }

    let payload = if unseen_wins.len() <= CELEBRATE_CAP {
        let wins: Vec<_> = unseen_wins
            .iter()
            .map(|(home, away, points)| {
                let exact = *points >= scoring::POINTS_EXACT;
                json!({
                    "label": format!("{home} - {away}"),
                    "points": points,
                    "level": if exact { "exact" } else { "outcome" },
                    "message": if exact {
                        loc.f("Score exact ! +3", "Exact score! +3")
                    } else {
                        loc.f("Bien vu ! +1", "Nice! +1")
                    },
                })
            })
            .collect();
        json!({ "mode": "individual", "wins": wins })
    } else {
        let total: i32 = unseen_wins.iter().map(|(_, _, p)| p).sum();
        let count = unseen_wins.len();
        let message = match loc {
            Locale::Fr => format!("{count} pronos gagnés, +{total} pts"),
            Locale::En => format!("{count} winning picks, +{total} pts"),
        };
        json!({ "mode": "aggregate", "count": count, "points": total, "message": message })
    };

    // Escape `<` so the JSON can sit safely inside a <script> tag.
    Ok(Some(payload.to_string().replace('<', "\\u003c")))
}
