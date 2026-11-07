# `move --create-dir` and several paths - done

- status: done
- completed: 2026-11-08 (GitHub issue [#25](https://github.com/smapira/kaseifu/issues/25); commit `410cd7e`)
- milestone: M3 - Workflow semantics (extends the command #22 added, with
  journal and undo semantics for the created directory)
- next owner: none (verify at merge time)
- blocker: none

## The finding

`move` (#22) needed an existing directory and took one path, so filing a real
`~/Desktop` (13 folders / 5.8G) needed 13 `mkdir` calls and 15 `move` calls. The
help text said it outright: `Directory to move the path into (must already
exist)`, and a second path was rejected as an unexpected argument.

## Decision

- `--create-dir` is explicit: creating a directory is a mutation of the
  destination, and making it the default would change what `--to` means for
  every existing caller. An existing **file** where the folder should go is
  refused rather than replaced.
- Several paths ride the existing command (`<PATH>...`), not a second command:
  one destination, no classifier, so the decision per path is identical.
- Each created level is journaled as its own `mkdir` row in the same batch, which
  is what lets `undo` remove `~/Documents/archive/2026` from the inside out and
  only while it is empty. A batch that moved nothing removes the empty directory
  it made instead of leaving it behind.
- One failed path is reported, the others still move, and the run exits non-zero.
  A successful move is never rolled back because a sibling failed.
- One path keeps the one-path messages and the JSON keys #22 introduced; every
  run gains `items` / `create_directories`, and a multi-path run carries
  `batch_id` / `created_directories`.
- `organize` is untouched: #14 still refuses a folder at the boundary.

## What changed

- `src/operations/relocate.rs`: `MoveBatch` / `MoveItem` / `BatchOutcome`,
  `plan_batch`, `dry_run_batch`, `execute_batch`, `create_missing_directories`,
  `remove_empty_directories`, `remove_created_directories`,
  `destination_levels` (the never-a-destination rule on every level that would be
  created), and the shared `execute_one` used by both the single and batch paths.
- `src/storage/history.rs`: `record_directory_creation` (`mkdir` rows),
  `created_directories_in_batch`, and the shared `new_batch_id`.
- `src/operations/undo.rs`: `UndoOutcome.removed_directories`; an undo removes
  the directories its batch created once they are empty.
- `src/main.rs`: the `move` flags, the batch handler, the per-path report, and the
  additive JSON document (`move_report_document`); the `--format` help names
  `move`.
- `src/rename/batch.rs`: uses the shared batch-id helper.
- `src/skill/kaseifu/SKILL.md` (and the placed `.github` copy), README, CHANGELOG
  `[Unreleased]`.
- `tests/v121_move_create_dir_test.rs` (new, 9 tests); `tests/v120` follows the
  shared `PendingRecord` shape; `tests/v070_zero_config_p1_help_test.rs` follows
  the help line that now names `move`.

## Verification (2026-11-08)

- `tests/v121_move_create_dir_test.rs`: 9 passed (see
  `documents/plans/dev/20261108b-move-create-dir.md` for the list).
- `tests/v120_move_command_test.rs` and `tests/v113_folder_target_test.rs` stay
  green.
- `bash scripts/quality_check.sh`: OVERALL PASS (1053 tests, rustfmt 0,
  clippy 0, CC >= 16 = 0, max CC 15, coverage 84.99%).
- Isolated HOME, real binary: dry run printed the directory to create and both
  path mappings and created nothing; `--execute` moved a folder and a file as one
  batch; `undo --batch` restored both and removed the three created levels.
