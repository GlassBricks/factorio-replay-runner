# CLAUDE.md

## Overview

This is a Rust workspace for running and analyzing Factorio game replays.
It downloads replay files, injects custom Lua scripts (compiled from TypeScript), runs them in Factorio, and validates results against rules.

## `replay_script` Crate

Contains TypeScript → Lua compilation system:

- **TypeScript source**: `tstl_src/` directory contains TypeScript files
- **Build process**: `build.rs` runs `bun` and `tstl` (TypescriptToLua) to compile TS → Lua
- **Code generation**: `build.rs` generates `replay_scripts.rs` by parsing YAML frontmatter from `tstl_src/rules/*.ts` files
- **API**: `ReplayScripts` struct with boolean fields for each script, renders to Lua via `Display` trait

Script metadata is defined in YAML comments at the top of each TypeScript file in `tstl_src/rules/`:

```typescript
// name: script_name
// param_type: bool
// default: true
// enable_if: param
// enable_value: true
```

## Error Handling Architecture

The codebase uses a type-based error classification system:

1. **Per-crate typed errors**: Each crate defines semantic error enums
2. **CLI boundary classification**: The `cli` crate classifies errors at the boundary
   - `ClassifiedError`: Wraps errors with classification
   - `ErrorClass`: `Final` (submitter fault), `Retryable` (infrastructure), `RateLimited` (with optional retry delay)
   - Each typed error has `From<ErrorType> for ClassifiedError` implementation

## Project Conventions

- All dependencies must be workspace dependencies
- **`.env`**: Environment variables (OAuth tokens, API keys) — required for download services
- **`speedrun_rules.yaml`**: Game/category rules for speedrun.com integration

## Editing

- run `cargo clippy` and `cargo fmt` after changes
