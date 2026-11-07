# `move`: create the destination, take several paths (#25)

Last updated: 2026-11-08.

Issue: [#25](https://github.com/smapira/kaseifu/issues/25). Predecessor:
[#22](https://github.com/smapira/kaseifu/issues/22) (`kaseifu move`, commit
`7702afc`). Implementation: `410cd7e`.

## The two constraints this removes

#22 shipped a move that needs an existing directory and takes one path. Real
use (filing `~/Desktop`: 13 folders / 5.8G) therefore needed 13 `mkdir` calls and
15 `move` calls, and the help text said it: `Directory to move the path into
(must already exist)`.

## What the command does now

```text
kaseifu move <path>... --to <dir> [--create-dir] [--execute]
```

- `--create-dir` creates the destination and any missing parent
  (`create_dir_all`). An existing directory is used as before; an existing
  **file** where the directory should go is refused, never replaced.
- The never-a-destination rules are checked on **every level the run would
  create**, not only on the leaf: `~/Library/**`, caches, bundles and system
  trees are refused as a creation target too.
- Several paths are one batch: one `batch_id` on every journal row, one report,
  per-path outcome, and a non-zero exit code when any path failed. Successful
  moves are never rolled back because a sibling failed.
- Dry run (the default) prints the directory it would create and the destination
  of every path, and touches nothing.
- The journal records each created level as an `mkdir` row in the same batch, so
  `undo` removes a directory kaseifu created once the restore leaves it empty.
  When nothing could be moved, the run removes the empty directory it made
  instead of leaving it behind.

## Decisions

- **A new flag rather than a changed default.** Creating a directory is a
  mutation of the destination; making it implicit would have changed what
  `move --to` means for every existing caller.
- **`<PATH>...` on the existing command, not a new `move-many`.** The decision
  is identical for every path (one destination, no classifier), so a second
  command would be the same code with a different name.
- **The creation is journaled per level, not as one row.** `remove_dir` refuses
  a non-empty directory, and undo removes deepest-first; one row per level is
  what makes `~/Documents/archive/2026` removable from the inside out, and it
  leaves a record of exactly what this run made.
- **A directory someone started using is never removed.** The emptiness check is
  the whole rule; undo does not ask whether the name came from kaseifu beyond
  the journal row.
- **Duplicate names in one batch are a plan collision.** Two sources called
  `a.txt` target one file; the plan names the second as a collision instead of
  letting the first move and the second fail at the rename.
- **One path keeps the one-path messages.** `cannot move this path: ...` and
  `move failed: ...` are unchanged for `len == 1`, because scripts and the
  command's own tests read them.
- **JSON stays additive.** One path keeps the keys #22 introduced; every run
  gains `items` and `create_directories`, and a multi-path run carries
  `batch_id` and `created_directories` instead of the single-path scalars.
- **`organize` is untouched.** #14 still refuses a folder at the boundary;
  `tests/v113_folder_target_test.rs` stays green.

## Verification (2026-11-08, worktree `kaseifu`)

- `tests/v121_move_create_dir_test.rs` (new, 9 tests): create + undo removes the
  created levels; dry run shows the directory and every path and creates
  nothing; an existing file is refused; an excluded tree is never created;
  several paths move and `undo --batch` restores them and removes the directory;
  one failing path does not roll the others back; two paths with one name
  collide; a batch that moves nothing leaves no directory; single-path JSON
  keeps its keys.
- `tests/v120_move_command_test.rs` and `tests/v113_folder_target_test.rs` stay
  green.
- `bash scripts/quality_check.sh`: OVERALL PASS (1053 tests, rustfmt 0, clippy 0,
  CC >= 16 = 0, max CC 15, coverage 84.99%).
- Isolated HOME, real binary: dry run printed
  `create directory: .../Documents/archive/2026 (with 2 parent folders)` plus
  `1. ... -> ...` / `2. ... -> ...` and created nothing; `--execute --format
  json` moved a folder and a file and returned `batch_id` / `items`;
  `undo --batch` restored both and printed `removed created directory:` for the
  three levels it had made.

## Deliberately not in this issue

- Progress output for a very large cross-volume tree (the copy stays silent
  until it finishes; recorded with #22).
- `watch` / `config` search scope and the path-shape coefficients: #23.
- The `prior` candidate presentation: the follow-up to #18 (see #23's
  discussion; the split was agreed in the issue tracker).
