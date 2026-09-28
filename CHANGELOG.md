# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-29

### Added

- **CLI binary `kaseifu`** — renamed from `semantic-file-organizer` for cleaner UX
- **Phase 1: Observe** — FileContext generation from filesystem metadata, Spotlight (`mdls`), and extended attributes (`xattr`)
- **Phase 2: Normalize** — noise filtering, representation unification, semantic signal extraction (`M:`, `K:`, `F:`, `X:` prefixes)
- **Phase 3: Discover** — Spotlight-based (`mdfind`) candidate discovery with directory aggregation and scoring
- **Phase 4: Jev** — semantic classification with validated candidates, mock and real HTTP adapter
- **Phase 5: Execute** — transactional move with SQLite journal, collision detection, and undo
- **Phase 6: Automate** — FSEvents watcher with debounce, deduplication, and stability check
- **`kaseifu inspect`** — print FileContext JSON for any file
- **`kaseifu discover`** — list scored candidate destinations
- **`kaseifu classify`** — run full decision pipeline (dry run)
- **`kaseifu organize`** — run pipeline and optionally move files (`--execute`)
- **`kaseifu undo`** — reverse previous moves (`--last N`)
- **`kaseifu watch`** — automatically organize new/changed files in watched directories
- **`KASEIFU_DEBUG=1`** environment variable for debug output
- **Filename-based search terms** (`F:` prefix) — improves discovery accuracy by using file stem as search signal
- **`normalize_terms()`** integration — filters noise keywords before Spotlight search
- Configuration via `config/default.toml` (thresholds, dry_run, etc.)
- Jev credentials via `.env` file (`JEV_API_KEY`, `JEV_API_URL`)
- JSON schemas for FileContext, CandidateDestination, and JevDecision
- Comprehensive test suite (unit + integration tests)

### Changed

- Package name: `semantic-file-organizer` → `kaseifu`
- Binary name: `semantic-file-organizer` → `kaseifu`
- Database path: `data/semantic-file-organizer.db` → `data/kaseifu.db`
- CLI commands: `cargo run -- <cmd>` → `kaseifu <cmd>`

### Fixed

- Dead code warnings (`normalize_filename`, `remove_noise`, `MetadataResult::kind`)
- `normalize_terms()` was not called in discover pipeline (noise filtering was skipped)

[Unreleased]: https://github.com/smapira/kaseifu/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/smapira/kaseifu/releases/tag/v0.1.0
