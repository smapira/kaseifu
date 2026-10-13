# Kaseifu — Semantic File Organizer

![Kaseifu hero](assets/hero.png)

A macOS-first semantic file organizer written in Rust.

The goal is to answer:

> Where does this file belong?

using the context already available on the user's Mac.

## How it works

kaseifu observes a file, discovers candidate destinations from your existing directory structure and Spotlight index, asks a semantic classifier (Jev) which destination fits best, and — if confidence is high enough — moves the file. Every move is journaled and undoable.

### Pipeline

```text
File
 ↓
Context Builder          (metadata, xattr, Spotlight)
 ↓
Normalize                (filter noise, unify representations)
 ↓
Candidate Discovery      (directory topology, Spotlight search, similar files)
 ↓
Jev                      (semantic classification over validated candidates)
 ↓
Policy Engine            (auto-move vs. review vs. refuse)
 ↓
Transactional Move       (atomic rename + SQLite journal)
 ↓
Undo                     (restore from journal)
```

### Signals

| Signal | Source |
|---|---|
| Filesystem metadata | `std::fs`, `read_dir` |
| Directory topology | parent/child directory analysis |
| Spotlight metadata | `mdls` attributes |
| Spotlight search | `mdfind` index queries |
| Extended attributes | macOS xattr (`com.apple.FinderInfo`, etc.) |
| Similar existing files | fingerprinting + candidate scoring |
| Content extraction | optional content analysis |
| Semantic classification | Jev API |

### Trust boundary

Jev never receives unrestricted filesystem control. The local Rust application:

1. Collects evidence from the filesystem and Spotlight.
2. Discovers existing directories and generates candidate destination IDs.
3. Validates candidate paths.

Jev only chooses among those validated IDs. The Rust process owns all filesystem mutations.

## Usage

### Install

```bash
cargo install kaseifu
```

Or build from source:

```bash
git clone https://github.com/smapira/kaseifu.git
cd kaseifu
cargo install --path .
```

### Quick Start

Run the setup wizard once:

```bash
kaseifu setup
```

Classify a file:

```bash
kaseifu classify ~/Desktop/example.pdf
```

No environment variables or `.env` file are required.

### Inspect a file

Print the `FileContext` JSON that forms the basis of candidate discovery and classification.

```bash
kaseifu inspect ~/Desktop/example.pdf
```

### Classify (dry run)

Run the full decision pipeline — observe, normalize, discover, Jev — and print the decision without moving anything.

```bash
kaseifu classify ~/Desktop/example.pdf
```

### Organize (dry run by default)

Run the full pipeline and print what *would* happen.

```bash
kaseifu organize ~/Desktop/example.pdf
```

Pass `--execute` to actually move the file:

```bash
kaseifu organize ~/Desktop/example.pdf --execute
```

### Undo

Restore the most recent move:

```bash
kaseifu undo
```

Restore the last N moves:

```bash
kaseifu undo --last 3
```

### Watch

Watch directories and automatically organize new or changed files. Dry run by default.

```bash
kaseifu watch --path ~/Downloads --path ~/Desktop
```

Pass `--execute` to actually move files:

```bash
kaseifu watch --path ~/Downloads --execute
```

Events are filtered, debounced, deduplicated, and stability-checked before the pipeline runs.

### Setup

Interactive first-time configuration. General settings go to
`~/Library/Application Support/kaseifu/config.toml`, the API key goes to
the macOS Keychain (never to a file, never to shell history):

```bash
kaseifu setup
```

In `env` mode the Keychain step is skipped and setup points at `.env`
instead (see "API key source" below).

### Auth

Manage the stored API key:

```bash
kaseifu auth set-key      # read the key interactively, store it in the Keychain
kaseifu auth status       # show whether a key is stored (never the key itself)
kaseifu auth remove-key   # delete the stored key (asks first)
```

`auth status` also reports where the key resolved (`Source: env` / `.env` /
`keychain` / `not configured`) and the effective `KASEIFU_KEY_SOURCE` mode.
The same source appears in `doctor` as the `API Key` check. In `env` mode
these commands are fast and never trigger a Keychain approval dialog,
because the Keychain is not consulted at all in that mode.

### Config

Show or update general settings (the API key cannot be set this way):

```bash
kaseifu config show
kaseifu config set jev.model jev-latest
kaseifu config set jev.endpoint https://api.typesafe.ai/v1/systemone
```

### Doctor

Diagnose configuration and environment without touching the network:

```bash
kaseifu doctor
```

Add `--check-connection` to also probe endpoint reachability. The
`API Key` check reports the resolved source
(`provided via environment (JEV_API_KEY)` / `provided via .env` /
`resolved from macOS Keychain` / a clear failure when nothing is configured).

### Debug

Enable debug output with the `KASEIFU_DEBUG` environment variable:

```bash
KASEIFU_DEBUG=1 kaseifu classify ~/Desktop/example.pdf
```

### Shell completion

Print the zsh completion script (stdout only — nothing is written):

```bash
kaseifu completions zsh
```

Install it into the first writable completion directory:

```bash
kaseifu completions zsh --install
```

The target directory is detected in this order; every candidate must already
exist and be writable:

1. `--dir <PATH>`
2. `KASEIFU_COMPLETION_DIR`
3. `/opt/homebrew/share/zsh/site-functions`
4. `/usr/local/share/zsh/site-functions`
5. `~/.zprezto/modules/completion/external/src`
6. `${XDG_DATA_HOME:-~/.local/share}/zsh/site-functions`
7. `~/.zsh/completions`

No `.zshrc` change is required: the Homebrew and Prezto directories are
already on `fpath` on those setups, and the file is named `_kaseifu` so
`compinit` picks it up. `--dir` and `KASEIFU_COMPLETION_DIR` are used as
given: when they cannot be written to, the command fails instead of silently
falling back. `--install` supports zsh only.

After upgrading kaseifu, the installed script is regenerated automatically the
next time any command runs when the file is missing or still carries an older
version marker. Every failure (no directory, no permission, concurrent write)
is ignored so the command you ran is not affected, and `KASEIFU_NO_COMPLETION_UPDATE`
(non-empty) turns the automatic update off. `kaseifu doctor` reports the state
as `installed`, `stale`, or `missing` together with the directory it refers to.

Verify a generated script before installing it:

```bash
kaseifu completions zsh > /tmp/_kaseifu
zsh -n /tmp/_kaseifu
```

Other shells (`bash`, `fish`, `elvish`, `powershell`) can be printed as well.
Only the zsh script is installed, and only the zsh script hides the
debug-only `discover` command: `clap_complete` renders hidden subcommands in
every shell it supports (upstream
[clap-rs/clap#1335](https://github.com/clap-rs/clap/issues/1335)), so kaseifu
removes them from the zsh output after generation.

## Configuration

Settings resolve in this order: CLI flags (`--jev-endpoint`, `--jev-model`)
win over environment variables, which win over the user configuration file
plus Keychain, which win over application defaults. The legacy
`JEV_API_BASE_URL` variable still works as a fallback for `JEV_API_URL`.

### API key source (`KASEIFU_KEY_SOURCE`)

The API key is resolved from exactly one place per invocation:

| `KASEIFU_KEY_SOURCE` | Key comes from | Notes |
|---|---|---|
| `auto` | `JEV_API_KEY` (environment or `.env`) first, then the macOS Keychain | Default for release builds |
| `env` | `JEV_API_KEY` (environment or `.env`) only | Default for debug builds; the Keychain is never read or written |
| `keychain` | the macOS Keychain only | `JEV_API_KEY` is ignored; for verifying the Keychain path |

The variable is read from the process environment first, then from the
project-local `.env`; unknown values are rejected with an error. In `env`
mode `setup`, `auth set-key` and `auth remove-key` do not touch the
Keychain and instead explain that development uses `JEV_API_KEY` in `.env`
or the environment (register the Keychain entry with the release-build
kaseifu, or with `KASEIFU_KEY_SOURCE=auto kaseifu setup`). Commands that
call Jev (`classify`, `organize`, `watch`) fail fast with a clear
"no API key configured" error when an explicit mode resolves no key,
instead of attempting an unauthenticated request. `auto` keeps the
historical behavior.

| Setting | Default | Description |

| Setting | Default | Description |
|---|---|---|
| `jev.endpoint` | `https://api.typesafe.ai/v1/systemone` | Jev API endpoint (base or full URL) |
| `jev.model` | `jev-latest` | Model alias sent to the API |
| `classifier.thresholds.auto_move_confidence` | 0.85 | Minimum confidence for automatic moves |
| `classifier.thresholds.auto_move_margin` | 0.40 | Minimum margin over second-best candidate |
| `classifier.thresholds.review_confidence` | 0.60 | Below this, the file is left in place |
| `operations.dry_run` | `true` | Default to dry run unless `--execute` is passed |
| `operations.enable_undo` | `true` | Record moves in the journal so they can be undone |

Developer setup (`.env` files, environment variables, local builds, tests,
CI, API mocks, resolution order) lives in `documents/development.md`.

## Project structure

```
src/
├── main.rs              # CLI entry point (clap)
├── lib.rs               # Library root
├── auth/                # Keychain access + interactive prompts (v3.3)
├── completion/          # zsh completion generation + installation
├── config/              # User TOML + resolution order (v3.3)
├── doctor.rs            # Environment diagnostics (v3.3)
├── context/             # FileContext builder (Phase 1)
├── normalize/           # Normalization layer (Phase 2)
├── discovery/           # Candidate discovery (Phase 3)
├── classifier/          # Jev integration + pipeline (Phase 4)
├── models/              # Shared data types
├── operations/          # Organize, plan, move, undo (Phase 5)
├── policy/              # Auto/review/refuse decisions
├── platform/            # macOS-specific code (FSEvents)
├── storage/             # SQLite history + cache
└── watch/               # FSEvents watcher + coordinator (Phase 6)

config/
└── default.toml         # Runtime configuration

schemas/
├── file-context-v1.json
├── candidate-destination-v1.json
└── jev-decision-v1.json

docs/
├── ARCHITECTURE.md
├── JEV.md
├── MACOS.md
└── design/              # Per-phase design documents
```

## Safety

- **Dry run by default.** Nothing moves unless you pass `--execute`.
- **Validated candidates only.** Jev chooses from directories the local app discovered — it cannot invent destinations.
- **Journaled moves.** Every executed move is recorded in a SQLite database.
- **Undo.** Reverse any move with `kaseifu undo`.
- **Collision detection.** If a file already exists at the destination, the move is refused and flagged for review.

## Development stages

All six phases are implemented:

| Phase | Status | Description |
|---|---|---|
| 1 — Observe | ✅ | FileContext generation from filesystem + Spotlight |
| 2 — Normalize | ✅ | Noise filtering, representation unification |
| 3 — Discover | ✅ | Spotlight-based candidate discovery |
| 4 — Jev | ✅ | Semantic classification with validated candidates |
| 5 — Execute | ✅ | Transactional move, journal, undo |
| 6 — Automate | ✅ | FSEvents watcher with debounce, dedup, stability check |

## Code quality

One script runs every quality check, locally and in CI
(`.github/workflows/rust-quality.yml`, macOS runner):

```bash
bash scripts/quality_check.sh
```

| Check | Command | CI gate |
|---|---|---|
| Formatting | `cargo fmt --all -- --check` | 1+ violation fails |
| Lint | `cargo clippy --all-targets --all-features -- -D warnings` | 1+ warning fails |
| Tests | `cargo test --all-targets --all-features` | 1 failure fails |
| Complexity | `rust-code-analysis-cli 0.0.25` + `scripts/complexity_report.py` | new/changed fn CC 16+ fails |
| Coverage | `cargo-llvm-cov 0.9.1 --summary-only` | report-only (current line coverage ~71%) |

Complexity analyzer setup (pinned; `--locked` is required):

```bash
cargo install rust-code-analysis-cli --version 0.0.25 --locked
cargo install cargo-llvm-cov --version 0.9.1 --locked
```

Quality policy (project-local thresholds, not official Rust limits):

- CC 1–10 Normal, 11–15 Review (warning), 16+ Refactor.
- Existing high-CC functions are tracked in `reports/quality/baseline.json`
  and never fail CI by themselves; only **new** functions with CC 16+ or
  **worsening** to CC 16+ fail.
- Cognitive complexity, function length, and coverage are
  measured as warnings/reports only (staged rollout).
- Nesting depth is not measurable with the adopted analyzer
  (its metric set has no depth output), so no depth gate is enforced.
- Unmeasurable functions are excluded from statistics — never counted as CC 0.
- No live Jev API key is needed for quality checks; tests use mocks.

Refreshing the reviewed baseline after intentional refactoring:

```bash
bash scripts/quality_check.sh
python3 scripts/complexity_report.py baseline \
  --current reports/quality/complexity.json \
  --out reports/quality/baseline.json
```

Reports land in `reports/quality/` (git-ignored except `baseline.json`)
and are uploaded as CI artifacts.

## License

MIT

---

*Hero artwork: [aahub.org](https://aahub.org)*
