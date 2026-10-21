# `organize` refuses a folder instead of deciding one - done

- status: done
- completed: 2026-10-21 (GitHub issue [#14](https://github.com/smapira/kaseifu/issues/14), real-use feedback from `seo-operations`)
- milestone: M1 - reliability and CLI clarity
- next owner: none (verify at merge time)
- blocker: none

## Objective (as reported)

A folder given to `organize` ran the file pipeline and printed a plausible
verdict (exit 0, "no move"), while `organize --help`, the README and the skill
all say file. The report asked for one of two resolutions: refuse folders, or
support them officially (documenting what `--execute` does to a folder, journal
and undo included).

## Decision

Refuse, at the boundary. The plan layer already refuses to move a folder
(`PlanError::SourceNotAFile`), so no working capability is removed; supporting
folders would mean defining a folder move for the journal, `undo`, `--select`
and `--to`, which is a feature rather than a fix.

## What changed

- `src/main.rs`: `require_regular_file(target, command)` runs at the head of
  `dispatch_organize` (so `plain`, `--execute`, `--to`, `--select` and
  `--keywords` are all covered) and of `handle_classify` - before the repository
  policy is read, before the API key is resolved, before the journal is opened
  and before any network call. The message names the folder and the command that
  takes folders (`kaseifu label <folder>`).
- Both commands' long help states that the target is one regular file;
  `inspect` still reports a folder on purpose and `watch --path <dir>` still
  takes a directory tree.
- `tests/v113_folder_target_test.rs` (new, 5 tests): every organize shape, the
  `classify` refusal, the `inspect` contrast, a file that is still decided by
  its rule, and the help sentence.
- `README.md`, `src/skill/kaseifu/SKILL.md` (plus the byte-identical
  distributed copy) and `tests/user-test-plan.csv` (T42) state the rule.

## Verification

- `cargo test --test v113_folder_target_test`: 5 passed / 0 failed.
- `cargo test --test v109_skill_distribution_test`, `--test v111`, `--lib`:
  green after the same round's other changes.
- `cargo test --all-targets --all-features`: 962 passed / 0 failed (0.13.0
  baseline 955; +7: five v113 tests, `b17` and one library test).
- `bash scripts/quality_check.sh`: `OVERALL: PASS` (rustfmt 0, clippy 0,
  cargo-test 962 passed, CC >= 16 = 0, max CC 15).
- Manual sandbox runs (isolated HOME, no API key, stdin closed): a folder is
  refused with exit 1 in all six shapes; no journal directory is created; a
  folder that matches a rule is refused before the rule is read; a file still
  reports its rule hit with exit 0; `inspect <folder>` still exits 0.
