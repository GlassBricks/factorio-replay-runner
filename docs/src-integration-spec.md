# Automatic Speedrun Verification - Functional Overview

## What It Does

An automated system that continuously monitors speedrun.com for new Factorio speedruns, downloads them, validates them against configured rules, and tracks the results.
Think of it as a "CI/CD pipeline" for speedrun verification.

## How It Works

### Discovery Phase
The system periodically polls the speedrun.com API looking for new verified runs in configured game/category pairs.
When new runs are found, they're added to a local database for processing.

### Processing Phase
Runs are processed sequentially:
1. Download the save file from the run submission
2. Process the save:
  - Inject validation scripts into the save file
  - Run Factorio with the modified save to execute validation
4. Analyze the output logs to determine if the run followed the rules
5. Record the results (passed/failed) with relevant details

### Status Tracking
Every run moves through a series of states tracked in a SQLite database:
- **Discovered**: Found via API, waiting to be processed
- **Processing**: Currently being downloaded and validated
- **Completed**: Validation completed, all rules followed
- **Failed**: Validation failed or encountered an error
- **Skipped**: Manually excluded from processing

### Error Handling & Retry

**MVP Approach (Simple):**
All failures result in "failed" status. Operators manually retry failed runs using a CLI command.

**Phase 2 Approach (Automatic):**
The system classifies errors into specific types and automatically retries transient failures:
- Network timeouts → retry with exponential backoff
- Factorio crashes → retry a limited number of times
- Invalid submissions (404, corrupted saves) → mark as submission error, don't retry
- Rate limits → retry with longer backoff

Failed runs are retried automatically up to a configured limit (typically 2-3 attempts) with increasing delays between attempts.

## User Interaction Model

### Daemon Mode
The primary mode of operation. Start the daemon and let it run continuously:
- Polls speedrun.com on a schedule
- Processes runs as they're discovered
- Handles errors gracefully
- Responds to shutdown signals cleanly

### CLI Commands
Operators interact with the system through commands:
- **daemon**: Start the continuous verification service
- **reset-run**: Manually reset a failed run for retry

### Configuration
YAML files defines what to monitor and how to validate:
- Which games and categories to track
- Speedrun.com game/category IDs
- Rule-specific parameters/rules (blueprints allowed, etc.)
In a separate file:
- Daemon behavior (poll intervals, database location)

## Key Design Principles

**Keep It Simple:**
- Process one run at a time (no parallelism complexity)
- SQLite for storage (no database server needed)
- Reuse existing download and validation code
- File-based configuration

**Reliability:**
- Graceful shutdown on signals
- Database transactions for atomic state changes
- Clear error messages and audit trail
- Conservative retry logic to avoid infinite loops

**Operational Friendly:**
- All state persisted in database
- Easy to query run status
- Manual override capabilities

## Technical Dependencies

- Existing Factorio download/execution infrastructure
- speedrun.com API for run discovery
- SQLite for state tracking
- Rust speedrun-api crate for API interaction
- Existing rule validation system (TypeScript → Lua scripts)
