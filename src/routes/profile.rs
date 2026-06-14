//! Player profile, focused on achievements: earned badges, locked badges with
//! their condition and numeric progress towards the next tier, plus a one-off
//! toast for any badge unlocked since the last visit.

use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Response};

use crate::achievements::{self, POINT_TIERS, STREAK_TIERS};
use crate::error::AppResult;
use crate::i18n::Locale;
use crate::routes::auth::AuthUser;
use crate::stakes;
use crate::state::AppState;
use crate::tenant::{Tenant, TenantCtx};

pub struct BadgeView {
    pub name: &'static str,
    pub desc: &'static str,
    pub icon: String,
    pub earned: bool,
    /// e.g. "120 / 250" for tiered badges still in progress.
    pub progress: Option<String>,
}

#[derive(Template)]
#[template(path = "profile.html")]
struct ProfileTpl<'a> {
    loc: Locale,
    tenant: &'a Tenant,
    user_name: &'a str,
    total_points: i32,
    is_admin: bool,
    nav_active: &'static str,
    best_streak: i32,
    current_streak: i32,
    earned_count: usize,
    total_badges: usize,
    badges: Vec<BadgeView>,
    /// Names of badges unlocked since the last visit, for the toast.
    unlocked: Vec<&'static str>,
}

pub async fn me(
    State(state): State<AppState>,
    TenantCtx(tenant): TenantCtx,
    loc: Locale,
    AuthUser(user): AuthUser,
) -> AppResult<Response> {
    let board = stakes::load_leaderboard(&state.pool, tenant.id).await?;
    let total_points = stakes::points_for(&board, user.id);
    let me_row = board.iter().find(|r| r.user_id == user.id);
    let best_streak = me_row.map(|r| r.best_streak).unwrap_or(0);
    let current_streak = me_row.map(|r| r.current_streak).unwrap_or(0);

    let earned: std::collections::HashSet<String> =
        achievements::earned_codes(&state.pool, tenant.id, user.id)
            .await?
            .into_iter()
            .collect();

    let pts = total_points as i64;
    let badges: Vec<BadgeView> = achievements::CATALOG
        .iter()
        .map(|b| {
            let is_earned = earned.contains(b.code);
            // Numeric progress for the tiered badges while still locked.
            let progress = if is_earned {
                None
            } else {
                POINT_TIERS
                    .iter()
                    .find(|t| b.code == format!("pts_{t}"))
                    .map(|tier| format!("{} / {}", pts.min(*tier), tier))
                    .or_else(|| {
                        STREAK_TIERS
                            .iter()
                            .find(|t| b.code == format!("streak_{t}"))
                            .map(|tier| format!("{} / {}", best_streak.min(*tier), tier))
                    })
            };
            BadgeView {
                name: b.name(loc),
                desc: b.desc(loc),
                icon: b.icon_path(),
                earned: is_earned,
                progress,
            }
        })
        .collect();

    // Toast any badge the player hasn't seen yet, then mark them seen.
    let unlocked: Vec<&'static str> = achievements::take_unseen(&state.pool, tenant.id, user.id)
        .await?
        .iter()
        .filter_map(|code| achievements::def(code).map(|d| d.name(loc)))
        .collect();

    let tpl = ProfileTpl {
        loc,
        tenant: &tenant,
        user_name: &user.display_name,
        total_points,
        is_admin: user.is_admin,
        nav_active: "profile",
        best_streak,
        current_streak,
        earned_count: earned.len(),
        total_badges: achievements::CATALOG.len(),
        badges,
        unlocked,
    };
    Ok(Html(tpl.render()?).into_response())
}
