# Automatic Retry Mechanism with Exponential Backoff

## Overview

This plan introduces automatic retry logic with exponential backoff for runs that fail with retryable errors. The error classification system (see `error-classification.md`) provides the foundation by categorizing errors into `Final`, `Retryable`, and `RateLimited`.

## Design Philosophy

### Principles

1. **Leverage existing error classification**: Use `ErrorClass` to determine retry behavior
2. **Exponential backoff**: Prevent overwhelming infrastructure with repeated failures
3. **Bounded retries**: Give up after a reasonable number of attempts
4. **Graceful degradation**: Don't block new work while waiting for retries
5. **Transparency**: Track retry state in the database for observability

### Retry Strategy

**Final errors** → No retry, mark as permanently failed
**Retryable errors** → Retry with exponential backoff (up to max attempts)
**RateLimited errors** → Respect `retry_after` hint, otherwise exponential backoff

## Architecture

### Database Schema Changes

Update the `runs` table creation to include retry tracking columns:

```sql
CREATE TABLE runs (
    -- existing columns --
    retry_count INTEGER NOT NULL DEFAULT 0,
    next_retry_at TEXT,
    error_class TEXT
);

CREATE INDEX idx_runs_retry ON runs(next_retry_at) WHERE next_retry_at IS NOT NULL;
```

**Fields:**

- `retry_count`: Number of retry attempts so far (0 = first attempt)
- `next_retry_at`: Timestamp when this run is eligible for retry (NULL = not scheduled)
- `error_class`: Stored error classification for debugging ('final', 'retryable', 'rate_limited')

### Updated Run Status Flow

```
Discovered → Processing → [Success states: Passed/NeedsReview/Failed]
                       ↓
                    Error (with classification)
                       ↓
        ┌──────────────┴──────────────┐
        ↓                             ↓
    ErrorClass::Final           ErrorClass::Retryable
    (stays Error)               (stays Error w/ next_retry_at)
                                       ↓
                                 [wait until next_retry_at]
                                       ↓
                                 Status unchanged
                                 (processor picks up when ready)
                                       ↓
                                 Processing (retry_count++)
                                       ↓
                                  [repeat]
```

### Retry Configuration

Define retry parameters in code. Add as subfield to `DaemonConfig`.

```rust
pub struct RetryConfig {
    max_attempts: u32,        // Default: 8
    initial_backoff: Duration, // Default: 60s
    max_backoff: Duration,     // Default: 3600s
    backoff_multiplier: f64,   // Default: 2.0
}
```

**Rate-limited schedule:**

- If `retry_after` is provided by the service, use that value and don't increment `retry_count`
- Otherwise, use exponential backoff

### Backoff Calculation

```rust
fn calculate_next_retry(
    retry_count: u32,
    error_class: &ErrorClass,
    config: &RetryConfig,
) -> Option<DateTime<Utc>>
```

Returns `None` for `Final` errors or when max attempts reached. For `Retryable`/`RateLimited`, calculates exponential delay capped at `max_backoff`.

## Implementation Plan

### Phase 1: Database Schema Update

1. **Update `001_initial.sql` migration**
   - Add `retry_count`, `next_retry_at`, `error_class` columns to `runs` table
   - Add index on `next_retry_at`

2. **Update `Run` struct in `cli/src/database/types.rs`**
   - Add fields: `retry_count: u32`, `next_retry_at: Option<DateTime<Utc>>`, `error_class: Option<String>`

3. **Update existing queries in `cli/src/database/operations.rs`**
   - Modify `insert_run` to initialize retry fields
   - Update all query macros to include new columns

### Phase 2: Retry Configuration

1. **Create `cli/src/retry.rs`**
   - Define `RetryConfig` struct with defaults
   - Implement `calculate_next_retry(retry_count, error_class, config) -> Option<DateTime<Utc>>`
   - Add `error_class_to_string(ErrorClass) -> &'static str` helper for database storage

### Phase 3: Database Operations

**Design Decision: Unified Query vs. Priority-Based**

We use a single `get_next_run_to_process` query that competes runs by `submitted_date`, rather than separate queries with explicit priority (new runs before retries).

**Rationale:**

- **Fairness**: A run submitted 3 days ago that needs retry shouldn't be skipped in favor of a run submitted 5 minutes ago
- **Simplicity**: Single query, single ordering criterion
- **Natural priority**: Recent submissions naturally get processed quickly, older ones eventually get their retries
- **No starvation**: Prevents pathological case where constant new submissions block all retries

**Example ordering:**

```
submitted_date  | status     | next_retry_at | processed_order
2024-01-01      | error      | 2024-01-05    | 1 (oldest, retry ready)
2024-01-03      | discovered | NULL          | 2 (new run)
2024-01-04      | discovered | NULL          | 3 (new run)
2024-01-04      | error      | 2024-01-06    | (not ready yet)
```

1. **Update `cli/src/database/operations.rs`**

   **Rename method:**
   - `get_next_discovered_run` → `get_next_run_to_process`
   - Query runs where: `status = Discovered` OR `(status = Error AND next_retry_at <= NOW())`
   - Order by `submitted_date ASC` (new runs and retries compete fairly)

   **Add helper methods:**
   - `mark_run_permanently_failed(run_id)` - Sets `next_retry_at = NULL`, keeps status as `Error`
   - `schedule_retry(run_id, retry_count, error_class, next_retry_at)` - Updates retry fields when scheduling retry
   - `clear_retry_fields(run_id)` - Resets retry fields when run succeeds

   **Modify existing:**
   - `process_replay_result` - Call `calculate_next_retry`, then either `schedule_retry` or `mark_run_permanently_failed`. On success, call `clear_retry_fields`.

### Phase 4: Processor Integration

1. **Update `cli/src/daemon/processor.rs`**

   **Simplify `find_run_to_process`:**
   - Replace `get_next_discovered_run` call with `get_next_run_to_process`
   - Single code path for both discovered and retry-eligible runs

   **Update `process_run`:**
   - Before marking `Processing`, check `run.retry_count` to log whether this is initial or retry attempt
   - Clear `next_retry_at` when starting processing (preserve `retry_count`)

### Phase 5: Testing

1. **Unit tests for retry logic**
   - Test exponential backoff calculation
   - Test max attempts enforcement
   - Test rate-limited retry scheduling
   - Test final error handling

2. **Integration tests**
   - Test retry workflow end-to-end
   - Test processor picking up retry runs
   - Test permanent failure after max attempts
   - Test successful retry clearing retry fields

3. **Database tests**
   - Test `get_next_run_to_process` ordering (discovered vs retry-eligible)
   - Test filtering by allowed categories
   - Test retry state persistence

### Phase 6: Observability

1. **Add logging**
   - Log when retry is scheduled (with attempt number and delay)
   - Log when max attempts reached
   - Log when retry succeeds
   - Distinguish between initial attempt and retry in logs

## Deployment Strategy

Since the system hasn't been deployed yet:

1. Update database schema in migration file
2. Reset/recreate database
3. Deploy code changes

## Implementation Checklist

- [x] Phase 1: Database schema update
  - [x] Update `001_initial_schema.sql` with retry fields
  - [x] Update `Run` struct with new fields
  - [x] Update all database queries
- [x] Phase 2: Retry configuration
  - [x] Create `cli/src/retry.rs`
  - [x] Implement `RetryConfig`
  - [x] Implement `calculate_next_retry`
  - [x] Implement `error_class_to_string`
  - [x] Add `RetryConfig` to `DaemonConfig`
  - [x] Write comprehensive unit tests
- [x] Phase 3: Database operations
  - [x] Rename `get_next_discovered_run` to `get_next_run_to_process`
  - [x] Add `mark_run_permanently_failed`
  - [x] Add `schedule_retry`
  - [x] Add `clear_retry_fields`
  - [x] Update `process_replay_result`
  - [x] Write comprehensive unit tests for retry database operations
- [x] Phase 4: Processor integration
  - [x] Update `find_run_to_process` to use `get_next_run_to_process`
  - [x] Update `process_run` to pass `retry_config` to `process_replay_result`
  - [x] Add `retry_config` to `RunProcessingContext`
  - [x] Update all `RunProcessingContext` creation sites
- [x] Phase 5: Testing
  - [x] Integration tests (end-to-end retry workflow, permanent failure, rate-limited scheduling)
  - [x] Database tests (ordering, category filtering, retry state persistence)
- [x] Phase 6: Observability
  - [x] Add retry logging (logging added in `process_replay_result`)
