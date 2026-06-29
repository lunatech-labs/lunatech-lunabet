use std::collections::HashSet;

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::state::AppState;
use crate::tenant;

#[derive(Debug, Deserialize)]
struct MatchesResp {
    matches: Vec<ApiMatch>,
}

#[derive(Debug, Deserialize)]
struct ApiMatch {
    id: i64,
    #[serde(rename = "utcDate")]
    utc_date: DateTime<Utc>,
    status: String,
    stage: Option<String>,
    group: Option<String>,
    competition: ApiCompetition,
    #[serde(rename = "homeTeam")]
    home_team: ApiTeam,
    #[serde(rename = "awayTeam")]
    away_team: ApiTeam,
    score: Option<ApiScore>,
}

#[derive(Debug, Deserialize)]
struct ApiCompetition {
    code: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiTeam {
    name: Option<String>,
    tla: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiScore {
    #[serde(rename = "fullTime")]
    full_time: ApiScorePart,
}

#[derive(Debug, Deserialize)]
struct ApiScorePart {
    home: Option<i32>,
    away: Option<i32>,
}

pub async fn sync_fixtures(state: &AppState) -> anyhow::Result<()> {
    let Some(api_key) = state.cfg.football_data_api_key.clone() else {
        return Ok(());
    };

    // Collect the unique set of competitions any tenant cares about so we
    // make one API call per code regardless of how many tenants share it.
    let tenants = tenant::load_all(&state.pool).await?;
    let mut competitions: HashSet<String> = HashSet::new();
    for t in &tenants {
        competitions.insert(t.football_competition.clone());
    }
    if competitions.is_empty() {
        return Ok(());
    }

    for competition in competitions {
        if let Err(e) = sync_one_competition(state, &api_key, &competition).await {
            tracing::warn!(competition = %competition, "fixtures sync failed: {e:#}");
        }
    }
    Ok(())
}

async fn sync_one_competition(
    state: &AppState,
    api_key: &str,
    competition: &str,
) -> anyhow::Result<()> {
    let url = format!("https://api.football-data.org/v4/competitions/{competition}/matches");

    let resp = state
        .http
        .get(&url)
        .header("X-Auth-Token", api_key)
        .send()
        .await
        .context("calling football-data.org")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("football-data.org returned {status}: {body}");
    }

    let data: MatchesResp = resp.json().await.context("decoding football-data.org response")?;

    let mut tx = state.pool.begin().await?;
    let mut count = 0usize;
    for m in &data.matches {
        let competition_name = m
            .competition
            .name
            .clone()
            .or_else(|| m.competition.code.clone())
            .unwrap_or_else(|| competition.to_string());
        let (home_score, away_score) = m
            .score
            .as_ref()
            .map(|s| (s.full_time.home, s.full_time.away))
            .unwrap_or((None, None));
        let home_team = m.home_team.name.clone().unwrap_or_else(|| "?".into());
        let away_team = m.away_team.name.clone().unwrap_or_else(|| "?".into());

        sqlx::query(
            r#"
            INSERT INTO matches (
                id, competition, stage, group_name,
                home_team, away_team, home_team_code, away_team_code,
                kickoff_at, status, home_score, away_score, updated_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12, NOW())
            ON CONFLICT (id) DO UPDATE SET
                competition    = EXCLUDED.competition,
                stage          = EXCLUDED.stage,
                group_name     = EXCLUDED.group_name,
                home_team      = EXCLUDED.home_team,
                away_team      = EXCLUDED.away_team,
                home_team_code = EXCLUDED.home_team_code,
                away_team_code = EXCLUDED.away_team_code,
                kickoff_at     = EXCLUDED.kickoff_at,
                -- A locked match keeps its manually-set score and status: the
                -- upstream feed is wrong for it (e.g. a disallowed goal the feed
                -- never corrected), so we ignore the feed's outcome while still
                -- refreshing every other field above.
                status         = CASE WHEN matches.score_locked THEN matches.status     ELSE EXCLUDED.status     END,
                home_score     = CASE WHEN matches.score_locked THEN matches.home_score ELSE EXCLUDED.home_score END,
                away_score     = CASE WHEN matches.score_locked THEN matches.away_score ELSE EXCLUDED.away_score END,
                updated_at     = NOW()
            "#,
        )
        .bind(m.id)
        .bind(&competition_name)
        .bind(&m.stage)
        .bind(&m.group)
        .bind(&home_team)
        .bind(&away_team)
        .bind(&m.home_team.tla)
        .bind(&m.away_team.tla)
        .bind(m.utc_date)
        .bind(&m.status)
        .bind(home_score)
        .bind(away_score)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }
    tx.commit().await?;

    tracing::info!("synced {count} matches from football-data.org ({competition})");
    Ok(())
}
