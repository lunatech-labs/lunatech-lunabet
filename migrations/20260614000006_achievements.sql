-- Achievements (badges): collectable milestones earned from the existing
-- scoring data. The catalogue of badge definitions lives in Rust
-- (src/achievements.rs); only the earned rows are persisted here. The primary
-- key prevents double-granting and makes the evaluation pass idempotent.

CREATE TABLE achievements (
    tenant_id UUID NOT NULL REFERENCES tenants(id),
    user_id   UUID NOT NULL REFERENCES users(id),
    code      TEXT NOT NULL,
    earned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- Flipped to TRUE once the player has seen the unlock toast, so each new
    -- badge is announced exactly once.
    seen      BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (tenant_id, user_id, code)
);

CREATE INDEX achievements_user_idx ON achievements (tenant_id, user_id);

-- RLS to match the other tenant-scoped tables. FORCE is off across the app
-- (see the 20260529 migrations), so the table-owner connection bypasses this;
-- it is defence-in-depth for any future non-owner role.
ALTER TABLE achievements ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON achievements
    USING (
        current_setting('app.bypass_rls', true) = 'on'
        OR tenant_id::text = current_setting('app.current_tenant_id', true)
    )
    WITH CHECK (
        current_setting('app.bypass_rls', true) = 'on'
        OR tenant_id::text = current_setting('app.current_tenant_id', true)
    );
