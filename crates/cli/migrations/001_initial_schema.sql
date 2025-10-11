-- runs table
CREATE TABLE runs (
    run_id TEXT PRIMARY KEY NOT NULL,
    game_id TEXT NOT NULL,
    category_id TEXT NOT NULL,
    submitted_date TEXT NOT NULL,
    status TEXT NOT NULL,
    error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_runs_status ON runs(status);
CREATE INDEX idx_runs_game_category ON runs(game_id, category_id);

-- poll_state table
CREATE TABLE poll_state (
    game_id TEXT NOT NULL,
    category_id TEXT NOT NULL,
    last_poll_time TEXT NOT NULL,
    last_poll_success TEXT NOT NULL,
    PRIMARY KEY (game_id, category_id)
);
