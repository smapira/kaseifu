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

### Inspect a file

Print the `FileContext` JSON that forms the basis of candidate discovery and classification.

```bash
kaseifu inspect ~/Desktop/example.pdf
```

### Discover candidates

Search for similar files and list scored candidate destinations.

```bash
kaseifu discover ~/Desktop/example.pdf
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

### Debug

Enable debug output with the `KASEIFU_DEBUG` environment variable:

```bash
KASEIFU_DEBUG=1 kaseifu discover ~/Desktop/example.pdf
```

## Configuration

Copy `.env.example` to `.env` and fill in your Jev credentials:

```bash
cp .env.example .env
```

```
JEV_API_KEY=your-key-here
JEV_API_URL=https://your-jev-endpoint.example.com
```

Thresholds and behavior can be tuned in `config/default.toml`:

| Setting | Default | Description |
|---|---|---|
| `classifier.thresholds.auto_move_confidence` | 0.85 | Minimum confidence for automatic moves |
| `classifier.thresholds.auto_move_margin` | 0.40 | Minimum margin over second-best candidate |
| `classifier.thresholds.review_confidence` | 0.60 | Below this, the file is left in place |
| `operations.dry_run` | `true` | Default to dry run unless `--execute` is passed |

## Project structure

```
src/
├── main.rs              # CLI entry point (clap)
├── lib.rs               # Library root
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
