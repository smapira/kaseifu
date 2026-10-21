# Skill coverage and `skill check` exit codes - done

- status: done
- completed: 2026-10-21 (GitHub issue [#15](https://github.com/smapira/kaseifu/issues/15), real-use feedback from `seo-operations`)
- milestone: M1 - reliability and CLI clarity
- next owner: none (verify at merge time)
- blocker: none

## Objective (as reported)

Three details from real use of 0.13.0: the agent skill (the file an agent reads)
covered none of the release's new features; the README described three different
destination refusals in one sentence; and `skill check` exited 1 both for a
stale copy and for a file that does not exist, so a CI step could not tell the
two apart.

## What changed

- `src/skill/kaseifu/SKILL.md` (embedded; the distributed copy is refreshed with
  the binary and stays byte-identical): a "Repository policy (`.kaseifu.toml`)"
  section, `organize <file> --keywords` with what reaches Jev and what does not,
  the `Index  not indexed yet; candidates may be incomplete` row and
  `target_indexed`, the one-file rule for `organize` / `classify`, and exit
  code 3.
- `README.md`: the policy prose states the two matching bases and the three
  refusals separately (read-time error for absolute / `.` / `..` / `{name}`,
  REVIEW for a resolved symbolic link), and the skill section documents
  `match` = 0, `stale` = 1, no file = 3.
- `src/skill/mod.rs`: `check` returns `CheckOutcome::{Compared, Missing}`
  (a missing file is an outcome, not an error); `src/main.rs`
  (`handle_skill_check` + `exit_with_code`) reports the missing copy with exit
  3 - the only place kaseifu leaves 0/1/2.
- `tests/v109_skill_distribution_test.rs`: the missing-copy case asserts exit 3.

## Verification

- `cargo test --test v109_skill_distribution_test`: green (11 tests).
- `cargo test --lib skill`: green, including the new
  `check_reports_a_missing_copy_and_compares_the_canonical_one`.
- `shasum -a 256` on both skill copies: identical after the binary refresh
  (`kaseifu skill install --to . --write --force`), and `kaseifu skill check`
  exits 0 on the repository's own copy.
- Manual runs: `skill check` at 0 (match), 1 (stale, both digests printed) and
  3 (no file, message names the install command); a directory passed as
  `--path` stays a read error (exit 1).
- Full suite and `scripts/quality_check.sh`: see
  `20261021-done-organize-folder-target.md` (same round, one worktree).
