CREATE TABLE game_cache (
    game_id TEXT PRIMARY KEY NOT NULL,
    game_name TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE category_cache (
    category_id TEXT PRIMARY KEY NOT NULL,
    category_name TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_game_cache_updated_at ON game_cache(updated_at);
CREATE INDEX idx_category_cache_updated_at ON category_cache(updated_at);
