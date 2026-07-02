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
    duration: Option<String>,
    #[serde(rename = "fullTime")]
    full_time: ApiScorePart,
    #[serde(rename = "regularTime")]
    regular_time: Option<ApiScorePart>,
    #[serde(rename = "extraTime")]
    extra_time: Option<ApiScorePart>,
    // Penalty shootout score, present only when the match was decided on
    // penalties. Stored for display next to the final score; it does not affect
    // scoring, which settles on the after-120-minute score (see settled_score).
    penalties: Option<ApiScorePart>,
}

#[derive(Debug, Deserialize)]
struct ApiScorePart {
    home: Option<i32>,
    away: Option<i32>,
}

impl ApiScore {
    /// The score that bets are settled against: the result after 120 minutes,
    /// with any penalty shootout ignored. For a PENALTY_SHOOTOUT match the
    /// feed folds the shootout into `fullTime` (e.g. a 1-1 draw won 5-3 on
    /// pens is reported as 6-4), so we reconstruct the after-extra-time score
    /// from regularTime + extraTime instead. Every other duration already
    /// reports the 120' (or 90') result in fullTime.
    fn settled_score(&self) -> (Option<i32>, Option<i32>) {
        if self.duration.as_deref() == Some("PENALTY_SHOOTOUT") {
            let reg = self.regular_time.as_ref();
            let ext = self.extra_time.as_ref();
            let home = sum_parts(reg.and_then(|p| p.home), ext.and_then(|p| p.home));
            let away = sum_parts(reg.and_then(|p| p.away), ext.and_then(|p| p.away));
            (home, away)
        } else {
            (self.full_time.home, self.full_time.away)
        }
    }
}

/// Add two optional score parts, treating a missing side as 0 when the other
/// is present; None only when both are absent.
fn sum_parts(a: Option<i32>, b: Option<i32>) -> Option<i32> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
    }
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
            .map(|s| s.settled_score())
            .unwrap_or((None, None));
        let (pens_home, pens_away) = m
            .score
            .as_ref()
            .and_then(|s| s.penalties.as_ref())
            .map(|p| (p.home, p.away))
            .unwrap_or((None, None));
        let home_team = m.home_team.name.clone().unwrap_or_else(|| "?".into());
        let away_team = m.away_team.name.clone().unwrap_or_else(|| "?".into());

        sqlx::query(
            r#"
            INSERT INTO matches (
                id, competition, stage, group_name,
                home_team, away_team, home_team_code, away_team_code,
                kickoff_at, status, home_score, away_score, pens_home, pens_away, updated_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14, NOW())
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
                pens_home      = CASE WHEN matches.score_locked THEN matches.pens_home  ELSE EXCLUDED.pens_home  END,
                pens_away      = CASE WHEN matches.score_locked THEN matches.pens_away  ELSE EXCLUDED.pens_away  END,
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
        .bind(pens_home)
        .bind(pens_away)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }
    tx.commit().await?;

    tracing::info!("synced {count} matches from football-data.org ({competition})");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Two helpers, two seams:
    //   settled(json)       -> the settled 120-minute SCORE (ingestion seam:
    //                          serde decode -> ApiScore::settled_score).
    //   score_bet(json, ..) -> the POINTS a user receives (feeds the settled
    //                          score into scoring::compute_points -- the
    //                          user-facing seam).
    //
    // Each shootout payload therefore has two matching tests:
    //   *_settles_* asserts the SCORE, pinning the decode seam on its own, so a
    //              failure localises to decoding rather than scoring.
    //   *_scores_*  asserts the POINTS, pinning the composed user-facing outcome.
    //              These are the red-capable guard: a 0-0 bet must score 3 and a
    //              3-0 bet must score 0, which invert if settled_score ever
    //              regresses to returning fullTime.

    fn settled(score_json: &str) -> (Option<i32>, Option<i32>) {
        serde_json::from_str::<ApiScore>(score_json).unwrap().settled_score()
    }

    // Runs the real end-to-end chain a user's points flow through: serde decode
    // into ApiScore, the private settled_score() reduction to the 120-minute
    // score, then the public compute_points() with the user's bet. Nothing is
    // stubbed.
    fn score_bet(json: &str, bet_home: i32, bet_away: i32) -> i32 {
        let (actual_home, actual_away) = settled(json);
        crate::scoring::compute_points(
            bet_home,
            bet_away,
            actual_home.unwrap(),
            actual_away.unwrap(),
        )
    }

    // Real football-data.org v4 payloads (Euro 2024, free tier). For a
    // PENALTY_SHOOTOUT match the shootout is folded into fullTime, so bets must
    // be settled on regularTime + extraTime instead.

    // England 1-1 Switzerland a.e.t., won 5-3 on pens. fullTime reads 6-4.
    #[test]
    fn shootout_after_a_draw_settles_at_the_120_minute_draw() {
        let json = r#"{
            "winner": "HOME_TEAM", "duration": "PENALTY_SHOOTOUT",
            "fullTime": { "home": 6, "away": 4 },
            "regularTime": { "home": 1, "away": 1 },
            "extraTime": { "home": 0, "away": 0 },
            "penalties": { "home": 5, "away": 3 }
        }"#;
        assert_eq!(settled(json), (Some(1), Some(1)));
    }

    // End-to-end: the England 1-1 Switzerland shootout payload (settles 1-1)
    // scored against a user's bet. A user betting the 120-minute draw is paid
    // for the exact score, not left unpaid because fullTime folded in the 6-4
    // penalty count.
    #[test]
    fn shootout_after_a_draw_scores_the_120_minute_draw() {
        let json = r#"{
            "winner": "HOME_TEAM", "duration": "PENALTY_SHOOTOUT",
            "fullTime": { "home": 6, "away": 4 },
            "regularTime": { "home": 1, "away": 1 },
            "extraTime": { "home": 0, "away": 0 },
            "penalties": { "home": 5, "away": 3 }
        }"#;
        assert_eq!(score_bet(json, 1, 1), 3);
        assert_eq!(score_bet(json, 2, 2), 1);
        assert_eq!(score_bet(json, 2, 1), 0);
    }

    // Portugal 0-0 Slovenia a.e.t., won 3-0 on pens. fullTime reads 3-0, which
    // would wrongly grade a correct draw guess as a home win.
    #[test]
    fn goalless_shootout_settles_at_nil_nil_not_the_penalty_count() {
        let json = r#"{
            "winner": "HOME_TEAM", "duration": "PENALTY_SHOOTOUT",
            "fullTime": { "home": 3, "away": 0 },
            "regularTime": { "home": 0, "away": 0 },
            "extraTime": { "home": 0, "away": 0 },
            "penalties": { "home": 3, "away": 0 }
        }"#;
        assert_eq!(settled(json), (Some(0), Some(0)));
    }

    // End-to-end: the Portugal 0-0 Slovenia goalless shootout payload (settles
    // 0-0) scored against a user's bet. The 0-0-scores-3 and 3-0-scores-0 pair
    // is the red-capable guard: if settled_score regressed to returning fullTime
    // (3-0), those two assertions would invert (0-0 bet drops to 0, 3-0 bet
    // jumps to 3) and the test would fail. The 1-0 bet proves the folded 3-0
    // penalty count was not used as the score.
    #[test]
    fn goalless_shootout_scores_the_nil_nil_not_the_penalty_count() {
        let json = r#"{
            "winner": "HOME_TEAM", "duration": "PENALTY_SHOOTOUT",
            "fullTime": { "home": 3, "away": 0 },
            "regularTime": { "home": 0, "away": 0 },
            "extraTime": { "home": 0, "away": 0 },
            "penalties": { "home": 3, "away": 0 }
        }"#;
        assert_eq!(score_bet(json, 0, 0), 3);
        assert_eq!(score_bet(json, 1, 0), 0);
        assert_eq!(score_bet(json, 3, 0), 0);
    }

    // End-to-end: an EXTRA_TIME match (no shootout) settles on fullTime 2-1, so
    // a user betting the exact 2-1 is paid the full 3 points. Proves the
    // shootout special-casing did not disturb the ordinary extra-time path.
    #[test]
    fn extra_time_winner_scores_the_full_time_score() {
        let json = r#"{
            "winner": "HOME_TEAM", "duration": "EXTRA_TIME",
            "fullTime": { "home": 2, "away": 1 },
            "regularTime": { "home": 1, "away": 1 },
            "extraTime": { "home": 1, "away": 0 }
        }"#;
        assert_eq!(score_bet(json, 2, 1), 3);
    }

    // End-to-end: a REGULAR 90' match settles on fullTime 0-2. The exact 0-2 bet
    // scores 3; an outcome-only bet (0-3, right away winner, wrong score) scores
    // 1, proving the non-shootout path still routes through fullTime.
    #[test]
    fn regular_match_scores_the_full_time_score() {
        let json = r#"{
            "winner": "AWAY_TEAM", "duration": "REGULAR",
            "fullTime": { "home": 0, "away": 2 }
        }"#;
        assert_eq!(score_bet(json, 0, 2), 3);
        assert_eq!(score_bet(json, 0, 3), 1);
    }

    // A match decided in extra time (no shootout) already reports the 120'
    // result in fullTime, so it is used as-is.
    #[test]
    fn extra_time_winner_uses_full_time() {
        let json = r#"{
            "winner": "HOME_TEAM", "duration": "EXTRA_TIME",
            "fullTime": { "home": 2, "away": 1 },
            "regularTime": { "home": 1, "away": 1 },
            "extraTime": { "home": 1, "away": 0 }
        }"#;
        assert_eq!(settled(json), (Some(2), Some(1)));
    }

    // A normal 90' match has no regularTime/extraTime breakdown; fullTime wins.
    #[test]
    fn regular_match_uses_full_time() {
        let json = r#"{
            "winner": "AWAY_TEAM", "duration": "REGULAR",
            "fullTime": { "home": 0, "away": 2 }
        }"#;
        assert_eq!(settled(json), (Some(0), Some(2)));
    }
}
