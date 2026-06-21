-- Allow a match score to be pinned manually so the football-data.org sync stops
-- overwriting it. Used when the upstream feed reports a wrong final score — e.g. a
-- VAR-disallowed goal the feed never corrected — so an admin can set the right
-- score and flip this flag; the periodic sync then leaves home_score, away_score
-- and status alone for that match while still refreshing every other field.
ALTER TABLE matches ADD COLUMN score_locked BOOLEAN NOT NULL DEFAULT FALSE;
