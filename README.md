# Kaseifu — Semantic File Organizer

![Kaseifu hero](assets/hero.png)

A macOS-first semantic file organizer written in Rust.

The goal is to answer:

> Where does this file belong?

using the context already available on the user's Mac.

## Quick Start

Install, then run one command:

```bash
cargo install kaseifu
kaseifu classify ~/Desktop/example.pdf
```

`classify` decides where the file belongs and moves nothing. When no API key
is configured yet and stdin is a terminal, kaseifu asks for it with hidden
input and stores it where `KASEIFU_KEY_SOURCE` keeps keys (the macOS Keychain
by default, `.env` in `env` mode). No environment variables or `.env` file are
required; without a terminal (scripts, CI) a missing key fails immediately
(non-zero exit) and names the commands that need no key (`kaseifu inspect`,
`kaseifu doctor`) plus `kaseifu setup`.

```bash
kaseifu                                            # a bare run prints a short guide and does nothing else
kaseifu doctor                                     # check configuration, API key, Spotlight, journal, completions
kaseifu setup                                      # the setup wizard does the same configuration up front
kaseifu organize ~/Desktop/example.pdf --execute   # move a file (without --execute it is a dry run)
```

Building from source instead of installing from crates.io:

```bash
git clone https://github.com/smapira/kaseifu.git
cd kaseifu
cargo install --path .
```

Everything else is below: [How it works](#how-it-works) explains the
pipeline, [Usage](#usage) lists every command with its options, and
[Configuration](#configuration) covers the settings.

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

### What kaseifu classifies

Today's supported target is image files: `.jpg`, `.jpeg`, `.png`, `.heic`,
`.heif`, `.hif`, `.tif`, `.tiff` and `.webp`. Each of them is observed like
any other file, and images additionally get capture time, camera and (when
allowed) location handling. Everything else is matched by name and Spotlight
metadata only.

Video files (MP4, MOV, and other video containers) are not classified yet:
their capture time and GPS are neither read nor sent to Jev, so a video may
look untouched even when it sits next to a well organized photo series.
Whether to support video is a separate decision, not a hidden setting. (The
`rename` command does rename videos: see its section below.)

### Spotlight indexing

Discovery uses the macOS Spotlight index, which macOS updates in the
background, so results depend on its current state:

- Spotlight indexing is asynchronous, so a file that was just created, copied
  or moved may not appear in `mdfind` (the search Discovery uses) yet: about
  4 s measured on an idle machine, and later under load or after bulk copies.
  A very recent file may not be found at all.
- Attributes come from per-type importers. Names and sizes are immediate, but
  content-derived values (EXIF capture time, GPS, camera, pixel size) can be
  missing for a given file. Missing means missing: kaseifu never guesses, and
  reads EXIF from the file as a fallback for images.
- Search does not return hidden paths (any `.`-prefixed path component, such
  as a `.delta/` worktree) or excluded areas (`/tmp`, the Spotlight privacy
  list). `mdls` can still print values for those files, so "`mdls` shows it"
  does not mean "search finds it".

If a run finds no candidates, the file is probably not indexed yet: run
`KASEIFU_DEBUG=1 kaseifu classify <file>` to see the search terms and hit
count, wait for the index, or force an import with `mdimport <file>`.

### Trust boundary

Jev never receives unrestricted filesystem control. The local Rust application:

1. Collects evidence from the filesystem and Spotlight.
2. Discovers existing directories and generates candidate destination IDs.
3. Validates candidate paths.

Jev only chooses among those validated IDs. The Rust process owns all filesystem mutations.

## Usage

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

`--explain` adds the numbers behind the verdict: the confidence, the margin
and the effective thresholds, plus the destination the selection resolved to.
When a decision lands just below the auto-move thresholds, the display also
says how much more confidence or margin a move would need.

### Organize (dry run by default)

Run the full pipeline and print what *would* happen.

```bash
kaseifu organize ~/Desktop/example.pdf
```

Pass `--execute` to actually move the file:

```bash
kaseifu organize ~/Desktop/example.pdf --execute
```

`--explain` adds the same threshold view as `classify`, and `--format json`
prints the result as one structured object (`decision`, `policy`,
`candidate_path`, `mode` and `outcome`); the outcome says whether the file
`would_move`, `moved`, was `refused` or could not be planned, with a machine
`kind` and a human `reason`. `--explain` adds an `explanation` object.

### Choose the destination yourself

Jev's choice is a suggestion; `--select` lets you pick from the candidate
folders Discovery found instead:

```bash
kaseifu organize ~/Desktop/example.pdf --select
kaseifu organize ~/Desktop/example.pdf --select --execute --yes
```

The menu lists each candidate with its score and related-file count, and asks
for a number. Jev is not consulted in this flow, so it works without an API
key, without the Keychain and without the network. `q` (or Ctrl+C) cancels
with nothing changed. `--execute` performs the move after a confirmation
(`--yes` skips it), and the move is recorded as a human decision: the plan is
marked `[MANUAL]`, the journal keeps no classifier confidence for it, and
`undo` reverses it like any other move. Destinations that can never be used
(`~/Library/**`, caches, system trees) are not offered and are refused by the
plan even if one arrives from somewhere else. Without a terminal, `--select`
fails with guidance; `--select --format json` prints the candidate list
instead of prompting.

### Rename a file or folder (AI naming)

Propose a content-based name, and apply it on request:

```bash
kaseifu rename ~/Desktop/report.pdf            # dry run: print the proposal
kaseifu rename ~/Desktop/report.pdf --execute  # rename it (journaled, undoable)
kaseifu rename ~/Documents/untitled-folder     # a folder, named from its contents
```

The name comes from the rule you pick, and the rule's variables come from the
file itself: the on-device model supplies the title and category, the file
supplies size, extension, parent directory and dates, and images and videos
also get scene labels from on-device Vision (images directly, videos through a
few representative frames). Rules are the built-in presets `P01`-`P12` or your
own template:

```bash
kaseifu rename ~/Desktop/report.pdf --patterns
kaseifu rename ~/Desktop/report.pdf --pattern date-category-title
kaseifu rename ~/Desktop/report.pdf --template "{date}_{title}"
kaseifu rename ~/Desktop/report.pdf --pattern prefix-title --prefix WORK
```

Presets are stored in the same SQLite database as the journal, so no new
configuration file appears:

```bash
kaseifu rename --save-preset work --pattern prefix-title --prefix WORK
kaseifu rename ~/Desktop/report.pdf --preset work
kaseifu rename --set-default work     # later renames need no flags
kaseifu rename --patterns             # builtins plus stored presets
kaseifu rename --delete-preset work
```

Behaviour worth knowing:

- **Documents, images, videos and folders.** Documents are txt, md, pdf and
  html; images are jpg, jpeg, png, heic, heif, tif, tiff and webp; videos are
  mp4, mov, m4v, 3gp and 3g2; folders are any directory kaseifu may rename.
  Other types are refused with the reason.
- **Folders are named from what is inside them.** kaseifu reads the folder's own
  children (names, file types, and the bounded text of up to three document
  children) and names the collection from that: `新しいフォルダ/` with invoices,
  a contract and a payment sheet becomes `請求・契約関連書類/`. The folder's
  current name is the string being replaced, so it is not sent to the model as
  evidence. The read is bounded: at most 200 entries are read, at most 12 file
  names and 8 folder names reach the model, and the reading never recurses into
  a child folder (a grandchild is never opened). An empty folder (or one with
  nothing but system files) and a folder whose contents share no theme are
  reported as `status: held` with the reason instead of being renamed after a
  guess.
- **Folders kaseifu refuses to touch.** The volume root, the system trees
  themselves (`/System`, `/usr`, `/tmp`, ...), the home directory, the working
  directory (and their ancestors), app bundles (`Foo.app`,
  `Photos.photoslibrary`), anything inside the macOS Library tree, OS areas
  (trash, Spotlight index), cache and temporary areas, and hidden folders.
  A folder *inside* a system tree stays renameable, so a scratch folder under
  `/tmp` or `$TMPDIR` works.
- **Images are described on device.** Apple Vision labels the scene (`outdoor`,
  `moon`, ...) and the model turns those into the title, so `IMG_1234.png` can
  become `2026-10-14_夜空の月.png`. The image is opened read-only and never
  modified.
- **Videos are sampled, not decoded.** AVFoundation reads the container's own
  metadata (duration, resolution, frame rate, recording date) and extracts
  3-5 representative frames, chosen by length (10% to 90% of the clip); each
  frame is classified with the same Vision recipe and the labels are merged.
  The video is opened read-only, is never re-encoded and no frame is written
  to disk. Videos whose frames cannot be perceived (unreadable containers,
  audio-only files) are reported as `status: held` with the reason instead of
  being renamed after a guess.
- **Dates.** Images prefer the EXIF capture time (the Spotlight EXIF value,
  then the EXIF inside the file); videos prefer the container's own recording
  date; documents prefer the creation time, then the modification time, then a
  date found in the text; folders use their own creation and modification times
  (a folder has neither a capture time nor a text body). `--date-format` picks
  the rendering, and the display reports `date_source` so you can see which one
  was used (`now` means the last resort was used, and is reported as such).
- **Missing values hold the proposal back.** A rule whose required variable is
  empty, a name collision, a dangerous name (`/`, `..`, an extension change) or
  a document that looks like a prompt-injection payload is reported as
  `status: held` with a reason; nothing is silently repaired or invented.
- **Names stay shell friendly.** The stem is cut to 80 characters at a word
  boundary, whitespace runs become `_`, and the extension never changes.
- **Journal and undo.** An executed rename is one journal row, and
  `kaseifu undo` reverses it exactly like an organize move. That includes a
  folder rename (the folder and its children move back under the old name) and
  the case where a folder was renamed around child files: undo always reverses
  the newest operation first, so a parent folder is renamed back before any row
  recorded under its old name is reached, and those rows resolve again. kaseifu
  never rewrites journal rows; when a recorded path cannot be found and no row
  explains where it went (a rename done by another tool, or an interrupted
  operation), the undo refuses with that path and changes nothing.
- **No model, no rename.** Without Apple Intelligence the command exits with
  `model unavailable: ...`; it never falls back to another model.
- `--format json` (with or without `--execute`) prints one object: `target`,
  `current_name`, `pattern`, `proposal`, `date_source`, `status`, `reason`,
  `operation_id`, `perception` for images and videos (the labels, or why they
  are missing), `video` for videos (the measured metadata and the frame count),
  and `directory` for folders (the entry counts, whether the read hit its cap,
  and the file type distribution).

### Undo

Restore the most recent move:

```bash
kaseifu undo
```

Restore the last N moves:

```bash
kaseifu undo --last 3
```

The journal lives at `~/Library/Application Support/kaseifu/kaseifu.db`
(absolute, independent of the working directory; `doctor` prints the resolved
path). If a move was interrupted before its result could be written — for
example killed between the rename and the journal update — the journal keeps
a `pending` row: `undo` reports it before restoring, and `doctor` shows the
count with the age and the current location of each pending file. Pending
rows are never repaired, deleted or undone automatically: the journal cannot
tell an interrupted move apart from one still in flight, so only you can
judge the on-disk state. The old CWD-relative journal (`./data/kaseifu.db`,
used by earlier builds) is not migrated or deleted; `doctor` reports it when
it is still there.

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

Add `--check-connection` to also probe endpoint reachability. The first line
is the verdict (`Ready: yes`, or `Ready: no (N check(s) failed: ...)`), and
every `FAIL` row names the next action, e.g. `(next: kaseifu setup)` or
`(next: kaseifu completions zsh --install)`, so a failure is never a dead
end. The `API Key` check reports the resolved source
(`provided via environment (JEV_API_KEY)` / `provided via .env` /
`resolved from macOS Keychain` / a clear failure when nothing is configured).
The `Journal` check prints the resolved journal path, the number of recorded
operations and any `pending` rows (with the current state of the files they
name); `Legacy Journal` reports a leftover `./data/kaseifu.db` from an older
build together with its recorded operation count — `0 operation(s) recorded;
safe to delete` means the file holds nothing, anything else may be undo
history. Both checks are read-only: `doctor` never creates, modifies or
deletes a journal (the old one is reported, never migrated).

### Uninstall

Remove what kaseifu installed. The default is a dry run: it lists every
target with its size and deletes nothing.

```bash
kaseifu uninstall
kaseifu uninstall --execute
```

`--execute` deletes the zsh completion script. The user configuration and the
journal are kept unless `--purge` is passed; purging the journal deletes the
undo history with it, so the command says so before anything happens. The API
key in the macOS Keychain is kept unless `--remove-key` (or `--purge`) is
passed, which asks for confirmation (`--yes` skips the prompt). In `env` mode
the Keychain is never read or deleted:

```bash
kaseifu uninstall --purge --execute
kaseifu uninstall --remove-key --yes
```

An old `./data/kaseifu.db` in the current directory is only deleted by
`--execute --purge`, and only when it can be read as a journal; journals in
other directories are never touched. Files kaseifu moved are out of scope
(restore them with `kaseifu undo` first), and the `kaseifu` command itself is
left in place: remove it with `cargo uninstall kaseifu`, which also stops the
completion script from being regenerated on the next run. `--format json`
prints the same target list as structured output.

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
kaseifu, or with `KASEIFU_KEY_SOURCE=auto kaseifu setup`).

Commands that call Jev (`classify`, `organize`, `watch`) resolve the key
first. When nothing is configured and stdin is a terminal, they prompt for
it with hidden input and store it where the mode keeps keys: the macOS
Keychain for `auto`/`keychain`, the project-local `.env` for `env` (created
with mode 0600 when missing, an existing `JEV_API_KEY=` line is replaced and
every other line is kept). In `env` mode the Keychain is still never read or
written. Without a terminal the command fails with the usual
"no API key configured" error and names the commands that need no key
(`inspect`, `doctor`) plus `kaseifu setup`. The key value is never printed,
logged or put into JSON, and an empty or interrupted entry stores nothing.
`watch` asks before it starts watching, never while it runs.

| Setting | Default | Description |

| Setting | Default | Description |
|---|---|---|
| `jev.endpoint` | `https://api.typesafe.ai/v1/systemone` | Jev API endpoint (base or full URL) |
| `jev.model` | `jev-latest` | Model alias sent to the API |
| `classifier.thresholds.auto_move_confidence` | 0.85 | Minimum confidence for automatic moves |
| `classifier.thresholds.auto_move_margin` | 0.40 | Minimum margin over second-best candidate |
| `classifier.thresholds.review_confidence` | 0.60 | Below this, the file is left in place |
| `operations.dry_run` | `true` | **Inactive** — accepted so existing files keep loading; moves are controlled by `--execute` |
| `operations.enable_undo` | `true` | **Inactive** — every executed move is journaled and undoable regardless |

The two `operations.*` settings are accepted for compatibility but have no
effect: `organize` and `watch` move files only with `--execute`, and every
executed move is journaled so `undo` can reverse it. `config show` marks both
keys as inactive (`inactive_settings` in `--format json`).

Developer setup (`.env` files, environment variables, local builds, tests,
CI, API mocks, resolution order) lives in `documents/development.md`.

## Project structure

```
src/
├── main.rs              # CLI entry point (clap)
├── lib.rs               # Library root
├── auth/                # Keychain access + interactive prompts (v3.3)
├── benchmark/           # TREC related-document harness (kaseifu-bench)
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
├── rename/              # Naming rules, document + image + video + folder rename
├── semantic/            # Foundation Models bridge + semantic provider (Phase 0-2)
├── storage/             # SQLite history + cache
└── watch/               # FSEvents watcher + coordinator (Phase 6)

swift/
└── kaseifu_fm_bridge.swift  # C ABI bridge to Apple Foundation Models (build.rs)

config/
└── default.toml         # Built-in defaults (documentation; not read at runtime)

schemas/
├── file-context-v1.json
├── candidate-destination-v1.json
├── jev-decision-v1.json
└── fm-rename-v1.json    # Rust <-> Swift Foundation Models contract

docs/
├── ARCHITECTURE.md
├── JEV.md
├── MACOS.md
└── design/              # Per-phase design documents
```

The Foundation Models bridge (the AI rename foundation, phases 0-2) is compiled
by `build.rs` when the machine has a Swift compiler and a macOS 26 SDK; it is
weak-linked, so the binary still starts on older systems and answers with an
explicit `unavailable: <reason>` instead of falling back to another model.
Without a Swift toolchain (or on an older SDK, like a CI runner) the build
falls back to a Rust stub with the same C ABI that reports
`unavailable: foundation-models-sdk-missing` / `swift-toolchain-missing`.
Because the bridge uses the OS Swift concurrency runtime, the binary requires
macOS 12.0 or later. To see what the bridge answers on this machine:

```bash
cargo run --example fm_bridge_smoke
```

Real inference runs there only with Apple Intelligence enabled; the test suite
stays model-independent (`cargo test --test v090_fm_bridge_test -- --ignored`
runs the inference path manually).

The TREC evaluation harness is a separate research binary (`kaseifu-bench`) that
measures related-document selection against the official Robust04 qrels. It is
not part of the user workflow:

```bash
cargo build --release --bin kaseifu-bench
./target/release/kaseifu-bench --help
```

The document corpus comes from NIST (the qrels are public). When a dataset is
missing the harness stops and says so; it never fabricates or substitutes data,
and it only ever reads the corpus.

## Safety

- **Dry run by default.** Nothing moves unless you pass `--execute`.
- **Validated candidates only.** Jev chooses from directories the local app discovered — it cannot invent destinations.
- **Journaled moves.** Every executed move is recorded in a SQLite database.
- **Undo.** Reverse any move with `kaseifu undo`.
- **Collision detection.** If a file already exists at the destination, the move is refused and flagged for review.
- **Model attribution.** File meaning and attributes (including who or what is pictured) come from Apple's on-device model and the Jev classifier. This tool does not restrict what those models may describe: it validates only the shape of their answers, and nothing moves without review.

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
