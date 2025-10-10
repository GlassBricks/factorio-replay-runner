# MVP: Automatic Speedrun Verification

## Overview

A daemon that automatically discovers, downloads, and validates Factorio speedruns from speedrun.com.

**MVP Goal:** Get a working daemon quickly to validate the approach. Manual retry for failures.

## Core Flow

1. Poll speedrun.com API every 15 minutes for new runs
2. Store discovered runs in SQLite database
3. Process runs sequentially: download → validate → record result
4. Manual intervention for failed runs via `reset-run` command

## Database Schema

**Location:** `./run_verification.db`

### Table: `runs`

| Column                | Type             | Description                             |
| --------------------- | ---------------- | --------------------------------------- |
| `run_id`              | TEXT PRIMARY KEY | speedrun.com run ID                     |
| `game_id`             | TEXT NOT NULL    | speedrun.com game ID                    |
| `category_id`         | TEXT NOT NULL    | speedrun.com category ID                |
| `runner_name`         | TEXT             | speedrunner username                    |
| `submitted_date`      | TEXT NOT NULL    | when run was submitted                  |
| `status`              | TEXT NOT NULL    | processing status                       |
| `error_message`       | TEXT             | last error if failed                    |
| `verification_status` | TEXT             | passed/failed if verification succeeded |
| `created_at`          | TEXT NOT NULL    | record creation timestamp               |
| `updated_at`          | TEXT NOT NULL    | last update timestamp                   |

**Indexes:**

```sql
CREATE INDEX idx_runs_status ON runs(status);
CREATE INDEX idx_runs_game_category ON runs(game_id, category_id);
```

#### Table: `poll_state`

**MVP - Required for tracking last poll time per game/category:**

| Column              | Type          | Description                    |
| ------------------- | ------------- | ------------------------------ |
| `game_id`           | TEXT NOT NULL | speedrun.com game ID           |
| `category_id`       | TEXT NOT NULL | speedrun.com category ID       |
| `last_poll_time`    | TEXT NOT NULL | last poll attempt timestamp    |
| `last_poll_success` | TEXT NOT NULL | last successful poll timestamp |
| PRIMARY KEY         |               | (game_id, category_id)         |

## Status Model

1. **discovered** - Found via API, not yet processed
2. **processing** - Currently downloading or validating
3. **passed** - Validation successful, all checks passed
4. **failed** - Validation successful, some checks failed
5. **error** - Validation not successful
6. **skipped** - Manually skipped

**Status Flow:**

```
discovered → processing → passed/failed/error
                ↑            ↓
                └── (manual reset via reset-run)
```

## Daemon Loop

### Startup

1. Load configuration from `daemon-config.yaml` and `speedrun_rules.yaml` (CLI arg with default)
2. Initialize database (create tables if needed)
3. Verify Factorio install directory exists

### Main Loop

1. **Poll phase:** For each configured game/category:
   - Check if `poll_interval` (5 min) elapsed
   - Fetch new runs with submitted date > last_poll_time
   - Insert as `status="discovered"`
   - Update `poll_state`

2. **Process phase:**
   - Find oldest run with `status="discovered"`
   - Update to `status="processing"`
   - Download save file
   - Run validation
   - Update to `"passed"`, `"failed"`, or `error` with error_message
   - Record output_log_path, max_msg_level, exited_successfully

3. Sleep 30 seconds, repeat

### Graceful Shutdown

- Listen for SIGINT/SIGTERM
- Interrupt current run processing
- Update status appropriately
- Exit cleanly

## Configuration

`daemon-config.yaml`

```yaml
poll_interval_seconds: 300 # 5 minutes
database_path: ./run_verification.db
cutoff_date: "2025-01-01"
```

## CLI Commands

### `daemon` - Run daemon mode

```bash
cli daemon [OPTIONS]
```

**Options:**

- `--config <PATH>` - Config file (default: daemon-config.yaml)
- `--rules <PATH>` - Speedrun rules file (default: speedrun_rules.yaml)
- `--install-dir <PATH>` - Factorio installs dir
- `--output-dir <PATH>` - Log output dir

### `reset-run` - Reset failed run

```bash
cli reset-run <run_id> [OPTIONS]
```

**Options:**

- `--database <PATH>`
- `--status <STATUS>` - Set to this status (default: discovered)

**Use case:** Retry failed runs by resetting to `discovered`

## Implementation Steps

### 1. Database Infrastructure

**In:** `crates/cli`

Subfolder: `src/database`

- RunStatus enum
- Run, PollState structs
- Database operations: init, insert, update, get, list
- `get_next_discovered_run()` - simple query ORDER BY discovered_date

External api only has abstract backing methods

### 2. Configuration

`crates/daemon/src/config.rs`:

- Add DaemonConfig

### 3. Run Lookup

`src/run_lookup.rs`:

- `poll_game_category()` using speedrun-api crate
- Filter by submitted date > last_poll_time
- Handle pagination

### 4. Daemon Loop

`src/daemon.rs`:

- Main loop with poll + process phases
- Signal handler integration
- Simple error handling: all errors → "failed"

### 5. CLI Integration

`main.rs`:

- Add Daemon, ResetRun subcommands
- Implement handlers

## Testing

- Unit tests for database operations
- Integration tests for full flow
- Manual testing with real API

## Design Principles

1. **Single-threaded** - Process one run at a time
2. **SQLite** - No external database needed
3. **Reuse existing code** - FileDownloader, run_replay_from_src_run
4. **Simple error handling** - All errors → "failed", manual retry
5. **Conservative** - Poll slowly, process carefully
