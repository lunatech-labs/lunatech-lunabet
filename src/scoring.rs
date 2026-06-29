use sqlx::PgPool;

pub const POINTS_EXACT: i32 = 3;
pub const POINTS_OUTCOME: i32 = 1;

#[allow(dead_code)]
pub fn compute_points(bet_home: i32, bet_away: i32, actual_home: i32, actual_away: i32) -> i32 {
    if bet_home == actual_home && bet_away == actual_away {
        return POINTS_EXACT;
    }
    let bet_sign = (bet_home - bet_away).signum();
    let actual_sign = (actual_home - actual_away).signum();
    if bet_sign == actual_sign {
        return POINTS_OUTCOME;
    }
    0
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
            -- Outcome point on the score after at most 120 minutes (regulation
            -- plus extra time, which the feed's fullTime already includes). The
            -- penalty shootout is ignored, so a knockout that goes to penalties
            -- counts as a draw, exactly like a group-stage draw.
            WHEN sign(b.home_score - b.away_score) = sign(m.home_score - m.away_score) THEN $2
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
        assert_eq!(compute_points(2, 1, 2, 1), 3);
    }

    #[test]
    fn good_winner_one_point() {
        assert_eq!(compute_points(3, 0, 2, 1), 1);
    }

    #[test]
    fn good_draw_one_point() {
        assert_eq!(compute_points(1, 1, 2, 2), 1);
    }

    #[test]
    fn wrong_zero() {
        assert_eq!(compute_points(0, 2, 2, 1), 0);
    }

    #[test]
    fn knockout_decided_on_penalties_counts_as_a_draw() {
        // Bets settle on the score after at most 120 minutes; the shootout is
        // ignored. A 1-1 that went to penalties is scored exactly like any draw:
        // 3 for the exact score, 1 for predicting a (different) draw, 0 for
        // predicting a winner.
        assert_eq!(compute_points(1, 1, 1, 1), 3);
        assert_eq!(compute_points(2, 2, 1, 1), 1);
        assert_eq!(compute_points(2, 1, 1, 1), 0);
    }
}
