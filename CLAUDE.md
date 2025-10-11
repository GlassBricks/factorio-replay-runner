# CLAUDE.md

## Overview

This is a Rust workspace for running and analyzing Factorio game replays.
It downloads replay files, injects custom Lua scripts (compiled from TypeScript), runs them in Factorio, and validates results against rules.

## Crates

### `replay_script`
Contains TypeScript → Lua compilation system:
- **TypeScript source**: `tstl_src/` directory contains TypeScript files
- **Build process**: `build.rs` runs `bun` and `tstl` (TypescriptToLua) to compile TS → Lua
- **Code generation**: `build.rs` generates `replay_scripts.rs` by parsing YAML frontmatter from `tstl_src/rules/*.ts` files
- **API**: `ReplayScripts` struct with boolean fields for each script, renders to Lua via `Display` trait
- **Parsing**: `ReplayLog` and `LogMessage` types parse output from replay scripts

Script metadata is defined in YAML comments at the top of each TypeScript file in `tstl_src/rules/`:
```typescript
// name: script_name
// param_type: bool
// default: true
// enable_if: param
// enable_value: true
```

### `factorio_manager`
Manages Factorio installations and process execution:
- `FactorioInstallDir`: manages Factorio version installations
- `FactorioInstance`: runs Factorio processes
- `SaveFile`/`WrittenSaveFile`: save file manipulation and mod injection
- `ExpectedMods`: mod validation
- `process_manager`: signal handling for graceful shutdown

### `zip_downloader`
Downloads and validates zip files from various sources:
- Services: Dropbox, Google Drive, speedrun.com integration
- Basic Security validation for safe downloads

### `cli`
Main entry point that orchestrates everything:
- `main.rs`: CLI argument parsing with clap
- `run_replay.rs`: core replay execution logic
- `config.rs`: `RunRules`, `SrcRunRules`, `GameConfig`, `CategoryConfig` - defines validation rules and configuration
- `src_integration.rs`: speedrun.com integration
- `database/`: SQLite database infrastructure for run tracking
  - `types.rs`: `RunStatus`, `VerificationStatus`, `Run`, `PollState` types
  - `connection.rs`: `Database` wrapper around sqlx pool with migrations
  - `operations.rs`: database CRUD operations for runs and poll state
  - `migrations/`: SQL migration files for schema versioning
- Two main commands:
  - `run`: Run replay on local save file with rules
  - `run-src`: Run replay fetched from speedrun.com

## Working with TypeScript/Lua Scripts

After modifying replay scripts in `crates/replay_script/tstl_src/rules/`,
`build.rs` will
- Generate Lua in `target/debug/build/replay_script-*/out/rules/`
- Generate Rust in `target/debug/build/replay_script-*/out/replay_scripts.rs`

## Code Style

- **Idiomatic Rust**: No unsafe code, prefer functional style
- **Prefer iterators**: Use `iter()` methods over for loops
- **Prefer functional combinators**: `map_err()`, `ok_or_else()`, `bool::then()` over simple if/match
- **Avoid deep nesting**: Break into smaller functions or invert control flow
- **No code comments**: Self-documenting code preferred
- **Workspace dependencies**: All dependencies should be workspace dependencies

- After initial code implementation, after everything works, re-visit the code again and maximize code quality:
  - Avoid deep nesting
  - Break up large functions; keep them single responsibility
  - Identify common patterns and code duplication, into reusable functions
  - Remove unnecessary or dead code; always err on simplicity

## Architecture and code organization

- Keep it simple, YAGNI, only add the minimum needed to support current goal

## Configuration Files

- **`.env`**: Environment variables (OAuth tokens, API keys) - required for download services
- **`speedrun_rules.yaml`**: Game/category rules for speedrun.com integration

## Git Commit Guidelines

- **Subject line**: Imperative mood, 50 chars max, capitalized, no period
  - `feat`: New feature
  - `fix`: Bug fix
  - `refactor`: Code restructuring
  - `test`: Test changes
  - `docs`: Documentation
  - `chore`: Maintenance tasks
- **Body**: Explain why, not what (optional)
- **One logical change per commit**: Keep commits focused and atomic
