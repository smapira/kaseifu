# `kaseifu skill`: emit and check the agent skill file

Last updated: 2026-10-19. Status: implemented and verified.

## 1. Objective

The user asked (2026-10-19) to add a function that emits skill files, using
`routeflags/netloom`'s `netloom skill` family as the reference. netloom embeds
its agent skill (`SKILL.md`) in the Go binary, places it in other repositories
(`skill install --to`, `--scan`), and compares copies with it by sha256
(`skill check`). kaseifu now does the same for its own skill:

- The canonical skill is `src/skill/kaseifu/SKILL.md` and is compiled into the
  binary with `include_str!`.
- `kaseifu skill install --to <repo> [--write [--force]]` places it at
  `<repo>/.github/skills/kaseifu/SKILL.md`; the default is a preview.
- `kaseifu skill install --scan <dir> [--write [--force]]` does the same for
  every repository at or below `<dir>` that already holds a copy.
- `kaseifu skill check --path <file>` compares a copy with the embedded file by
  sha256 and exits non-zero when it is stale, so one CI step detects drift.
- The repository ships its own copy at `.github/skills/kaseifu/SKILL.md`.

No new crate, no new configuration key, no new state: `sha2` and `walkdir` are
already dependencies, and no command reads or writes anything outside the file
the caller names.

## 2. Behavior units (as implemented and verified)

| ID | Behavior |
|---|---|
| B1 | `skill install --to <repo>` without `--write` creates nothing (not even the directory chain) and exits 0 with `status: preview (...)`; JSON is `{"skill":{"action":"install","status":"preview",...}}`. |
| B2 | `--write` writes `<repo>/.github/skills/kaseifu/SKILL.md` byte-identical to `src/skill/kaseifu/SKILL.md`, creating the directory chain; a second `--write` reports `up-to-date` and exits 0. |
| B3 | A copy that differs is refused: exit 1, `status: refused`, stderr names `--force`, and the file is left exactly as it was (sha256 compared before and after, both in the manual run and in the test). `--write --force` replaces it. |
| B4 | `skill check --path FILE`: byte-identical = exit 0 (`status: match`); differing = exit 1 (`status: stale`, both digests in the report); missing or unreadable = exit 1 with the `skill install --to` hint. |
| B5 | `skill install --scan DIR` reports one row per repository copy in lexical path order, skips `.git`, never follows a symbolic link, ignores a repository more than six directories below the scanned one, and only touches directories that already hold a copy (a scan never introduces one). |
| B6 | A scan exits 0 when every copy is fine and 1 when any copy was refused or could not be read; a scan directory that is not a directory is an error (exit 1), a tree without copies is not (exit 0). |
| B7 | Usage errors exit 2: no `--to`/`--scan`, both at once, `--force` without `--write`, `check` without `--path`. |
| B8 | `--format json` applies to all three verbs (`{"skill": ...}` documents); `skill` joined the command list in the `--format` help text, and `tests/v070_zero_config_p1_help_test.rs` pins the new wording. |
| B9 | The command writes nothing else: the best-effort completion refresh every other command performs is skipped for `skill`, so `skill check` in CI writes nothing anywhere (asserted by test, with an ordinary command in the same sandbox as the contrast). |
| B10 | The repository's own copy `.github/skills/kaseifu/SKILL.md` is byte-identical to the canonical file: asserted by a test and by `skill check` against the built binary. |
| B11 | Help output stays ASCII and free of internal wording; the whole-help scan walks `skill`, `skill install` and `skill check` automatically. |

## 3. Design decisions

- **The reference's verbs and semantics, kaseifu's conventions.** `install` /
  `check`, `--to` / `--scan` / `--write` / `--force` / `--path`, preview by
  default, refuse a differing copy without `--force`, and the same words
  (`up-to-date` / `installed` / `preview` / `refused` / `FAIL`) in text and
  JSON are taken from netloom because they are already thought through. The
  surrounding machinery stays kaseifu's: clap reconcile, `emit_report`, a
  data-only module plus a renderer, and the repository's own exit-code
  convention (0 success, 1 failure, 2 command line error) instead of netloom's
  extra 3-for-I/O code - kaseifu's `main` reports every failure as one, and one
  failure code is enough for a CI step that reads stderr for the reason.
- **No version in the skill's frontmatter** (a deliberate deviation). netloom
  writes `netloom-version:` into the skill and warns when it differs from the
  binary. That makes every release invalidate every copy, and adds a release
  step that has to be remembered. kaseifu's rule is single: the command
  compares content, so a copy is stale exactly when the content changed. The
  report states both digests, which is what a human or a CI log needs.
- **A refusal is a report status, not an error.** `install` returns a report
  that says `refused` and the caller turns that into exit 1 (with the reason on
  stderr). This keeps the module free of user-facing strings, lets `--scan`
  aggregate refusals as rows instead of aborting, and matches the existing
  "the report is the result" style of `uninstall`.
- **`--force` requires `--write`.** A caller who typed `--force` alone almost
  certainly meant `--write --force`; clap refuses the combination instead of
  silently doing nothing.
- **`--to` and `--scan` are one clap `ArgGroup`** (required, single), so the
  error reads `<--to <DIR>|--scan <DIR>>` and the usage line shows both.
- **Scan limit: repositories up to six directories below the scanned one**,
  and the walk stops four levels deeper than that limit. The limit is on the
  repository, not the file, and six reaches `.delta/worktrees/<id>/<repo>`,
  the deepest layout a scan is pointed at in practice. The element count of
  `.github/skills/kaseifu/SKILL.md` is a constant next to the limit, with a
  unit test keeping the two in step.
- **English, ASCII-only skill text.** kaseifu's documentation and its default
  response language are English, and the repository's output rules apply to the
  emitted file as well; the unit test rejects non-ASCII and an em dash.
- **`skill` keeps the completion refresh off.** Every other command
  best-effort refreshes the installed zsh script; a command whose whole job is
  to place or check a named file must not write anywhere else, because
  `skill check` is meant to run in CI.

## 4. File changes

| # | File | Change |
|---|---|---|
| T1 | `src/skill/kaseifu/SKILL.md` | New: the canonical agent skill (frontmatter plus the agent-facing manual: read-only commands, moving, naming, watching, exit codes, this file's own upkeep, scope limits). |
| T2 | `src/skill/mod.rs` | New module: the embedded skill, `sha256_hex`, `install`, `check`, `scan`, `repo_dir_of`, `INSTALL_REL_PATH`, `MAX_SCAN_DEPTH`; unit tests for the path-element constant, the ASCII/name invariants and `repo_dir_of`. |
| T3 | `src/presentation/renderers/skill.rs` | New: text lines and `{"skill": ...}` JSON for the three verbs, sharing one status vocabulary and detail sentences. |
| T4 | `src/lib.rs`, `src/presentation/renderers/mod.rs` | Register the two modules. |
| T5 | `src/main.rs` | `Commands::Skill` + `SkillCommands`, `handle_skill`, `handle_skill_install`, `handle_skill_scan`, `handle_skill_check`, the completion-refresh exclusion, and `skill` added to the `--format` help list. |
| T6 | `.github/skills/kaseifu/SKILL.md` | New: the repository's distributed copy, byte-identical to T1. |
| T7 | `tests/v109_skill_distribution_test.rs` | New: 11 tests covering B1-B11. |
| T8 | `tests/v070_zero_config_p1_help_test.rs` | The pinned `--format` sentence now includes `skill`. |
| T9 | `README.md`, `CHANGELOG.md` | Usage section (with the CI line) and the `[Unreleased]` entry. |

## 5. Verification (2026-10-19, worktree `kaseifu`)

- `cargo test --test v109_skill_distribution_test`: 11 passed, 0 failed.
- `cargo test --all-targets --all-features`: 900 passed, 0 failed (up from
  886: 11 integration tests plus 3 module unit tests).
- `bash scripts/quality_check.sh`: `OVERALL: PASS` (rustfmt 0 violations,
  clippy 0 warnings, complexity CC >= 16 = 0, max CC 15, coverage 83.93%
  report-only).
- Manual sandbox runs (temp HOME, `KASEIFU_NO_COMPLETION_UPDATE=1`, temp
  trees) for every behavior unit above, including the sha256 of a refused copy
  before and after a `--scan --write` run.
- Embedded skill digest of this revision:
  `da6f701412c01aef927326fad7adab1027469fc533005aeb96a0dfd4a0510c24`.
