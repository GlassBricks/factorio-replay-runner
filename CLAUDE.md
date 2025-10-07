# CLAUDE.md

## Overview

This is a Rust workspace for running and analyzing Factorio game replays.
It downloads replay files, injects custom Lua scripts (compiled from TypeScript), runs them in Factorio, and validates results against rules.

## Build Commands

```bash
# Build all crates
cargo build
# Run tests for all crates
cargo test

# Run tests for specific crate
cargo test -p cli
cargo test -p factorio_manager
cargo test -p replay_script

# Format code (uses custom rustfmt.toml)
cargo fmt

# Run the CLI binary
cargo run --bin cli -- <args>
```

## Crate Architecture

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
- `rules.rs`: `RunRules`, `SrcRunRules`, `GameRules` - defines validation rules
- `src_integration.rs`: speedrun.com integration
- Two main commands:
  - `run`: Run replay on local save file with rules
  - `run-src`: Run replay fetched from speedrun.com

## Development Workflow

### Working with TypeScript/Lua Scripts

When modifying replay scripts in `crates/replay_script/tstl_src/rules/`:
1. Edit TypeScript files
2. Rebuild with `cargo build -p replay_script` (triggers `build.rs`)
3. Generated Lua appears in `target/debug/build/replay_script-*/out/rules/`
4. Generated Rust code in `target/debug/build/replay_script-*/out/replay_scripts.rs`

## Code Style

- **Idiomatic Rust**: No unsafe code, prefer functional style
- **Prefer iterators**: Use `iter()` methods over for loops
- **Prefer functional combinators**: `map_err()`, `ok_or_else()`, `bool::then()` over simple if/match
- **Avoid deep nesting**: Break into smaller functions or invert control flow
- **No code comments**: Self-documenting code preferred
- **Workspace dependencies**: All dependencies should be workspace dependencies

## Configuration Files

- **`.env`**: Environment variables (OAuth tokens, API keys) - required for download services
- **`speedrun_rules.yaml`**: Game/category rules for speedrun.com integration
- **Run rules files**: YAML/JSON files defining `RunRules` structure (scripts to enable, mods expected)
