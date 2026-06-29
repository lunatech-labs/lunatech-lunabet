-- Bets are settled on the score after at most 120 minutes (regulation plus
-- extra time); the penalty shootout no longer counts, so a knockout decided on
-- penalties is scored as a draw, consistent with the group stage. The shootout
-- columns added in 20260629000001 are unused again, so drop them.
ALTER TABLE matches DROP COLUMN IF EXISTS pens_home;
ALTER TABLE matches DROP COLUMN IF EXISTS pens_away;
