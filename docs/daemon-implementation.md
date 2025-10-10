# Daemon Loop Implementation Design

## Overview

Breaking down the daemon loop into small, testable, single-responsibility functions.

## Module Structure

**File:** `crates/cli/src/daemon.rs`

## Function Breakdown

### 1. Top-Level Orchestration

```rust
pub async fn run_daemon(config: DaemonConfig, install_dir: PathBuf, output_dir: PathBuf) -> Result<()>
```

**Responsibility:** Main entry point, coordinates lifecycle
- Initialize database and dependencies
- Set up signal handler
- Run main loop until shutdown requested
- Clean up on exit

**Testability:** Integration test, verify shutdown behavior

---

### 2. Main Loop Iteration

```rust
async fn daemon_iteration(
    db: &Database,
    config: &DaemonConfig,
    game_rules: &[GameConfig],
    install_dir: &Path,
    output_dir: &Path,
) -> Result<IterationAction>

enum IterationAction {
    Continue,
    Shutdown,
}
```

**Responsibility:** Single iteration of daemon loop
- Execute poll phase
- Execute process phase
- Determine if should continue

**Testability:** Unit test with mock database

**Notes:** Returns action to allow top-level to control loop

---

### 3. Poll Phase

```rust
async fn poll(
    db: &Database,
    config: &DaemonConfig,
    game_rules: &[GameConfig],
) -> Result<usize>
```

**Responsibility:** Poll all configured game/categories
- Iterate through game_rules
- Check if poll interval elapsed for each
- Call `poll_single_game_category` for each due
- Return count of new runs discovered

**Testability:** Unit test with mock database and time

---

### 4. Poll Single Game/Category

```rust
async fn poll_category(
    db: &Database,
    game_rule: &GameConfig,
    cutoff_date: DateTime<Utc>,
) -> Result<Vec<Run>>
```

**Responsibility:** Poll single game/category for new runs
- Get last poll time from database (or use cutoff_date)
- Fetch runs from API with filter: submitted > last_poll_time
- Map API runs to database Run structs
- Insert discovered runs into database
- Update poll_state
- Return discovered runs

**Testability:** Unit test with mock database and API client

**Dependencies:** Uses `run_lookup` module

---

### 5. Process Phase

```rust
async fn poll_runs(
    db: &Database,
    game_rules: &[GameConfig],
    install_dir: &Path,
    output_dir: &Path,
) -> Result<Option<ProcessResult>>

struct ProcessResult {
    run_id: String,
    status: RunStatus,
    verification_status: Option<VerificationStatus>,
}
```

**Responsibility:** Process next discovered run
- Get oldest discovered run from database
- Return None if no runs to process
- Call `process_single_run`
- Return process result

**Testability:** Unit test with mock database

---

### 6. Process Single Run

```rust
async fn process_run(
    db: &Database,
    run: Run,
    game_rule: &GameConfig,
    install_dir: &Path,
    output_dir: &Path,
) -> Result<ProcessResult>
```

**Responsibility:** Download and validate a single run
- Mark run as processing in database
- Download save file
- Run validation (reuse existing code)
- Parse results
- Update database with result
- Return process result

**Testability:** Integration test with test save file

**Error Handling:**
- Catch all errors, map to RunStatus::Error
- Store error message in database
- Never panic or crash daemon

---

### 7. Find Matching Game Rule

```rust
fn find_game_config<'a>(
    game_rules: &'a [GameConfig],
    game_id: &str,
    category_id: &str,
) -> Option<&'a GameConfig>
```

**Responsibility:** Find rule config for game/category
- Linear search through rules
- Match on game_id and category_id

**Testability:** Pure function, trivial unit test

---

### 8. Should Poll Game Category

```rust
fn should_poll(
    poll_state: Option<&PollState>,
    poll_interval: Duration,
    now: DateTime<Utc>,
) -> bool
```

**Responsibility:** Determine if enough time elapsed to poll again
- Compare last_poll_time + interval vs now
- Handle missing poll_state (first poll)

**Testability:** Pure function, easy unit test

---

### 9. Map API Run to Database Run

```rust
fn api_run_to_db_run(
    api_run: &speedrun_api::Run,
    game_rule: &GameConfig,
) -> Run
```

**Responsibility:** Convert API type to database type
- Extract relevant fields
- Set initial status to Discovered
- Set timestamps

**Testability:** Pure function, unit test

---

### 10. Sleep and Check Shutdown

```rust
async fn interruptible_sleep(
    duration: Duration,
    shutdown_signal: &mut SignalReceiver,
) -> ShouldShutdown

enum ShouldShutdown {
    Yes,
    No,
}
```

**Responsibility:** Sleep but wake early on shutdown signal
- Use tokio::select! to race sleep vs signal
- Return whether shutdown was requested

**Testability:** Unit test with mock signal

---

## Data Flow

```
run_daemon
  ├─> setup signal handler
  ├─> loop:
  │     ├─> daemon_iteration
  │     │     ├─> poll_phase
  │     │     │     └─> for each game_rule:
  │     │     │           ├─> should_poll?
  │     │     │           └─> poll_single_game_category
  │     │     │                 ├─> fetch from API
  │     │     │                 ├─> api_run_to_db_run
  │     │     │                 └─> insert into db
  │     │     └─> process_phase
  │     │           ├─> get next discovered run
  │     │           └─> process_single_run
  │     │                 ├─> find_game_config
  │     │                 ├─> download save
  │     │                 ├─> run validation
  │     │                 └─> update db
  │     └─> interruptible_sleep
  └─> cleanup
```

## Error Handling Strategy

### Recoverable Errors (Continue Daemon)
- Network timeouts during API poll → log, continue
- Download failures → mark run as error, continue
- Validation failures → mark run as failed/error, continue
- Database write failures → log, continue (may need retry)

### Unrecoverable Errors (Shutdown Daemon)
- Database connection lost → shutdown
- Configuration invalid → shutdown at startup
- Signal handler setup failed → shutdown at startup

### Function-Level Error Handling

Each function handles errors differently:

1. **Pure functions** (`should_poll`, `api_run_to_db_run`):
   - Return `Result` or `Option`
   - No side effects

2. **Database functions** (`poll_single_game_category`, `process_single_run`):
   - Catch errors, log them
   - Store error state in database
   - Return `Result` with context

3. **Top-level** (`run_daemon`, `daemon_iteration`):
   - Decide whether to continue or shutdown
   - Log errors appropriately

## Testing Strategy

### Unit Tests
- `should_poll`: test time logic
- `api_run_to_db_run`: test conversion
- `find_game_config`: test lookup logic

### Integration Tests
- `poll_single_game_category`: test with mock API + real database
- `process_single_run`: test with test save file + real database
- `daemon_iteration`: test full iteration with mocks

### End-to-End Tests
- Full daemon run with test API server
- Verify graceful shutdown
- Verify run processing from discovery to completion

## Implementation Order

1. Write pure helper functions first (testable immediately)
2. Write database operations (already done in step 1)
3. Write single game/category poll function
4. Write single run process function
5. Write phase functions
6. Write iteration function
7. Write top-level orchestration
8. Add signal handling
9. Add sleep/shutdown logic

## Dependencies

### External
- `tokio`: async runtime, signals, sleep
- `speedrun-api`: API client (already used in run_lookup.rs)
- `sqlx`: database (already used)

### Internal
- `database` module (step 1)
- `run_lookup` module (step 3)
- `run_replay` module (existing)
- `rules` module (existing)

## Signal Handling

Use existing `process_manager` module from `factorio_manager` crate:
- Register shutdown handler at daemon startup
- Check `should_shutdown()` at key points:
  - Before each iteration
  - During sleep
  - During long-running operations (if possible)
- Interrupt current run processing gracefully
- Update database before exit

## Configuration

Reuse `DaemonConfig` from step 2:
```rust
pub struct DaemonConfig {
    pub poll_interval_seconds: u64,
    pub database_path: PathBuf,
    pub cutoff_date: String,
}
```
