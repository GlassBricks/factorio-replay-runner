# Error Classification Refactoring Plan

## Design Philosophy

### Core Principles

1. **Semantic errors per crate**: Each crate defines its own meaningful error enums
2. **Classification at CLI boundary**: Only the `cli` crate knows about error classification
3. **Type-based classification**: Each error variant maps to exactly one `ErrorClass`
4. **No string inspection**: Classification is purely structural, via match statements
5. **Simplicity first**: Avoid over-engineering, add complexity only when needed

### Why This Design?

**Before**: Generic `anyhow::Error` everywhere, classification via string matching
**After**: Typed errors per domain, automatic classification via `From` impls

Benefits:

- Type-safe error propagation within each crate
- Classification logic centralized in one place
- No fragile string parsing or heuristics
- Each crate documents its failure modes explicitly
- Better error messages with structured information
- Compiler enforces complete classification coverage

## Architecture Overview

```
┌─────────────────┐
│ zip_downloader  │──┐
│  DownloadError  │  │
└─────────────────┘  │
                     │
┌─────────────────┐  │      ┌────────────────────┐
│factorio_manager │──┼─────▶│   cli crate        │
│  FactorioError  │  │      │  ClassifiedError   │──▶ Database
└─────────────────┘  │      │  ErrorClass        │    (error_message)
                     │      └────────────────────┘
┌─────────────────┐  │
│   cli/api       │──┘
│  ApiError       │
└─────────────────┘

Each crate defines semantic errors
CLI crate classifies via From<CrateError> impls
```

## Error Types

### 1. CLI Crate: Error Classification

**Location**: `crates/cli/src/error.rs` (new file)

```rust
use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    Final,
    Retryable,
    RateLimited {
        retry_after: Option<Duration>,
    },
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ClassifiedError {
    pub class: ErrorClass,
    pub message: String,
}

```
Error classes represent fault attribution:
- **Final**: Submitter's fault, invalid submission, permanent failure
- **Retryable**: Infrastructure or transient issue, can retry later
- **RateLimited**: Rate limiting by external service, retry with backoff (optional delay from service)

### 2. zip_downloader: Download Errors

**Location**: `crates/zip_downloader/src/lib.rs`

Refactor existing `DownloadError` to have one-to-one variant-to-classification mapping:

```rust
use std::io;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("No valid download link found in input")]
    NoLinkFound,

    #[error("File not accessible: {0}")]
    FileNotAccessible(#[source] anyhow::Error),

    #[error("Service error: {0}")]
    ServiceError(#[source] anyhow::Error),

    #[error("Security violation: {0}")]
    SecurityViolation(#[source] anyhow::Error),

    #[error("Rate limited: {message}")]
    RateLimited {
        retry_after: Option<Duration>,
        #[source]
        source: anyhow::Error,
    },

    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
}
```

**Covers these actual errors**:
- NoLinkFound: when no service detects a valid download link
- FileNotAccessible: HTTP 403/404, HTML responses indicating auth required, non-success status codes
- ServiceError: reqwest errors (timeouts, DNS failures), curl failures, network issues
- SecurityViolation: file too large, wrong extension, too many ZIP entries, total uncompressed too large, path traversal, invalid ZIP magic, size mismatch, invalid filename
- RateLimited: HTTP 429 responses from services
- IoError: file system operations during download

**Classification** in `cli/src/error.rs`:

```rust
impl From<DownloadError> for ClassifiedError {
    fn from(e: DownloadError) -> Self {
        let class = match &e {
            DownloadError::NoLinkFound => ErrorClass::Final,
            DownloadError::SecurityViolation(_) => ErrorClass::Final,
            DownloadError::FileNotAccessible(_) => ErrorClass::Final,
            DownloadError::ServiceError(_) => ErrorClass::Retryable,
            DownloadError::RateLimited { retry_after, .. } =>
                ErrorClass::RateLimited { retry_after: *retry_after },
            DownloadError::IoError(_) => ErrorClass::Retryable,
        };
        ClassifiedError::from_error(class, &e)
    }
}
```

### 3. factorio_manager: Factorio Operation Errors

**Location**: `crates/factorio_manager/src/error.rs` (new file)

```rust
use std::io;
use std::path::PathBuf;
use thiserror::Error;
use crate::factorio_install_dir::VersionStr;

#[derive(Debug, Error)]
pub enum FactorioError {
    #[error("Invalid save file: {0}")]
    InvalidSaveFile(#[source] anyhow::Error),

    #[error("Invalid version string: {0}")]
    InvalidVersion(#[source] anyhow::Error),

    #[error("Factorio version {version} is not supported")]
    VersionTooOld {
        version: VersionStr,
    },

    #[error("Mod mismatch. Missing: {missing_mods:?}, Extra: {extra_mods:?}")]
    ModMismatch {
        missing_mods: Vec<String>,
        extra_mods: Vec<String>,
    },

    #[error("Failed to inject replay script: {0}")]
    ScriptInjectionFailed(#[source] anyhow::Error),

    #[error("Failed to download Factorio {version}")]
    FactorioDownloadFailed {
        version: VersionStr,
        #[source]
        source: anyhow::Error,
    },

    #[error("Failed to extract Factorio: {0}")]
    ExtractionFailed(#[source] anyhow::Error),

    #[error("Factorio installation not found for version {0}")]
    InstallationNotFound(VersionStr),

    #[error("Install directory error: {0}")]
    InstallDirError(#[source] anyhow::Error),

    #[error("Failed to spawn Factorio process: {0}")]
    ProcessSpawnFailed(#[source] io::Error),

    #[error("Failed to read mod information: {0}")]
    ModInfoReadFailed(#[source] anyhow::Error),

    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
}
```

**Covers these actual errors**:
- InvalidSaveFile: ZIP has no folder, multiple folders, missing level-init.dat or control.lua
- InvalidVersion: cannot parse version from level-init.dat (malformed bytes)
- VersionTooOld: version < 2.0.65
- ModMismatch: save file has different mods than expected for category
- ScriptInjectionFailed: ZIP copy errors during script installation
- FactorioDownloadFailed: HTTP download errors, network failures when downloading Factorio binary
- ExtractionFailed: tar.xz extraction failures
- InstallationNotFound: version directory doesn't exist after download
- InstallDirError: path canonicalization failures, directory not found, create_dir_all failures
- ProcessSpawnFailed: Command::spawn IO errors
- ModInfoReadFailed: Command failures when reading mod-list.json or running Factorio commands
- IoError: general file operations

**Classification** in `cli/src/error.rs`:

```rust
impl From<FactorioError> for ClassifiedError {
    fn from(e: FactorioError) -> Self {
        let class = match &e {
            FactorioError::InvalidSaveFile(_) => ErrorClass::Final,
            FactorioError::InvalidVersion(_) => ErrorClass::Final,
            FactorioError::VersionTooOld { .. } => ErrorClass::Final,
            FactorioError::ModMismatch { .. } => ErrorClass::Final,
            FactorioError::ScriptInjectionFailed(_) => ErrorClass::Final,
            FactorioError::FactorioDownloadFailed { .. } => ErrorClass::Retryable,
            FactorioError::ExtractionFailed(_) => ErrorClass::Retryable,
            FactorioError::InstallationNotFound(_) => ErrorClass::Retryable,
            FactorioError::InstallDirError(_) => ErrorClass::Retryable,
            FactorioError::ProcessSpawnFailed(_) => ErrorClass::Retryable,
            FactorioError::ModInfoReadFailed(_) => ErrorClass::Retryable,
            FactorioError::IoError(_) => ErrorClass::Retryable,
        };
        ClassifiedError::from_error(class, &e)
    }
}
```

### 4. CLI: Speedrun API Errors

**Location**: `crates/cli/src/speedrun_api.rs`

Define errors for API interactions:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Network error: {0}")]
    NetworkError(#[source] anyhow::Error),

    #[error("Not found: {0}")]
    NotFound(#[source] anyhow::Error),

    #[error("Parse error: {0}")]
    ParseError(#[source] anyhow::Error),

    #[error("Missing required field: {0}")]
    MissingField(String),
}
```

**Covers these actual errors**:
- NetworkError: reqwest send failures, HTTP client build failures
- NotFound: HTTP 404, API requests for non-existent runs/games/categories
- ParseError: JSON deserialization failures, datetime parse failures
- MissingField: run has no comment field, no submitted date

**Classification** in `cli/src/error.rs`:

```rust
impl From<ApiError> for ClassifiedError {
    fn from(e: ApiError) -> Self {
        let class = match &e {
            ApiError::NetworkError(_) => ErrorClass::Retryable,
            ApiError::NotFound(_) => ErrorClass::Final,
            ApiError::ParseError(_) => ErrorClass::Retryable,
            ApiError::MissingField(_) => ErrorClass::Final,
        };
        ClassifiedError::from_error(class, &e)
    }
}
```

## Implementation Plan

### Phase 1: Foundation ✓

1. **Create `cli/src/error.rs`** ✓
   - Define `ErrorClass` enum with `Final`, `Retryable`, and `RateLimited` variants
   - Define `ClassifiedError` struct with public `class` and `message` fields, using `thiserror::Error`
   - Implement `from_error` helper method for creating `ClassifiedError` from any error

### Phase 2: Refactor zip_downloader

1. **Refactor `DownloadError` in `zip_downloader/src/lib.rs`**
   Update the enum to split ambiguous ServiceError/Other variants into specific ones (FileNotAccessible, ServiceError, RateLimited), remove the Other variant. Keep anyhow::Error payloads with #[source] attribute for backtrace preservation.

2. **Update `zip_downloader/src/security.rs`**
   No changes needed - already returns anyhow::Error which can be wrapped in DownloadError::SecurityViolation by the caller.

3. **Update `zip_downloader/src/services/gdrive.rs`**
   Change error handling to distinguish between FileNotAccessible (HTTP errors, HTML responses), ServiceError (network/reqwest errors), and potential RateLimited responses. Use anyhow::Context to add context before wrapping in specific variants.

4. **Update `zip_downloader/src/services/dropbox.rs`**
   Similar to gdrive, classify HTTP errors as FileNotAccessible and network errors as ServiceError, preserving error context with anyhow.

5. **Update `zip_downloader/src/services/speedrun.rs`**
   Update curl command error handling to return FileNotAccessible for HTTP failures and ServiceError for network/execution failures, using anyhow::Context for additional information.

6. **Update `zip_downloader/src/lib.rs` download flow**
   Update do_download_zip to map security validation errors to SecurityViolation variant and service errors to appropriate variants.

7. **Add classification impl in `cli/src/error.rs`**
   Add impl From<DownloadError> for ClassifiedError with match on all variants.

### Phase 3: Add FactorioError

1. **Create `factorio_manager/src/error.rs`**
   Define FactorioError enum with all variants listed above. Export from lib.rs.

2. **Update `factorio_manager/src/factorio_install_dir.rs`**
   Replace anyhow::bail! with FactorioError variants: VersionStr::try_from errors become InvalidVersion (wrapping the parse error), download failures become FactorioDownloadFailed (wrapping network errors with context), extraction failures become ExtractionFailed, path issues become InstallDirError (wrapping IO errors), get_factorio returning None becomes InstallationNotFound.

3. **Update `factorio_manager/src/expected_mods.rs`**
   Change check_expected_mods to return FactorioError::ModMismatch with extracted missing/extra mod lists instead of anyhow::Error.

4. **Update `factorio_manager/src/save_file.rs`**
   Replace Context errors with FactorioError: ZIP errors become InvalidSaveFile (wrapping zip errors), version read errors become InvalidVersion (wrapping IO errors), find_save_name errors become InvalidSaveFile (wrapping anyhow with context), script injection errors become ScriptInjectionFailed (wrapping zip/IO errors).

5. **Update `factorio_manager/src/factorio_instance.rs`**
   Change spawn to return ProcessSpawnFailed on Command::spawn IO errors, get_mod_versions failures become ModInfoReadFailed (wrapping command/parse errors with context).

6. **Update `cli/src/run_replay.rs`**
   Change anyhow::ensure! version check to return FactorioError::VersionTooOld.

7. **Add classification impl in `cli/src/error.rs`**
   Add impl From<FactorioError> for ClassifiedError with match on all variants.

### Phase 4: Add ApiError

1. **Define `ApiError` in `cli/src/speedrun_api.rs`**
   Add ApiError enum with NetworkError, NotFound, ParseError, MissingField variants.

2. **Update `cli/src/speedrun_api.rs` functions**
   Replace Context errors: HTTP client build failures become NetworkError (wrapping reqwest error), send failures become NetworkError (with context), non-success status becomes NotFound (wrapping status code info), JSON parse failures become ParseError (wrapping serde error), missing comment/submitted fields become MissingField (with field name string).

3. **Update `cli/src/run_processing.rs`**
   Update fetch_run_description to return ApiError::MissingField when comment is missing, update poll_game_category to wrap datetime parse failures in ApiError::ParseError.

4. **Add classification impl in `cli/src/error.rs`**
   Add impl From<ApiError> for ClassifiedError with match on all variants.

### Phase 5: Wire Up in CLI

1. **Update `cli/src/run_processing.rs`**
   Change download_and_run_replay signature to return Result<ReplayReport, ClassifiedError>, wrap all error returns in appropriate ClassifiedError conversions.

2. **Update `cli/src/run_replay.rs`**
   Ensure all functions propagate FactorioError variants correctly, convert any remaining anyhow errors to appropriate typed errors.

3. **Update `cli/src/database/operations.rs`**
   Change process_replay_result signature to accept Result<ReplayReport, ClassifiedError>, extract error message from ClassifiedError for database storage.

4. **Update `cli/src/daemon/processor.rs`**
   Update process_run to handle ClassifiedError from download_and_run_replay.

5. **Update `CLAUDE.md`**
   Document new error handling architecture with typed errors per crate and classification at CLI boundary.

## Error Classification Reference

### Quick Reference Table

| Error Variant            | Classification | Reason                           |
| ------------------------ | -------------- | -------------------------------- |
| **DownloadError**        |                |                                  |
| `NoLinkFound`            | Final          | No link in run comment           |
| `SecurityViolation`      | Final          | File violates requirements       |
| `FileNotAccessible`      | Final          | 403, 404, not shared publicly    |
| `ServiceError`           | Retryable      | Network, 5xx, timeouts           |
| `RateLimited`            | RateLimited    | Service rate limiting            |
| `IoError`                | Retryable      | File system issues               |
| **FactorioError**        |                |                                  |
| `InvalidSaveFile`        | Final          | Corrupted save                   |
| `InvalidVersion`         | Final          | Can't parse version              |
| `VersionTooOld`          | Final          | Version < 2.0.65                 |
| `ModMismatch`            | Final          | Wrong mods for category          |
| `ScriptInjectionFailed`  | Final          | Malformed save                   |
| `FactorioDownloadFailed` | Retryable      | Can't download Factorio          |
| `ExtractionFailed`       | Retryable      | Can't extract tar.xz             |
| `InstallationNotFound`   | Retryable      | Version dir missing after dl     |
| `InstallDirError`        | Retryable      | Path/directory issues            |
| `ProcessSpawnFailed`     | Retryable      | Can't start Factorio             |
| `ModInfoReadFailed`      | Retryable      | Can't read mod info              |
| `IoError`                | Retryable      | File operations                  |
| **ApiError**             |                |                                  |
| `NetworkError`           | Retryable      | API temporarily unavailable      |
| `NotFound`               | Final          | Run/game/category missing        |
| `ParseError`             | Retryable      | API response parsing             |
| `MissingField`           | Final          | Required field missing in run    |

## Benefits of This Design

### 1. Type Safety

- Each error is represented by a specific variant
- Match statements ensure complete handling
- Compiler catches missing classifications

### 2. Simplicity

- No string parsing or heuristics
- Clear one-to-one mapping
- Easy to understand and maintain

### 3. Maintainability

- Adding new error: define variant, add to classification match
- Changing classification: update single match arm
- Refactoring: compiler guides you

### 4. Documentation

- Error types document failure modes
- Structured data provides context
- Clear fault attribution

### 5. Future-Proofing

- Classification enables retry logic later
- Can add error_class database column
- Foundation for analytics and monitoring

## Migration Strategy

### Backward Compatibility

- Existing tests continue to work (with updates)
- Can migrate one module at a time
- No breaking changes to external APIs

### Testing

All incremental changes should be accompanied by unit tests.

## Error Coverage Verification

All errors that can cause a run to end up in "Error" state:

### Download Errors (zip_downloader crate)
- ✓ No link found → `DownloadError::NoLinkFound` (Final)
- ✓ HTTP 403/404 → `DownloadError::FileNotAccessible` (Final)
- ✓ HTML auth pages → `DownloadError::FileNotAccessible` (Final)
- ✓ Network/timeout → `DownloadError::ServiceError` (Retryable)
- ✓ HTTP 429 → `DownloadError::RateLimited` (RateLimited)
- ✓ File too large → `DownloadError::SecurityViolation` (Final)
- ✓ Wrong extension → `DownloadError::SecurityViolation` (Final)
- ✓ Too many ZIP entries → `DownloadError::SecurityViolation` (Final)
- ✓ Uncompressed too large → `DownloadError::SecurityViolation` (Final)
- ✓ Path traversal → `DownloadError::SecurityViolation` (Final)
- ✓ Invalid ZIP magic → `DownloadError::SecurityViolation` (Final)
- ✓ Size mismatch → `DownloadError::SecurityViolation` (Final)
- ✓ Invalid filename → `DownloadError::SecurityViolation` (Final)
- ✓ File I/O errors → `DownloadError::IoError` (Retryable)

### Save File Errors (factorio_manager crate)
- ✓ No folder in ZIP → `FactorioError::InvalidSaveFile` (Final)
- ✓ Multiple folders → `FactorioError::InvalidSaveFile` (Final)
- ✓ Missing level-init.dat → `FactorioError::InvalidSaveFile` (Final)
- ✓ Missing control.lua → `FactorioError::InvalidSaveFile` (Final)
- ✓ Bad version bytes → `FactorioError::InvalidVersion` (Final)
- ✓ Version < 2.0.65 → `FactorioError::VersionTooOld` (Final)
- ✓ Script injection fail → `FactorioError::ScriptInjectionFailed` (Final)

### Mod Errors (factorio_manager crate)
- ✓ Missing mods → `FactorioError::ModMismatch` (Final)
- ✓ Extra mods → `FactorioError::ModMismatch` (Final)
- ✓ Mod info read fail → `FactorioError::ModInfoReadFailed` (Retryable)

### Installation Errors (factorio_manager crate)
- ✓ Download fail → `FactorioError::FactorioDownloadFailed` (Retryable)
- ✓ Extraction fail → `FactorioError::ExtractionFailed` (Retryable)
- ✓ Missing after dl → `FactorioError::InstallationNotFound` (Retryable)
- ✓ Path/dir issues → `FactorioError::InstallDirError` (Retryable)

### Execution Errors (factorio_manager crate)
- ✓ Process spawn fail → `FactorioError::ProcessSpawnFailed` (Retryable)
- ✓ File I/O errors → `FactorioError::IoError` (Retryable)

### API Errors (cli crate)
- ✓ HTTP client fail → `ApiError::NetworkError` (Retryable)
- ✓ Request send fail → `ApiError::NetworkError` (Retryable)
- ✓ HTTP 404 → `ApiError::NotFound` (Final)
- ✓ Non-success status → `ApiError::NotFound` (Final)
- ✓ JSON parse fail → `ApiError::ParseError` (Retryable)
- ✓ DateTime parse fail → `ApiError::ParseError` (Retryable)
- ✓ Missing comment → `ApiError::MissingField` (Final)
- ✓ Missing submitted → `ApiError::MissingField` (Final)

All 45 error cases are covered by the type-based classification system.

## Future Work (Post-MVP)

1. Database Schema Enhancement, adding error classification
2. Retry logic in daemon
  ```rust
  match error.class {
      ErrorClass::Final => {
          db.mark_run_permanently_failed(run_id).await?;
      }
      ErrorClass::Retryable => {
          db.schedule_retry(run_id, backoff_duration).await?;
      }
      ErrorClass::RateLimited { retry_after } => {
          let delay = retry_after.unwrap_or(Duration::from_secs(60));
          db.schedule_retry(run_id, delay).await?;
      }
  }
  ```
