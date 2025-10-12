-- runs table
CREATE TABLE runs (
    run_id TEXT PRIMARY KEY NOT NULL,
    game_id TEXT NOT NULL,
    category_id TEXT NOT NULL,
    submitted_date TEXT NOT NULL,
    status TEXT NOT NULL,
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    next_retry_at TEXT,
    error_class TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_runs_status ON runs(status);
CREATE INDEX idx_runs_game_category ON runs(game_id, category_id);
CREATE INDEX idx_runs_submitted_date ON runs(game_id, category_id, submitted_date DESC);
CREATE INDEX idx_runs_retry ON runs(next_retry_at) WHERE next_retry_at IS NOT NULL;
