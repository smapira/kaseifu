# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Interrupted-operation detection (`doctor`, `undo`, journal API)** — a move
  that is interrupted between the filesystem rename and its journal update
  leaves a `pending` row; the journal now makes those rows visible instead of
  requiring direct SQL: `doctor` prints the resolved journal path, the
  recorded operation count and every pending row with its age and the current
  state of its source/destination file (`FAIL` only when a pending row is old
  enough to be an interruption rather than an in-flight operation), and
  `undo` prints a warning with the number of pending rows before restoring.
  Nothing is repaired, rewritten or deleted automatically — the journal alone
  cannot tell an interrupted move from one still in flight, so recovery stays
  a human decision (this task deliberately implements detection only, per the
  debug-scenario design S05). The inspection is read-only (SQLite read-only
  connection): `doctor` never creates or modifies a journal.

### Changed

- **Journal location is now stable** — the default journal moved from the
  CWD-relative `data/kaseifu.db` (which silently created a different journal
  per working directory, so `undo` could not see moves recorded elsewhere) to
  the absolute `~/Library/Application Support/kaseifu/kaseifu.db`, next to
  the user configuration. `doctor` reports the resolved path. The old
  CWD-relative `./data/kaseifu.db` is not migrated, merged or deleted:
  `doctor`'s `Legacy Journal` check points it out (`FAIL`) and the file is
  left exactly as it is.
- **System-directory exclusion is component-based** — discovery used to drop
  any hit whose path merely *contained* `/system`, `/private`, `/usr`,
  `/bin`, `/sbin`, `/var` or `/tmp`, so user folders such as `~/var/photos`,
  `~/tmp/mail`, `~/system-backup` or `~/private-docs` never reached the
  candidate list. The macOS system trees are now matched as the **first path
  component** of absolute paths (`/System/...`, `/usr/...`,
  `/private/var/...`, `/var/...`, `/tmp/...`), the rule lives only in
  `discovery::destinations` (the single authority shared by the hit filter
  and the aggregation gate), and user folders that reuse a system name stay
  candidates.
- **Configuration honesty** — `config/default.toml` no longer lists the
  `[scanner]`, `[context]` and `[storage]` sections (nothing ever read them;
  the `[storage] database` path named a file the journal never used), and it
  now states that it documents the built-in defaults without being read at
  runtime. `operations.dry_run` / `operations.enable_undo` are still accepted
  in user TOML files (existing files must keep loading) but are inactive:
  `config show` marks both keys as such (`inactive_settings` in
  `--format json`) and the README table says so. Wiring `dry_run` as a safety
  interlock was considered and rejected — its default `true` would stop every
  `--execute` move. `config set` validation (unknown keys, API keys) is
  unchanged.
## [0.5.0] - 2026-10-13

### Added

- **API key source selection (`KASEIFU_KEY_SOURCE`)** — the API key now comes
  from exactly one source per invocation: `auto` (environment/`.env` first,
  then the macOS Keychain; release default), `env` (environment/`.env` only —
  the Keychain is never read or written; debug default) or `keychain` (the
  Keychain only; `JEV_API_KEY` is ignored). The variable is read from the
  process environment first, then from the project-local `.env`; unknown
  values are rejected with a clear error. `auth status` and `doctor` report
  the resolved source (`env` / `.env` / `keychain` / `not configured`) and
  the effective mode, and `doctor`'s `Keychain` check becomes `API Key`
  (seven checks stay). In `env` mode `setup`, `auth set-key` and
  `auth remove-key` never touch the Keychain and explain the development
  policy instead; `classify` / `organize` / `watch` fail fast with a clear
  "no API key configured" error when an explicit mode resolves no key
  (`auto` keeps the historical behavior).
- **zsh completion generation and installation** — `kaseifu completions zsh`
  prints the completion script and `completions zsh --install` writes
  `_kaseifu` into the first writable site-functions directory (Homebrew,
  `/usr/local`, Prezto, XDG or `~/.zsh/completions`; `--dir` and
  `KASEIFU_COMPLETION_DIR` take precedence, no `.zshrc` change is needed).
  The script carries a version marker and is regenerated automatically — best
  effort only, so a failure never changes the exit code or the output — when
  any command runs and the installed file is missing or stale;
  `KASEIFU_NO_COMPLETION_UPDATE` disables that update. `doctor` now reports
  the completion state (`installed` / `stale` / `missing`) with its directory.
  `clap_complete` renders hidden subcommands in every shell it supports
  (clap-rs/clap#1335), so the debug-only `discover` command is removed from
  the generated zsh script explicitly; other shells print whatever
  `clap_complete` produces.

### Changed

- **Debug builds no longer consult the macOS Keychain by default** — because
  `KASEIFU_KEY_SOURCE` defaults to `env` for debug builds, `cargo run` and
  tests resolve the key from the environment/`.env` only. This removes the
  macOS approval dialog (and the indefinite hang it caused) when the Keychain
  entry was created by a different binary than the one reading it; release
  builds default to `auto` and keep the Keychain behavior unchanged. The
  resolver performs at most one Keychain read per invocation and diagnostics
  never re-read it; `auth status` prints `Source:` instead of `Storage:`
  (`--format json` keeps the existing `storage` key and adds
  `api_key_source`, `key_source_mode`, `key_source_default`).
- **`organize` output** — the Rich view is now the same ASCII dashboard as
  `classify`/`inspect`/`discover`, in the order mode -> decision -> policy ->
  result (the mode line no longer splits the decision information). The Jev
  selection shows the directory the chosen candidate resolved to (or
  `no destination` for `NONE`) instead of only an ID, and it stays visible
  even when the policy leaves the file in place — it is a considered
  destination, never an execution permission. The mode banner reads
  `execute mode` / `dry run (no files moved)` instead of `executed`, which
  claimed a move the policy had refused; only the result section states what
  actually happened. Refused plans render as human sentences (`policy
  refused: automatic move not allowed`, `destination already exists: ...`,
  `unknown candidate selected by the decision: ...`) instead of Rust enum
  debug output (`NotAutoMove`), and the policy verdict reuses `classify`'s
  wording. Plain (non-TTY) output keeps the same sections without ANSI.

## [0.4.2] - 2026-10-13

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

## [0.4.1] - 2026-10-13

### Fixed

- **Discovery search terms** — image availability markers
  (`M:Capture date available` / `M:GPS available` / `M:Camera available`) no
  longer leak into mdfind queries; the Jev adapter derives them from
  `image_metadata` at request time instead of storing them as semantic
  signals. The Jev wire output is unchanged for every privacy combination;
  Rich `inspect` no longer repeats the markers next to the capture/location
  panels

## [0.4.0] - 2026-10-13

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
