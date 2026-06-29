-- Record the penalty shootout score for knockout matches decided on penalties.
-- The football-data.org feed reports the shootout separately from fullTime
-- (which already includes extra-time goals), so we store it to settle the bet on
-- the complete game: the exact-score point still targets the after-extra-time
-- score, while the outcome point follows whoever advanced, including the
-- shootout. NULL for every match that did not go to penalties.
ALTER TABLE matches ADD COLUMN pens_home INTEGER;
ALTER TABLE matches ADD COLUMN pens_away INTEGER;
