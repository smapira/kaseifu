# `kaseifu move`: file or folder, no decision - done

- status: done
- completed: 2026-11-08 (GitHub issue [#22](https://github.com/smapira/kaseifu/issues/22); commits `7702afc`, `96b946b`)
- milestone: M3 - Workflow semantics (a new command with journal and undo
  semantics, not a fix)
- next owner: none (verify at merge time)
- blocker: none

## The finding

#14 decided that `organize` refuses a folder at the boundary, and that decision
is not revisited here: a folder move needs its own journal, `undo`, `--select`
and `--to` semantics. But there was no other route, so a folder could not be
filed at all (`Error: cannot move this file: source is not a regular file`),
while 13 of 15 measured `~/Desktop` targets were folders. The undo side already
handled a directory (`is_file() || is_dir()`), and the journal recorded `move`
without a type, so the missing piece was the command, not the data model.

## Decision

- A separate, decision-free command: `kaseifu move <path> --to <dir>
  [--execute]`. No discovery, no classifier, no key, no network; dry run by
  default like every other mutation.
- Both ends go through the never-a-destination rules, so `~/Library/**`, caches,
  system trees and bundles are refused as source and as destination. A source
  the user named explicitly keeps its hidden or timestamped name (only the tree
  rules apply to it), which is what makes `move ~/Desktop/.env` possible.
- Refused: a collision (never overwritten), the source's own directory, and a
  folder into its own subtree. Symlinks and special files are refused as
  sources.
- Same volume: `rename`. Across volumes: copy, verify (file count and bytes),
  then remove the source. A failed copy or verification removes the partial copy
  and leaves the source whole; **a source that cannot be removed leaves both
  sides intact**, because a verified copy must never be deleted just because the
  other side failed.
- The journal gains an additive `kind` column (`file` / `dir`); old rows read as
  no kind, which is `file`. `undo` reverses both kinds through the existing path.
- `organize` / `classify` keep their file-only contract.

## What changed

- `src/operations/relocate.rs` (new): `plan_relocate`, `execute_relocate`,
  `transfer`, `copy_then_remove`, `copy_tree`, `verify_copy`, `remove_path`;
  the source predicate is the tree rules only.
- `src/storage/history.rs`: `kind` column with the same additive migration as
  `batch_id`, `record_pending_kind`, `HistoryEntry.kind`.
- `src/operations/undo.rs`: the same `transfer` helper for both kinds.
- `src/main.rs`: the `move` subcommand, its `--execute` / `--format json`
  report, and the dry-run wording.
- README, `src/skill/kaseifu/SKILL.md` (and the placed copy under
  `.github/skills/kaseifu/`), CHANGELOG `[Unreleased]`.
- `tests/v120_move_command_test.rs` (new, 15 tests) plus 4 module tests in
  `relocate.rs`.

## Verification (2026-11-08)

- `tests/v120_move_command_test.rs`: 15 passed (same-volume move + full undo,
  cross-volume copy failure leaves no residue, source-removal failure keeps both
  sides, excluded source/destination, collision, nested folder, journal `kind`,
  old rows, `undo --batch`, `organize` still refuses a folder).
- `tests/v113_folder_target_test.rs` stays green (the file-only contract).
- CLI smoke in an isolated HOME, run by the reviewer at `96b946b`: dry run ->
  `--execute` (`kind: dir`, `operation_id`) -> `undo` restored the folder to its
  original path with the file inside.
- `bash scripts/quality_check.sh` at `96b946b`: OVERALL PASS (1044 tests,
  max CC 15, coverage 84.81%).
- Review fix `96b946b`: the first implementation removed the verified copy when
  the source could not be removed; that path now keeps both sides.

## Follow-ups

- `move` takes one path per call; a batch form would need its own `--to` and
  report shape and is not part of this issue.
- Progress reporting for a very large cross-volume tree is not implemented
  (the copy is silent until it finishes).
