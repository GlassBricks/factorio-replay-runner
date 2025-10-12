# Database Query Command

## Overview

Add a `query` subcommand to the CLI for inspecting the run database. This provides visibility into the daemon's operation and enables troubleshooting without direct SQL access.

## Command Structure

```bash
factorio-replay-cli query <subcommand> [options]
```

## Proposed Subcommands

### List Runs

Display runs with filtering and formatting options.

```bash
factorio-replay-cli query list [options]
```

**Options:**
- `--status <status>`: Filter by status (discovered, processing, passed, needs_review, failed, error)
- `--game-id <id>`: Filter by game ID
- `--category-id <id>`: Filter by category ID
- `--limit <n>`: Limit results (default: 50)
- `--offset <n>`: Skip first n results (default: 0)
- `--sort <field>`: Sort by field (submitted_date, updated_at, created_at)
- `--order <asc|desc>`: Sort order (default: desc)
- `--format <format>`: Output format (table, json, csv)
- `--database <path>`: Database path (default: run_verification.db)

**Output columns:**
- Run ID (truncated or full)
- Game/Category (resolved names or IDs)
- Submitted date
- Status
- Retry count
- Next retry (if scheduled)
- Error class (if present)

**Examples:**
```bash
# List recent failed runs
factorio-replay-cli query list --status failed --limit 10

# List all runs for specific game/category
factorio-replay-cli query list --game-id 9d35xw1l --category-id 7kjpp0q7

# Export all runs to JSON
factorio-replay-cli query list --format json --limit 1000 > runs.json

# List runs awaiting retry
factorio-replay-cli query list --status error --sort next_retry_at
```

### Show Run Details

Display detailed information for a specific run.

```bash
factorio-replay-cli query show <run_id> [options]
```

**Options:**
- `--database <path>`: Database path (default: run_verification.db)
- `--format <format>`: Output format (text, json)

**Output:**
- Full run record (all fields)
- Speedrun.com link
- Error message (if present)
- Retry history
- Timestamps (created, updated)

**Examples:**
```bash
# Show run details
factorio-replay-cli query show abc123xyz

# Get run details as JSON
factorio-replay-cli query show abc123xyz --format json
```

### Statistics

Display aggregate statistics about runs.

```bash
factorio-replay-cli query stats [options]
```

**Options:**
- `--game-id <id>`: Filter by game ID
- `--category-id <id>`: Filter by category ID
- `--since <date>`: Only include runs since date (ISO 8601)
- `--database <path>`: Database path (default: run_verification.db)

**Output:**
- Total runs by status
- Retry statistics (average retries, max retries)
- Error classification breakdown
- Recent activity (last 24h, 7d, 30d)
- Processing rate (runs/hour, runs/day)

**Examples:**
```bash
# Overall statistics
factorio-replay-cli query stats

# Stats for specific category
factorio-replay-cli query stats --game-id 9d35xw1l --category-id 7kjpp0q7

# Recent statistics
factorio-replay-cli query stats --since 2025-01-01
```

### Queue Status

Show current processing queue state.

```bash
factorio-replay-cli query queue [options]
```

**Options:**
- `--database <path>`: Database path (default: run_verification.db)
- `--config <path>`: Daemon config (for game/category filter, default: daemon.yaml)

**Output:**
- Pending runs (discovered status)
- Scheduled retries (count and next retry time)
- Currently processing (if any)
- Queue depth by game/category
- Estimated processing time (based on average)

**Examples:**
```bash
# Show queue status
factorio-replay-cli query queue

# Queue status with daemon config filtering
factorio-replay-cli query queue --config ./daemon.yaml
```

### Errors

List and analyze error patterns.

```bash
factorio-replay-cli query errors [options]
```

**Options:**
- `--limit <n>`: Limit results (default: 20)
- `--error-class <class>`: Filter by error class (final, retryable, rate_limited)
- `--group-by <field>`: Group by error message, error class, or game/category
- `--database <path>`: Database path (default: run_verification.db)

**Output:**
- Run ID
- Error message
- Error class
- Retry count
- Timestamp

**Examples:**
```bash
# Recent errors
factorio-replay-cli query errors --limit 10

# Group errors by message to identify patterns
factorio-replay-cli query errors --group-by message

# Show only final (non-retryable) errors
factorio-replay-cli query errors --error-class final
```

### Reset Run

Reset a run's status to allow reprocessing.

```bash
factorio-replay-cli query reset <run_id> [options]
```

**Options:**
- `--database <path>`: Database path (default: run_verification.db)
- `--status <status>`: Set status (default: discovered)
- `--clear-error`: Clear error message and retry fields

**Examples:**
```bash
# Reset run to discovered
factorio-replay-cli query reset abc123xyz

# Reset and clear error state
factorio-replay-cli query reset abc123xyz --clear-error
```

### Cleanup

Remove old or unwanted runs from database.

```bash
factorio-replay-cli query cleanup [options]
```

**Options:**
- `--before <date>`: Remove runs submitted before date (ISO 8601)
- `--status <status>`: Only remove runs with specific status
- `--dry-run`: Show what would be deleted without deleting
- `--database <path>`: Database path (default: run_verification.db)

**Safety:** Requires confirmation prompt unless `--force` flag is provided.

**Examples:**
```bash
# Preview cleanup of old passed runs
factorio-replay-cli query cleanup --before 2024-01-01 --status passed --dry-run

# Remove old failed runs
factorio-replay-cli query cleanup --before 2024-06-01 --status failed
```

## Implementation Notes

### Database Module Extensions

Add to `crates/cli/src/database/operations.rs`:

```rust
// Flexible query with filters
pub async fn query_runs(&self, filter: RunFilter) -> Result<Vec<Run>>

// Count runs by status
pub async fn count_runs_by_status(&self) -> Result<HashMap<RunStatus, u64>>

// Get error statistics
pub async fn get_error_stats(&self) -> Result<ErrorStats>

// Reset run status
pub async fn reset_run(&self, run_id: &str, clear_retry: bool) -> Result<()>

// Delete runs by criteria
pub async fn delete_runs(&self, filter: RunFilter) -> Result<u64>
```

### Output Formatting

Create `crates/cli/src/query_output.rs`:

```rust
pub enum OutputFormat {
    Table,
    Json,
    Csv,
}

pub trait Formatter {
    fn format_runs(&self, runs: &[Run]) -> String;
    fn format_stats(&self, stats: &Stats) -> String;
}
```

Use `comfy-table` crate for table formatting, `serde_json` for JSON, `csv` crate for CSV.

### Name Resolution

Query commands should resolve game and category IDs to human-readable names using the speedrun API when possible, with fallback to IDs if resolution fails or is disabled.

**Options:**
- `--no-resolve`: Skip name resolution, show IDs only
- `--cache <path>`: Cache resolved names to file

### Configuration

Support loading database path from:
1. Command-line flag `--database`
2. Environment variable `FACTORIO_DB_PATH`
3. Daemon config file `daemon.yaml`
4. Default `run_verification.db`

## Common Use Cases

### Monitor daemon health
```bash
factorio-replay-cli query stats && factorio-replay-cli query queue
```

### Investigate failing category
```bash
factorio-replay-cli query list --game-id X --category-id Y --status failed
factorio-replay-cli query errors --limit 5
```

### Retry stuck runs
```bash
# Find permanently failed runs
factorio-replay-cli query list --status error --format json | \
  jq '.[] | select(.next_retry_at == null) | .run_id' | \
  xargs -I {} factorio-replay-cli query reset {}
```

### Export results for analysis
```bash
factorio-replay-cli query list --format csv --limit 10000 > analysis.csv
```

### Clean up old data
```bash
# Remove runs older than 6 months with final status
factorio-replay-cli query cleanup --before 2024-06-01 \
  --status passed --status failed
```

## Future Enhancements

### Logs Command
Show logs for a specific run (requires storing logs in database or file system).

```bash
factorio-replay-cli query logs <run_id>
```

### Watch Mode
Live-updating display of queue and statistics.

```bash
factorio-replay-cli query watch [--interval 5]
```

### Bulk Operations
Process multiple runs at once.

```bash
# Reset all rate-limited errors
factorio-replay-cli query reset --error-class rate_limited --all
```

### Export/Import
Export database state for backup or migration.

```bash
factorio-replay-cli query export --output backup.json
factorio-replay-cli query import --input backup.json
```

## Testing Strategy

### Unit Tests
- Database query functions with in-memory database
- Output formatters with sample data
- Filter parsing and validation

### Integration Tests
- Full command execution with test database
- Multiple output formats
- Error handling

### Manual Testing
- Run against production database (read-only operations)
- Verify output formatting across different terminal widths
- Test name resolution with real speedrun.com API

## Implementation Status

### Phase 1: Foundation ✓
- [x] Add dependencies (`comfy-table`, `csv`, `serde_json`)
- [x] Create `crates/cli/src/query/mod.rs` module for command handling
- [x] Add `Query` command variant to `Commands` enum in `main.rs`
- [x] Create `RunFilter` struct in `database/types.rs`
- [x] Implement `Database::query_runs()` in `database/operations.rs`

### Phase 2: Core Subcommands ✓
- [x] Implement `list` subcommand with filtering and basic table output
- [x] Implement `show` subcommand with detailed text output
- [x] Implement `stats` subcommand with aggregates
- [x] Add `Database::count_runs_by_status()`
- [x] Add Hash trait to RunStatus for HashMap support

### Phase 3: Output Formatting ✓
- [x] Create `query/formatter.rs` module
- [x] Implement `TableFormatter` using `comfy-table`
- [x] Implement `JsonFormatter` using `serde_json`
- [x] Implement `CsvFormatter` using `csv` crate
- [x] Add `--format` flag support to `list` (table/json/csv) and `show` (text/json)

### Phase 4: Additional Subcommands ✓
- [x] Implement `queue` subcommand showing pending runs and scheduled retries
- [x] Implement `errors` subcommand with error_class filtering
- [x] Implement `reset` subcommand to reset runs to discovered status
- [x] Make `update_run_status` public for reset functionality

### Phase 5: Not Implemented
- [ ] `cleanup` subcommand - can be added in the future if needed
- [ ] `--group-by` for errors command
- [ ] `--since` filtering for stats
- [ ] Name resolution for game/category IDs

### Implementation Details

**Actual files created:**
- `crates/cli/src/query/mod.rs` - All query subcommands in single module
- `crates/cli/src/query/formatter.rs` - Output formatters for table/json/csv

**Modified files:**
- `crates/cli/src/main.rs` - Add Query command variant
- `crates/cli/src/database/types.rs` - Add RunFilter struct, Hash trait, Serialize trait
- `crates/cli/src/database/operations.rs` - Add query_runs, count_runs_by_status, make update_run_status public
- `Cargo.toml` - Add comfy-table, csv, serde_json dependencies

**Implementation notes:**
- All subcommands implemented in single mod.rs file for simplicity
- Sorting is always by submitted_date DESC (hardcoded)
- No name resolution for game/category IDs - shows raw IDs
- Reset always sets status to discovered (--status option not implemented)
- Tests added for query_runs and count_runs_by_status
