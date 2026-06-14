use std::collections::HashMap;

use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Response};

use crate::achievements;
use crate::characters;
use crate::error::AppResult;
use crate::i18n::Locale;
use crate::routes::auth::AuthUser;
use crate::stakes;
use crate::state::AppState;
use crate::tenant::{Tenant, TenantCtx};

/// A single badge icon shown next to a player's name.
pub struct BadgeChip {
    pub icon: String,
    pub name: &'static str,
}

pub struct Row {
    pub rank: usize,
    pub display_name: String,
    pub points: i64,
    pub exact: i64,
    pub bets: i64,
    pub stake_eur: Option<i32>,
    pub paid: bool,
    pub eligible: bool,
    pub payout_eur: Option<f64>,
    pub is_me: bool,
    pub current_streak: i32,
    pub best_streak: i32,
    /// Public path to the Tsubasa-inspired avatar assigned to this user.
    pub avatar: String,
    /// Up to 3 badge icons, plus an overflow count for the rest.
    pub badges: Vec<BadgeChip>,
    pub extra_badges: usize,
}

#[derive(Template)]
#[template(path = "leaderboard.html")]
struct LeaderboardTpl<'a> {
    loc: Locale,
    tenant: &'a Tenant,
    user_name: &'a str,
    total_points: i32,
    nav_active: &'static str,
    rows: Vec<Row>,
    pot_total_eur: i64,
    paid_count: i64,
    is_admin: bool,
    paid_user_stake_eur: Option<i32>,
    paid_user_paid: bool,
}

pub async fn index(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    loc: Locale,
    AuthUser(user): AuthUser,
) -> AppResult<Response> {
    let board = stakes::load_leaderboard(&state.pool, tenant.id).await?;
    let pot = stakes::load_pot(&state.pool, tenant.id, tenant.stake_deadline).await?;
    let top_paid = stakes::top_paid_from_leaderboard(&board);
    let payouts = stakes::compute_payouts(pot.total_eur, &top_paid);

    let payout_map: HashMap<_, _> = payouts.iter().map(|p| (p.user_id, p.payout_eur)).collect();
    let badges_map = achievements::earned_by_user(&state.pool, tenant.id).await?;

    let rows = board
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let codes = badges_map.get(&r.user_id).cloned().unwrap_or_default();
            let extra_badges = codes.len().saturating_sub(3);
            let badges = codes
                .iter()
                .take(3)
                .filter_map(|c| {
                    achievements::def(c).map(|d| BadgeChip {
                        icon: d.icon_path(),
                        name: d.name(loc),
                    })
                })
                .collect();
            Row {
            rank: i + 1,
            display_name: r.display_name.clone(),
            points: r.points,
            exact: r.exact_count,
            bets: r.settled_bets,
            stake_eur: r.stake_eur,
            paid: r.paid,
            eligible: r.paid && r.stake_eur.is_some(),
            payout_eur: payout_map.get(&r.user_id).copied(),
            is_me: r.user_id == user.id,
            current_streak: r.current_streak,
            best_streak: r.best_streak,
            avatar: characters::path_for(r.user_id),
            badges,
            extra_badges,
            }
        })
        .collect();

    let tpl = LeaderboardTpl {
        loc,
        tenant: &tenant,
        user_name: &user.display_name,
        total_points: stakes::points_for(&board, user.id),
        nav_active: "leaderboard",
        rows,
        pot_total_eur: pot.total_eur,
        paid_count: pot.paid_count,
        is_admin: user.is_admin,
        paid_user_stake_eur: user.stake_eur,
        paid_user_paid: user.paid_at.is_some(),
    };
    Ok(Html(tpl.render()?).into_response())
}
