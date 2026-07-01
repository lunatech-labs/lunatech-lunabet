-- Re-add the penalty shootout score, this time purely for display: finished
-- knockout cards show it next to the after-extra-time score (e.g. "1 - 1
-- (t.a.b. 4-2)"). It does NOT affect point attribution, which stays on the
-- score after at most 120 minutes. NULL for every match not decided on
-- penalties. (Columns were added in 20260629000001 and dropped in
-- 20260629000002 when penalties briefly counted towards points.)
ALTER TABLE matches ADD COLUMN pens_home INTEGER;
ALTER TABLE matches ADD COLUMN pens_away INTEGER;
