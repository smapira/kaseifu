# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.2] - 2026-10-14

### Fixed

- **Discovery candidate quality for images** — image targets now search with
  the Spotlight content type scope (`public.image`) and file-name terms inside
  the mdfind query itself, so photos and photo folders reach the candidate
  ranking instead of being crowded out by unrelated file types; the result
  window is applied after aggregation and ranking instead of truncating
  mdfind's raw hit order (non-image queries are unchanged)
- **Unusable destinations are never offered** — discovery drops directories
  that cannot be a move destination as a rule of path shape rather than a
  list of paths: app-managed containers (`*.library`, `*.app`,
  `*.photoslibrary`, …), the macOS library trees (`/Library`,
  `~/Library`, `/Volumes/<volume>/Library`), hidden destinations, cache and
  OS-reserved areas (`Caches`, `TemporaryItems`, `*.cache`, `.Trash`), and
  machine-generated folder names (UUIDs, hashes, Eagle-style `<hash>.info`
  serials). On a real photo this removes the 38 app-store folders that filled
  the candidate list and scores 17% faster by not scoring their files;
  user folders (`~/Pictures`, `~/Documents`, external volumes) are untouched.
  All-uppercase separator-free names (`PHOTOS2024BACKUP`) count as generated
  and are not offered either; a scan of 171,428 real folders found no such
  hand-made name

### Changed

- **`.env` is parsed with `dotenvy`** instead of a hand-rolled parser, so the
  developer/CI fallback honours `export`, quotes and trailing comments the way
  the format defines; environment variables still win over `.env`, and `.env`
  still wins over the Keychain (nothing is written into the process
  environment). The `discover` command is now hidden from `--help`: it stays
  available as a diagnostic view, while `classify` remains the supported path

## [0.4.1] - 2026-10-14

### Fixed

- **Discovery search terms** — image availability markers
  (`M:Capture date available` / `M:GPS available` / `M:Camera available`) no
  longer leak into mdfind queries; the Jev adapter derives them from
  `image_metadata` at request time instead of storing them as semantic
  signals. The Jev wire output is unchanged for every privacy combination;
  Rich `inspect` no longer repeats the markers next to the capture/location
  panels

## [0.4.0] - 2026-10-14

### Added

- **Image metadata observation** — capture date/time, GPS location, camera make/model, and pixel dimensions via Spotlight (single `mdls` call) with EXIF fallback for JPEG/TIFF (best-effort for HEIC); no new dependencies
- **Context-aware Jev requests** — `state.target` carries `capture_date` / `camera` / `dimensions` and `location` (only when explicitly allowed), plus availability signals in `semantic_signals`; no Rust-side scoring of dates or coordinates
- **Image privacy controls** — `[metadata.image]` / `[metadata.privacy]` settings (`send_gps_to_jev = false` by default), unknown-key rejection, no position persistence or log leakage
- **`inspect` capture sections** — CAPTURE METADATA and LOCATION panels (ASCII, `-` for missing values), JSON-compatible (`image_metadata` omitted for non-images)

## [0.3.0] - 2026-10-13

### Added

- **First-run setup** — `kaseifu setup` interactively stores the Jev API key in the macOS Keychain and general settings in `~/Library/Application Support/kaseifu/config.toml`; no `.env` or `export` needed
- **`kaseifu auth`** — `set-key` / `status` / `remove-key` for Keychain-backed API key management (never shown, never logged, never passed as a CLI argument)
- **`kaseifu config`** — `show` / `set` for validated TOML settings with atomic saves and unknown-key rejection
- **`kaseifu doctor`** — local environment diagnostics (platform, config, Keychain state, endpoint form, Spotlight, storage) with opt-in connectivity check
- **Setting resolution order** — CLI flags > environment variables (`JEV_API_URL` > legacy `JEV_API_BASE_URL`, `JEV_MODEL`, `JEV_API_KEY`) > user TOML + Keychain > application defaults
- **Quality gates** — `scripts/quality_check.sh` (rustfmt, Clippy `-D warnings`, tests, complexity, baseline comparison) wired into `.github/workflows/rust-quality.yml` (macOS runner) with `reports/quality/baseline.json` tracking
- **Developer docs** — `documents/development.md` (env files, mocks, resolution order, CI)

### Changed

- **Endpoint no longer required upfront** — missing endpoint/model resolve to defaults instead of erroring (`JEV_API_URL is not set` is gone; `setup` is the recommended path)
- **Complexity refactor** — max cyclomatic complexity 44 -> 12, CC 16+ functions 3 -> 0, CC<=10 ratio 96.6% -> 99.6%; behavior-preserving splits only
- **README** — Quick Start rewritten around `kaseifu setup`; `.env` instructions moved to developer docs

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
