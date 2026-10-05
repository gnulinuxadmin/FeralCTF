-- Runtime competition state set by the admin start/end/freeze controls.
-- No row means "running" so existing deployments are not locked out.
CREATE TABLE IF NOT EXISTS competition_state (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    started_at INTEGER,
    ended_at   INTEGER,
    frozen_at  INTEGER
);
