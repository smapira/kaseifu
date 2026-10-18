# `kaseifu skill`: emit and check the agent skill file - done

- status: done
- completed: 2026-10-19 (user request)
- milestone: n/a - user-requested feature (reference: `routeflags/netloom`'s `skill` family)
- next owner: none (verify at merge time)
- blocker: none

## Objective (as requested)

Add a function that emits skill files, referring to
`/Users/bookair18/OS/home/Codes/github.com/routeflags/netloom`'s
`netloom skill install` / `skill check` family.

## What changed

- New canonical skill `src/skill/kaseifu/SKILL.md`, embedded in the binary with
  `include_str!`; the repository's own copy lives at
  `.github/skills/kaseifu/SKILL.md` and is kept byte-identical (tested).
- New `kaseifu skill` command family: `install --to <repo> [--write [--force]]`,
  `install --scan <dir> [--write [--force]]`, and `check --path <file>`.
  Preview by default; a copy that differs from the embedded skill is refused
  unless `--force` asks for the replacement; `check` exits 1 for a stale copy
  so one CI step detects drift.
- New module `src/skill/mod.rs` (data plus decisions; `sha2` and `walkdir` were
  already dependencies) and renderer `src/presentation/renderers/skill.rs`
  (text lines and `{"skill": ...}` JSON, one status vocabulary).
- `skill` is excluded from the best-effort completion refresh, so `skill check`
  in CI writes nothing anywhere; the `--format` help list and its pinning test
  in `tests/v070_zero_config_p1_help_test.rs` now include `skill`.
- `tests/v109_skill_distribution_test.rs` (new, 11 tests) plus three module
  unit tests cover preview, write, refusal, force, check (match/stale/missing),
  scan order and limits, usage errors, the JSON shapes, the repository copy,
  and "writes nothing else".
- `README.md` usage section, `CHANGELOG.md` `[Unreleased]`, and the dev brief
  `documents/plans/dev/20261019a-skill-distribution.md`.

## Verification (2026-10-19, worktree `kaseifu`)

- `cargo test --test v109_skill_distribution_test`: 11 passed, 0 failed.
- `cargo test --all-targets --all-features`: 900 passed / 0 failed.
- `bash scripts/quality_check.sh`: OVERALL PASS (rustfmt 0, clippy 0,
  cargo-test 900 passed, CC >= 16 = 0, max CC 15, coverage 83.93% report-only).
- Manual sandbox runs for every verb (temp HOME and temp trees), including the
  refusal path leaving a copy's sha256 unchanged under `--scan --write`.
- Embedded skill digest of this revision:
  `da6f701412c01aef927326fad7adab1027469fc533005aeb96a0dfd4a0510c24`.
