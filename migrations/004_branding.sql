-- Branding set from Admin -> Settings. A NULL name falls back to
-- competition.name in config.toml; a NULL logo_url shows the built-in logo.
CREATE TABLE IF NOT EXISTS branding (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    name       TEXT,
    logo_url   TEXT,
    updated_at INTEGER NOT NULL
);
