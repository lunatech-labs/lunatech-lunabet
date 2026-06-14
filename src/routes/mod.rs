use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

pub mod admin;
pub mod auth;
mod bets;
mod dev;
mod home;
mod invitations;
mod lang;
mod leaderboard;
mod leagues;
mod matches;
mod platform;
mod profile;
mod seo;
mod signup;
mod stake;
mod super_admin;
mod tenant_settings;
mod today;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(home::index))
        .route("/robots.txt", get(seo::robots))
        .route("/sitemap.xml", get(seo::sitemap))
        .route("/lang/:code", get(lang::set))
        .route("/login", get(auth::login_page).post(auth::request_magic_link))
        .route("/login/sent", get(auth::login_sent))
        .route("/auth/callback", get(auth::callback))
        .route("/signup", get(signup::form).post(signup::submit))
        .route("/signup/verify", get(signup::verify))
        .route("/super-admin/", get(platform::dashboard))
        .route(
            "/super-admin/login",
            get(platform::login_page).post(platform::request_link),
        )
        .route("/super-admin/auth/callback", get(platform::callback))
        .route("/super-admin/logout", post(platform::logout))
        .route(
            "/super-admin/tenants/:slug/delete",
            post(platform::delete_tenant),
        )
        .route("/super-admin/send-results", post(platform::send_results))
        .route(
            "/super-admin/send-today-matches",
            post(platform::send_today_matches),
        )
        .route("/logout", post(auth::logout))
        .route("/today", get(today::page))
        .route("/matches", get(matches::list))
        .route("/results", get(matches::results))
        .route("/matches/:id/bet", post(bets::place_or_update))
        .route("/leaderboard", get(leaderboard::index))
        .route("/me", get(profile::me))
        .route("/leagues", get(leagues::index).post(leagues::create))
        .route("/leagues/join", post(leagues::join))
        .route("/leagues/:id", get(leagues::show))
        .route("/leagues/:id/leave", post(leagues::leave))
        .route("/leagues/:id/remove", post(leagues::remove_member))
        .route("/leagues/:id/rename", post(leagues::rename))
        .route("/leagues/:id/delete", post(leagues::delete))
        .route("/members", get(invitations::members_page))
        .route("/invitations", post(invitations::create))
        .route("/invitations/:id/revoke", post(invitations::revoke))
        .route("/invite/accept", get(invitations::accept))
        .route("/stake", get(stake::page).post(stake::submit))
        .route("/admin/stakes", get(admin::stakes_page))
        .route("/admin/stakes/:user_id/paid", post(admin::mark_paid))
        .route("/admin/stakes/:user_id/unpaid", post(admin::mark_unpaid))
        .route(
            "/admin/settings",
            get(tenant_settings::page)
                .post(tenant_settings::update)
                // Logo uploads are capped at 2 MiB in the handler; raise the
                // request limit above axum's 2 MiB default so the multipart
                // body (logo + form fields) isn't rejected before we can
                // return a friendly error.
                .layer(DefaultBodyLimit::max(4 * 1024 * 1024)),
        )
        .route(
            "/admin/tenants",
            get(super_admin::list).post(super_admin::create),
        )
        .route("/admin/tenants/new", get(super_admin::new_form))
        .route("/admin/tenants/:slug/edit", get(super_admin::edit_form))
        .route("/admin/tenants/:slug", post(super_admin::update))
        .route("/admin/tenants/:slug/delete", post(super_admin::delete))
        .route("/dev", get(dev::index))
        .route("/dev/login", get(dev::login_as))
}
