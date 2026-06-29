use sqlx::PgPool;

pub const POINTS_EXACT: i32 = 3;
pub const POINTS_OUTCOME: i32 = 1;

#[allow(dead_code)]
pub fn compute_points(
    bet_home: i32,
    bet_away: i32,
    actual_home: i32,
    actual_away: i32,
    pens: Option<(i32, i32)>,
) -> i32 {
    if bet_home == actual_home && bet_away == actual_away {
        return POINTS_EXACT;
    }
    if (bet_home - bet_away).signum() == match_outcome(actual_home, actual_away, pens) {
        return POINTS_OUTCOME;
    }
    0
}

/// The complete-game result as a sign (1 home, -1 away, 0 draw). Uses the
/// on-pitch score, which already includes extra time; when that ended level the
/// penalty shootout breaks the tie, so a knockout decided on penalties counts as
/// a win for the side that advanced rather than a draw.
fn match_outcome(home: i32, away: i32, pens: Option<(i32, i32)>) -> i32 {
    if home != away {
        return (home - away).signum();
    }
    match pens {
        Some((ph, pa)) if ph != pa => (ph - pa).signum(),
        _ => 0,
    }
}

pub async fn recompute_all(pool: &PgPool) -> anyhow::Result<()> {
    // `points` stores the EFFECTIVE score: the base (3 / 1 / 0) times the bet's
    // joker multiplier (1 or 2). Storing it pre-multiplied means every ranking
    // sum (leaderboard, profile, digest, …) reflects the joker for free. The
    // flip side: "exact score" can no longer be detected as `points = 3` — an
    // exact bet now scores 3 or 6 — so callers test `points >= 3` instead (safe
    // because an outcome-only bet scores at most 2; see the multiplier CHECK).
    sqlx::query(
        r#"
        UPDATE bets b
        SET points = b.multiplier * CASE
            WHEN b.home_score = m.home_score AND b.away_score = m.away_score THEN $1
            -- The outcome point follows the complete game: the on-pitch score
            -- (which already includes extra time), or the penalty shootout when
            -- that ended level, so a knockout won on penalties counts as a win
            -- for the side that advanced rather than a draw.
            WHEN sign(b.home_score - b.away_score) = CASE
                WHEN m.home_score <> m.away_score THEN sign(m.home_score - m.away_score)
                WHEN m.pens_home IS NOT NULL AND m.pens_away IS NOT NULL THEN sign(m.pens_home - m.pens_away)
                ELSE 0
            END THEN $2
            ELSE 0
        END,
        updated_at = NOW()
        FROM matches m
        WHERE b.match_id = m.id
          AND m.status = 'FINISHED'
          AND m.home_score IS NOT NULL
          AND m.away_score IS NOT NULL
        "#,
    )
    .bind(POINTS_EXACT)
    .bind(POINTS_OUTCOME)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_score_wins_three() {
        assert_eq!(compute_points(2, 1, 2, 1, None), 3);
    }

    #[test]
    fn good_winner_one_point() {
        assert_eq!(compute_points(3, 0, 2, 1, None), 1);
    }

    #[test]
    fn good_draw_one_point() {
        assert_eq!(compute_points(1, 1, 2, 2, None), 1);
    }

    #[test]
    fn wrong_zero() {
        assert_eq!(compute_points(0, 2, 2, 1, None), 0);
    }

    #[test]
    fn exact_after_extra_time_still_wins_three() {
        // 1-1 after extra time, won on penalties: the exact-score point targets
        // the on-pitch score, so a 1-1 prediction still scores three.
        assert_eq!(compute_points(1, 1, 1, 1, Some((4, 2))), 3);
    }

    #[test]
    fn picking_the_shootout_winner_scores_the_outcome() {
        // 1-1, home advances on penalties. A home-win prediction takes the
        // outcome point; an away-win prediction gets nothing.
        assert_eq!(compute_points(2, 1, 1, 1, Some((4, 2))), 1);
        assert_eq!(compute_points(0, 1, 1, 1, Some((4, 2))), 0);
        // Away advances on penalties: now the away-win prediction scores.
        assert_eq!(compute_points(0, 1, 1, 1, Some((2, 4))), 1);
    }

    #[test]
    fn a_draw_prediction_loses_a_shootout_game() {
        // The match had a winner, so a non-exact draw prediction scores zero
        // even though the pitch score was level.
        assert_eq!(compute_points(2, 2, 1, 1, Some((4, 2))), 0);
    }
}
