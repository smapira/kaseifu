# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-10-12

### Changed

- **Discover scoring (breaking scale change)** — candidate `score` is now 0..=1 (was 10..16). File score = `0.60 x filename_similarity + 0.25 x token_similarity + 0.15 x extension_match`; directory score = `0.85 x max + 0.15 x mean(top 3)`. Count alone no longer ranks first
- **Candidate IDs** — assigned after score sorting, so D1 is always the top candidate
- **`matched_terms`** — now contains only actually-matched search terms (was the full file path)
- **`similarity_score`** — now a computed value (was a fixed 1.0)
- **Target parent exclusion** — the file's own parent directory is never offered as a move destination
- **ASCII-only dashboard chrome** — CJK-font terminals render box-drawing (`─`) and block (`█`) glyphs double-width, scattering right borders (measured pixel-exact on-device). Frame, dividers (`|-- SECTION --|`), bars (`#`/`.`), truncation (`...`), and markers are now pure ASCII; colors and layout unchanged
- **Smarter path shortening** — a leading `~`/`.`/root is dropped instead of kept (`.../parent/last`), so shortened paths stay distinguishable
- **Ranked footer** — notes `(incl. NONE)` when the Jev distribution contains `NONE`

### Added

- **Rich CLI Cyber Dashboard** — self-drawn ANSI-aware panels, candidate list with relative score bars, `--format auto|rich|json` (global flag), `discover --all` / `--verbose`, `NO_COLOR` / `TERM=dumb` and non-TTY fallbacks
- **Score breakdown** — per-file filename/token/extension similarity via `KASEIFU_DEBUG=1` and `breakdown_line()`
- **Directory age adjustment** — evidence mtimes feed a capped bonus (`+0.10`, linear over 365 days, 1-day floor) and ties now break older-first; `established: ~N days` reasons, `SimilarFile.modified_at` (optional, JSON-compatible)
- **Classify Rich dashboard** — Jev ranking (descending raw probabilities, top 5 + `--all`, `NONE` support, out-of-top-5 selection always shown), separate JEV DECISION and Rust POLICY verdict sections; `classify --all` / `--verbose`; decision diagnostics moved to stderr behind `--verbose`
- **Inspect Rich dashboard** — three layers (FILE summary, SEMANTIC SIGNALS, DIRECTORY CONTEXT + `--verbose` DETAILS), nearby entries capped at 5 + `--all`, `ls -p` style markers, KiB sizes, short datetimes; JSON output unchanged
- **Panel section dividers** (`|-- SECTION --|`) with ANSI-aware width alignment

### Removed

- **`rich-rs` dependency** — evaluated during selection, then removed after its last usage was replaced by the self-drawn panel; one less third-party dependency

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

[Unreleased]: https://github.com/smapira/kaseifu/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/smapira/kaseifu/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/smapira/kaseifu/releases/tag/v0.1.0
