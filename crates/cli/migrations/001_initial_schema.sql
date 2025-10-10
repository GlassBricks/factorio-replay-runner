-- runs table
CREATE TABLE runs (
    run_id TEXT PRIMARY KEY NOT NULL,
    game_name TEXT NOT NULL,
    category_name TEXT NOT NULL,
    runner_name TEXT,
    submitted_date TEXT NOT NULL,
    status TEXT NOT NULL,
    error_message TEXT,
    verification_status TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_runs_status ON runs(status);
CREATE INDEX idx_runs_game_category ON runs(game_name, category_name);

-- poll_state table
CREATE TABLE poll_state (
    game_name TEXT NOT NULL,
    category_name TEXT NOT NULL,
    last_poll_time TEXT NOT NULL,
    last_poll_success TEXT NOT NULL,
    PRIMARY KEY (game_name, category_name)
);
