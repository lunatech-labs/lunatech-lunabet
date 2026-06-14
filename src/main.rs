use std::net::SocketAddr;

use anyhow::Context;
use axum::Router;
use axum_extra::extract::cookie::Key;
use chrono::Timelike;
use chrono_tz::Europe::Amsterdam;
use sqlx::postgres::PgPoolOptions;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

mod achievements;
mod characters;
mod config;
mod error;
mod fixtures;
mod football_data;
mod highlights;
mod i18n;
mod mail;
mod middleware;
mod models;
mod notifications;
mod rate_limit;
mod routes;
mod scoring;
mod stakes;
mod state;
mod streaks;
mod tenant;

use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lunatech_betting=info,tower_http=info".into()),
        )
        .init();

    let cfg = config::Config::from_env().context("loading configuration from env")?;

    // No after_connect hook. RLS policies are still installed (0525_07)
    // but FORCE is off (0529_02 / _04), so the table-owner role our app
    // connects with bypasses them naturally — same effective behaviour
    // as the explicit `app.bypass_rls = on` we used before, one less
    // moving piece to worry about in production.
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&cfg.database_url)
        .await
        .context("connecting to Postgres")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("running migrations")?;

    let default_tenant = tenant::ensure_default(&pool, &cfg)
        .await
        .context("ensuring default tenant exists")?;
    tracing::info!(slug = %default_tenant.slug, id = %default_tenant.id, "default tenant loaded");
    let tenants = tenant::TenantRegistry::new(pool.clone(), default_tenant.clone());

    if std::env::args().nth(1).as_deref() == Some("seed") {
        fixtures::seed(&pool, &default_tenant).await.context("seeding fixtures")?;
        println!("\nDone. Lance ensuite `cargo run` puis ouvre http://localhost:3000/dev");
        return Ok(());
    }

    let signup_limiter =
        rate_limit::SignupRateLimiter::new(std::time::Duration::from_secs(3600), 5);
    let endpoint_limiter =
        rate_limit::EndpointRateLimiter::new(std::time::Duration::from_secs(60), 10);

    if std::env::args().nth(1).as_deref() == Some("notify") {
        // One-off: run the match-reminder job once and exit (for testing).
        let cookie_key = Key::from(&vec![0u8; 64]);
        let state = AppState {
            pool: pool.clone(),
            cookie_key,
            cfg: cfg.clone(),
            http: reqwest::Client::builder().user_agent("lunatech-betting/0.1").build()?,
            tenants: tenants.clone(),
            signup_limiter: signup_limiter.clone(),
            endpoint_limiter: endpoint_limiter.clone(),
        };
        notifications::send_match_reminders(&state)
            .await
            .context("running match reminders")?;
        println!("Match reminder job completed.");
        return Ok(());
    }

    if std::env::args().nth(1).as_deref() == Some("daily-digest") {
        // One-off: send the daily recap once and exit. Optional date arg
        // (YYYY-MM-DD, UTC); defaults to yesterday.
        let cookie_key = Key::from(&vec![0u8; 64]);
        let state = AppState {
            pool: pool.clone(),
            cookie_key,
            cfg: cfg.clone(),
            http: reqwest::Client::builder().user_agent("lunatech-betting/0.1").build()?,
            tenants: tenants.clone(),
            signup_limiter: signup_limiter.clone(),
            endpoint_limiter: endpoint_limiter.clone(),
        };
        let date = std::env::args()
            .nth(2)
            .and_then(|s| chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
            .unwrap_or_else(|| (chrono::Utc::now() - chrono::Duration::days(1)).date_naive());
        notifications::send_daily_digest(&state, date, false)
            .await
            .context("running daily digest")?;
        println!("Daily digest job completed for {date}.");
        return Ok(());
    }

    if std::env::args().nth(1).as_deref() == Some("today-matches") {
        // One-off: send the "today's matches" preview once and exit. Optional
        // date arg (YYYY-MM-DD, UTC); defaults to today.
        let cookie_key = Key::from(&vec![0u8; 64]);
        let state = AppState {
            pool: pool.clone(),
            cookie_key,
            cfg: cfg.clone(),
            http: reqwest::Client::builder().user_agent("lunatech-betting/0.1").build()?,
            tenants: tenants.clone(),
            signup_limiter: signup_limiter.clone(),
            endpoint_limiter: endpoint_limiter.clone(),
        };
        let date = std::env::args()
            .nth(2)
            .and_then(|s| chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
            .unwrap_or_else(|| chrono::Utc::now().date_naive());
        notifications::send_today_matches(&state, date, false)
            .await
            .context("running today's matches email")?;
        println!("Today's matches email job completed for {date}.");
        return Ok(());
    }

    if std::env::args().nth(1).as_deref() == Some("recompute-streaks") {
        // One-off: recompute every user's streaks from the settled-bet history
        // and exit. Useful to backfill after the migration; the running server
        // does this automatically on each scoring tick.
        scoring::recompute_all(&pool).await.context("recomputing points")?;
        streaks::recompute_all(&pool).await.context("recomputing streaks")?;
        achievements::evaluate_all(&pool).await.context("evaluating achievements")?;
        println!("Streaks recomputed and achievements evaluated.");
        return Ok(());
    }

    let cookie_key = Key::from(&cfg.cookie_key_bytes);

    let state = AppState {
        pool,
        cookie_key,
        cfg: cfg.clone(),
        http: reqwest::Client::builder()
            .user_agent("lunatech-betting/0.1")
            .build()?,
        tenants,
        signup_limiter,
        endpoint_limiter,
    };

    if cfg.football_data_api_key.is_some() {
        let s = state.clone();
        tokio::spawn(async move {
            if let Err(e) = football_data::sync_fixtures(&s).await {
                tracing::warn!("initial fixtures sync failed: {e:#}");
            }
        });
    }

    {
        let s = state.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(300));
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if let Err(e) = football_data::sync_fixtures(&s).await {
                    tracing::warn!("fixtures sync failed: {e:#}");
                }
                if let Err(e) = scoring::recompute_all(&s.pool).await {
                    tracing::warn!("scoring recompute failed: {e:#}");
                }
                if let Err(e) = streaks::recompute_all(&s.pool).await {
                    tracing::warn!("streak recompute failed: {e:#}");
                }
                if let Err(e) = achievements::evaluate_all(&s.pool).await {
                    tracing::warn!("achievement evaluation failed: {e:#}");
                }
                if let Err(e) = notifications::send_match_reminders(&s).await {
                    tracing::warn!("match reminders failed: {e:#}");
                }
            }
        });
    }

    // Hourly cleanup: drop pending signups that expired more than a day ago
    // and shrink the in-memory rate limiter bucket maps.
    {
        let s = state.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(3600));
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let res = sqlx::query(
                    "DELETE FROM pending_tenants \
                     WHERE consumed_at IS NULL AND expires_at < NOW() - INTERVAL '1 day'",
                )
                .execute(&s.pool)
                .await;
                match res {
                    Ok(r) if r.rows_affected() > 0 => tracing::info!(
                        "purged {} expired pending tenant signups",
                        r.rows_affected()
                    ),
                    Ok(_) => {}
                    Err(e) => tracing::warn!("pending_tenants cleanup failed: {e:#}"),
                }
                // Expire pending invitations past their deadline so the login
                // gate and the members list stop honouring them.
                if let Err(e) = sqlx::query(
                    "UPDATE invitations SET status = 'expired' \
                     WHERE status = 'pending' AND expires_at < NOW()",
                )
                .execute(&s.pool)
                .await
                {
                    tracing::warn!("invitation expiry cleanup failed: {e:#}");
                }
                s.signup_limiter.purge_empty();
                s.endpoint_limiter.purge_empty();
            }
        });
    }

    // Scheduled digest emails. The check ticks every 15 min; both senders are
    // idempotent per (tenant, day) via their tables, so repeated ticks after the
    // hour are harmless no-ops. Hours are interpreted in Amsterdam local time
    // (CET/CEST), so they hold steady across daylight-saving changes.
    // - Daily recap: from DAILY_DIGEST_HOUR each morning, the previous day's
    //   results digest goes out once.
    // - Today's matches preview: from TODAY_MATCHES_HOUR, the list of the day's
    //   upcoming matches goes out once.
    {
        let s = state.clone();
        let digest_hour = cfg.daily_digest_hour;
        let today_matches_hour = cfg.today_matches_hour;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(900));
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let now = chrono::Utc::now();
                let local_hour = now.with_timezone(&Amsterdam).hour();
                if local_hour >= digest_hour {
                    if let Some(yesterday) = now.date_naive().pred_opt() {
                        if let Err(e) = notifications::send_daily_digest(&s, yesterday, false).await
                        {
                            tracing::warn!("daily digest failed: {e:#}");
                        }
                    }
                }
                if local_hour >= today_matches_hour {
                    if let Err(e) =
                        notifications::send_today_matches(&s, now.date_naive(), false).await
                    {
                        tracing::warn!("today's matches email failed: {e:#}");
                    }
                }
            }
        });
    }

    // User-uploaded assets (currently tenant logos) live outside the bundled
    // `static/` tree so they survive redeploys and are never clobbered by the
    // shipped files. Created on boot so ServeDir always has a directory.
    std::fs::create_dir_all(&cfg.uploads_dir)
        .with_context(|| format!("creating uploads dir {}", cfg.uploads_dir))?;

    let app = Router::new()
        .merge(routes::router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::resolve_tenant,
        ))
        .nest_service("/static", ServeDir::new("static"))
        .nest_service("/uploads", ServeDir::new(&cfg.uploads_dir))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = cfg.bind_addr.parse().context("parsing BIND_ADDR")?;
    tracing::info!("listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
