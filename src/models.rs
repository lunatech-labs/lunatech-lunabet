use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::i18n::Locale;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub is_admin: bool,
    #[allow(dead_code)]
    pub created_at: DateTime<Utc>,
    pub stake_eur: Option<i32>,
    #[allow(dead_code)]
    pub stake_chosen_at: Option<DateTime<Utc>>,
    pub paid_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Match {
    pub id: i64,
    pub competition: String,
    pub stage: Option<String>,
    pub group_name: Option<String>,
    pub home_team: String,
    pub away_team: String,
    pub home_team_code: Option<String>,
    pub away_team_code: Option<String>,
    pub kickoff_at: DateTime<Utc>,
    pub status: String,
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
}

/// A match cannot still be in play this long after kickoff. Past this point we
/// treat it as concluded even when the upstream feed never moved it to
/// FINISHED (an abandoned game, an AWARDED walkover, or a status the feed got
/// stuck on), so it stops rendering as "in progress" forever on the Today and
/// Matches screens. Mirrors the daily digest's completion grace
/// (DIGEST_COMPLETION_GRACE_HOURS in notifications.rs).
const MAX_MATCH_DURATION_HOURS: i64 = 4;

/// Feed statuses that mean a match is under way or already decided. Anything
/// outside this set (SCHEDULED, TIMED, POSTPONED, CANCELLED, or a value the
/// feed adds later) is a pre-match state.
pub fn has_kicked_off(status: &str) -> bool {
    matches!(
        status,
        "IN_PLAY" | "PAUSED" | "EXTRA_TIME" | "PENALTY_SHOOTOUT" | "FINISHED" | "AWARDED" | "SUSPENDED"
    )
}

/// Whether bets can still be placed on a match with this kickoff and status.
/// Open until the match actually starts: the kickoff is in the future and the
/// feed hasn't moved it into a live or finished state. Deliberately a blacklist
/// of started statuses rather than a whitelist of pre-match ones — the feed
/// sometimes reports an upcoming match with a status other than SCHEDULED/TIMED,
/// and betting must stay open for those instead of the card freezing as "in
/// progress" before kickoff.
pub fn betting_open(kickoff_at: DateTime<Utc>, status: &str) -> bool {
    kickoff_at > Utc::now() && !has_kicked_off(status)
}

impl Match {
    pub fn is_open_for_bets(&self) -> bool {
        betting_open(self.kickoff_at, &self.status)
    }
    pub fn has_final_result(&self) -> bool {
        self.status == "FINISHED" && self.home_score.is_some() && self.away_score.is_some()
    }

    /// Kicked off so long ago that it cannot still be in play. Used as a safety
    /// net for matches the feed never marked FINISHED.
    pub fn is_long_over(&self) -> bool {
        Utc::now() - self.kickoff_at > Duration::hours(MAX_MATCH_DURATION_HOURS)
    }

    /// Should this match render as concluded (final-score card) rather than as
    /// open or live? True when the feed gave a final result, or when so long has
    /// passed since kickoff that it can no longer be in play. The latter stops a
    /// game the feed left unfinalised from showing as "in progress" indefinitely.
    pub fn is_concluded(&self) -> bool {
        self.has_final_result() || self.is_long_over()
    }

    /// Localised display label for the match's tournament stage. Falls back
    /// to the raw football-data.org value when the stage isn't one we know
    /// (e.g. competitions other than the World Cup).
    #[allow(dead_code)]
    pub fn stage_label(&self, loc: Locale) -> Option<&str> {
        self.stage.as_deref().map(|s| stage_label_for(s, loc))
    }
}

pub fn stage_label_for(stage: &str, loc: Locale) -> &str {
    match (loc, stage) {
        (Locale::Fr, "GROUP_STAGE") => "Phase de groupes",
        (Locale::En, "GROUP_STAGE") => "Group stage",
        (Locale::Fr, "LAST_32") => "16èmes de finale",
        (Locale::En, "LAST_32") => "Round of 32",
        (Locale::Fr, "LAST_16") => "8èmes de finale",
        (Locale::En, "LAST_16") => "Round of 16",
        (Locale::Fr, "QUARTER_FINALS") => "Quarts de finale",
        (Locale::En, "QUARTER_FINALS") => "Quarter-finals",
        (Locale::Fr, "SEMI_FINALS") => "Demi-finales",
        (Locale::En, "SEMI_FINALS") => "Semi-finals",
        (Locale::Fr, "THIRD_PLACE") => "Match pour la 3e place",
        (Locale::En, "THIRD_PLACE") => "Third-place match",
        (Locale::Fr, "FINAL") => "Finale",
        (Locale::En, "FINAL") => "Final",
        (_, other) => other,
    }
}

/// Canonical ordering of WC stages for grouping the matches page.
pub const STAGE_ORDER: &[&str] = &[
    "GROUP_STAGE",
    "LAST_32",
    "LAST_16",
    "QUARTER_FINALS",
    "SEMI_FINALS",
    "THIRD_PLACE",
    "FINAL",
];

#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)]
pub struct Bet {
    pub id: Uuid,
    pub user_id: Uuid,
    pub match_id: i64,
    pub home_score: i32,
    pub away_score: i32,
    pub points: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(kickoff: DateTime<Utc>, status: &str, score: Option<(i32, i32)>) -> Match {
        Match {
            id: 1,
            competition: "World Cup".into(),
            stage: None,
            group_name: None,
            home_team: "A".into(),
            away_team: "B".into(),
            home_team_code: None,
            away_team_code: None,
            kickoff_at: kickoff,
            status: status.into(),
            home_score: score.map(|s| s.0),
            away_score: score.map(|s| s.1),
        }
    }

    #[test]
    fn finished_match_is_concluded() {
        let m = at(Utc::now() - Duration::hours(2), "FINISHED", Some((1, 0)));
        assert!(m.has_final_result());
        assert!(m.is_concluded());
        assert!(!m.is_open_for_bets());
    }

    #[test]
    fn live_match_is_not_concluded() {
        // Kicked off an hour ago, still in play: genuinely live, not concluded.
        let m = at(Utc::now() - Duration::hours(1), "IN_PLAY", Some((0, 0)));
        assert!(!m.has_final_result());
        assert!(!m.is_long_over());
        assert!(!m.is_concluded());
    }

    #[test]
    fn stuck_match_long_after_kickoff_is_concluded() {
        // The feed never moved it off IN_PLAY, but it kicked off long ago: it
        // can no longer be live, so it must not render as "in progress".
        let m = at(Utc::now() - Duration::hours(10), "IN_PLAY", Some((2, 1)));
        assert!(!m.has_final_result());
        assert!(m.is_long_over());
        assert!(m.is_concluded());
    }

    #[test]
    fn awarded_walkover_long_after_kickoff_is_concluded() {
        let m = at(Utc::now() - Duration::hours(10), "AWARDED", Some((3, 0)));
        assert!(!m.has_final_result());
        assert!(m.is_concluded());
    }

    #[test]
    fn future_match_is_open_and_not_concluded() {
        let m = at(Utc::now() + Duration::hours(3), "TIMED", None);
        assert!(m.is_open_for_bets());
        assert!(!m.is_long_over());
        assert!(!m.is_concluded());
    }

    #[test]
    fn future_scheduled_match_is_open() {
        let m = at(Utc::now() + Duration::hours(3), "SCHEDULED", None);
        assert!(m.is_open_for_bets());
    }

    #[test]
    fn future_match_with_unexpected_status_is_still_open() {
        // The feed reports an upcoming match with something other than
        // SCHEDULED/TIMED. Betting must stay open: kickoff is still ahead.
        for status in ["POSTPONED", "CANCELLED", "SOMETHING_NEW", ""] {
            let m = at(Utc::now() + Duration::hours(3), status, None);
            assert!(m.is_open_for_bets(), "expected open for status {status:?}");
            assert!(!m.is_concluded(), "expected not concluded for status {status:?}");
        }
    }

    #[test]
    fn started_match_is_not_open_even_before_listed_kickoff() {
        // Contradictory feed data (status says live, kickoff still ahead): keep
        // betting closed so nobody bets after play has started.
        let m = at(Utc::now() + Duration::hours(1), "IN_PLAY", Some((0, 0)));
        assert!(!m.is_open_for_bets());
    }

    #[test]
    fn past_kickoff_is_never_open() {
        let m = at(Utc::now() - Duration::minutes(5), "TIMED", None);
        assert!(!m.is_open_for_bets());
    }
}
