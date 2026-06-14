//! Achievements (badges): collectable milestones derived from the existing
//! scoring data. The catalogue below is static; only earned rows are persisted
//! (`achievements` table). `evaluate_all` replays every rule across all users
//! and inserts the badges they qualify for with `ON CONFLICT DO NOTHING`, so it
//! is fully idempotent: it can run on every scoring tick, backfills history,
//! and grants newly-added catalogue entries retroactively without disturbing
//! badges already earned.

use sqlx::PgPool;
use uuid::Uuid;

use crate::i18n::Locale;

/// Point milestones, shared by the catalogue and the profile progress widget.
pub const POINT_TIERS: [i64; 3] = [50, 100, 250];
/// Streak milestones (see src/streaks.rs).
pub const STREAK_TIERS: [i32; 2] = [5, 10];
/// Smallest phase size that counts for the "marathon" badge, so betting on a
/// single-match phase (final, third-place) doesn't trivially grant it.
const MARATHON_MIN_MATCHES: i64 = 2;

pub struct BadgeDef {
    pub code: &'static str,
    pub name_fr: &'static str,
    pub name_en: &'static str,
    pub desc_fr: &'static str,
    pub desc_en: &'static str,
    /// SVG file under static/badges/.
    pub icon: &'static str,
}

impl BadgeDef {
    pub fn name(&self, loc: Locale) -> &'static str {
        loc.f(self.name_fr, self.name_en)
    }
    pub fn desc(&self, loc: Locale) -> &'static str {
        loc.f(self.desc_fr, self.desc_en)
    }
    pub fn icon_path(&self) -> String {
        format!("/static/badges/{}", self.icon)
    }
}

/// Static catalogue. The "underdog" badge from the spec is intentionally
/// omitted: it needs a notion of favourite (a ranking/odds gap) and the schema
/// carries no team-strength signal, so any rule would be an arbitrary guess.
pub const CATALOG: &[BadgeDef] = &[
    BadgeDef {
        code: "first_exact",
        name_fr: "Premier sans faute",
        name_en: "First bullseye",
        desc_fr: "Trouver ton premier score exact.",
        desc_en: "Land your first exact score.",
        icon: "first_exact.svg",
    },
    BadgeDef {
        code: "perfect_day",
        name_fr: "Journée parfaite",
        name_en: "Perfect day",
        desc_fr: "Tous tes pronos d'une même journée exacts (au moins 2 matchs).",
        desc_en: "Every prediction exact on the same day (at least 2 matches).",
        icon: "perfect_day.svg",
    },
    BadgeDef {
        code: "pts_50",
        name_fr: "Palier 50",
        name_en: "50 club",
        desc_fr: "Atteindre 50 points cumulés.",
        desc_en: "Reach 50 total points.",
        icon: "pts_50.svg",
    },
    BadgeDef {
        code: "pts_100",
        name_fr: "Palier 100",
        name_en: "100 club",
        desc_fr: "Atteindre 100 points cumulés.",
        desc_en: "Reach 100 total points.",
        icon: "pts_100.svg",
    },
    BadgeDef {
        code: "pts_250",
        name_fr: "Palier 250",
        name_en: "250 club",
        desc_fr: "Atteindre 250 points cumulés.",
        desc_en: "Reach 250 total points.",
        icon: "pts_250.svg",
    },
    BadgeDef {
        code: "streak_5",
        name_fr: "En feu",
        name_en: "On fire",
        desc_fr: "Marquer des points sur 5 matchs d'affilée.",
        desc_en: "Score points on 5 matches in a row.",
        icon: "streak_5.svg",
    },
    BadgeDef {
        code: "streak_10",
        name_fr: "Incandescent",
        name_en: "White hot",
        desc_fr: "Marquer des points sur 10 matchs d'affilée.",
        desc_en: "Score points on 10 matches in a row.",
        icon: "streak_10.svg",
    },
    BadgeDef {
        code: "marathon",
        name_fr: "Marathonien",
        name_en: "Marathoner",
        desc_fr: "Parier sur tous les matchs d'une phase complète.",
        desc_en: "Bet on every match of a full phase.",
        icon: "marathon.svg",
    },
];

pub fn def(code: &str) -> Option<&'static BadgeDef> {
    CATALOG.iter().find(|b| b.code == code)
}

/// Run every rule and persist newly-qualified badges. Idempotent.
pub async fn evaluate_all(pool: &PgPool) -> anyhow::Result<()> {
    // first_exact: at least one settled bet worth the exact-score points.
    sqlx::query(
        r#"
        INSERT INTO achievements (tenant_id, user_id, code)
        SELECT u.tenant_id, u.id, 'first_exact'
        FROM users u
        WHERE EXISTS (SELECT 1 FROM bets b WHERE b.user_id = u.id AND b.points = 3)
        ON CONFLICT DO NOTHING
        "#,
    )
    .execute(pool)
    .await?;

    // perfect_day: a calendar day (kickoff date, UTC) on which the user has at
    // least 2 settled bets and every one of them is an exact score.
    sqlx::query(
        r#"
        INSERT INTO achievements (tenant_id, user_id, code)
        SELECT u.tenant_id, b.user_id, 'perfect_day'
        FROM bets b
        JOIN users u ON u.id = b.user_id
        JOIN matches m ON m.id = b.match_id
        WHERE m.status = 'FINISHED' AND b.points IS NOT NULL
        GROUP BY u.tenant_id, b.user_id, (m.kickoff_at AT TIME ZONE 'UTC')::date
        HAVING COUNT(*) >= 2 AND MIN(b.points) = 3
        ON CONFLICT DO NOTHING
        "#,
    )
    .execute(pool)
    .await?;

    // pts_50 / pts_100 / pts_250: cumulative settled points reach a tier.
    for tier in POINT_TIERS {
        let code = format!("pts_{tier}");
        sqlx::query(
            r#"
            INSERT INTO achievements (tenant_id, user_id, code)
            SELECT u.tenant_id, u.id, $1
            FROM users u
            WHERE (SELECT COALESCE(SUM(b.points), 0)
                   FROM bets b WHERE b.user_id = u.id AND b.points IS NOT NULL) >= $2
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(&code)
        .bind(tier)
        .execute(pool)
        .await?;
    }

    // streak_5 / streak_10: best streak ever reached hits a tier.
    for tier in STREAK_TIERS {
        let code = format!("streak_{tier}");
        sqlx::query(
            r#"
            INSERT INTO achievements (tenant_id, user_id, code)
            SELECT tenant_id, id, $1 FROM users WHERE best_streak >= $2
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(&code)
        .bind(tier)
        .execute(pool)
        .await?;
    }

    // marathon: the user has a bet on every match of some phase that has at
    // least MARATHON_MIN_MATCHES matches.
    sqlx::query(
        r#"
        INSERT INTO achievements (tenant_id, user_id, code)
        SELECT u.tenant_id, u.id, 'marathon'
        FROM users u
        JOIN (
            SELECT stage, COUNT(*) AS total
            FROM matches WHERE stage IS NOT NULL
            GROUP BY stage HAVING COUNT(*) >= $1
        ) s ON TRUE
        WHERE (
            SELECT COUNT(DISTINCT b.match_id)
            FROM bets b JOIN matches m ON m.id = b.match_id
            WHERE b.user_id = u.id AND m.stage = s.stage
        ) = s.total
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(MARATHON_MIN_MATCHES)
    .execute(pool)
    .await?;

    Ok(())
}

/// Codes earned by a user, in catalogue order.
pub async fn earned_codes(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT code FROM achievements WHERE tenant_id = $1 AND user_id = $2")
            .bind(tenant_id)
            .bind(user_id)
            .fetch_all(pool)
            .await?;
    let earned: std::collections::HashSet<String> = rows.into_iter().map(|(c,)| c).collect();
    Ok(CATALOG
        .iter()
        .filter(|b| earned.contains(b.code))
        .map(|b| b.code.to_string())
        .collect())
}

/// Mark every earned badge of a user as seen, returning the codes that had not
/// been seen yet (so the caller can toast them). In catalogue order.
pub async fn take_unseen(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "UPDATE achievements SET seen = TRUE \
         WHERE tenant_id = $1 AND user_id = $2 AND seen = FALSE RETURNING code",
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    let unseen: std::collections::HashSet<String> = rows.into_iter().map(|(c,)| c).collect();
    Ok(CATALOG
        .iter()
        .filter(|b| unseen.contains(b.code))
        .map(|b| b.code.to_string())
        .collect())
}

/// All earned badge codes for a whole tenant, grouped by user, in catalogue
/// order. Used to decorate the leaderboard.
pub async fn earned_by_user(
    pool: &PgPool,
    tenant_id: Uuid,
) -> anyhow::Result<std::collections::HashMap<Uuid, Vec<String>>> {
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT user_id, code FROM achievements WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_all(pool)
            .await?;
    let mut by_user: std::collections::HashMap<Uuid, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for (uid, code) in rows {
        by_user.entry(uid).or_default().insert(code);
    }
    Ok(by_user
        .into_iter()
        .map(|(uid, set)| {
            let ordered = CATALOG
                .iter()
                .filter(|b| set.contains(b.code))
                .map(|b| b.code.to_string())
                .collect();
            (uid, ordered)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_codes_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for b in CATALOG {
            assert!(seen.insert(b.code), "duplicate code {}", b.code);
        }
    }

    #[test]
    fn point_and_streak_tiers_have_catalogue_entries() {
        for t in POINT_TIERS {
            assert!(def(&format!("pts_{t}")).is_some());
        }
        for t in STREAK_TIERS {
            assert!(def(&format!("streak_{t}")).is_some());
        }
    }
}
