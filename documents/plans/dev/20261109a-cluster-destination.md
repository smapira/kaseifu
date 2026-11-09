# `cluster`: file each kind into its OS standard folder (#27, #28)

Last updated: 2026-11-09.

Issues: [#27](https://github.com/smapira/kaseifu/issues/27) (the parent folder),
[#28](https://github.com/smapira/kaseifu/issues/28) (do not create one). Reuses
[#18](https://github.com/smapira/kaseifu/issues/18)'s extension table and
[#25](https://github.com/smapira/kaseifu/issues/25)'s created-directory
machinery.

## Why

#26 created the group folders inside the scanned directory (`<dir>/<name>`). The
owner rejected that layout: tidying `~/Desktop/misc` produced
`~/Desktop/misc/images/`, so the Desktop still carried the clutter, one level
down. #27 moved the folders to the OS standard directory for the kind, and #28
went one step further: a run that was asked to tidy must not answer by building
folders the owner did not ask for (`~/Pictures/images` repeats the parent,
`documents (2)` queues up on every run).

## What the command does now

```text
kaseifu cluster <dir> [--create-dir] [--min N] [--max N] [--include-folders]
                    [--recursive] [--by-type] [--name-source type|auto|model]
                    [--to <base> | --in-place] [--execute]
```

R is the kind root, T the type folder, S a name chosen for the group.

| # | Situation | Destination |
|---|---|---|
| 1 | Default, a matching directory under R | not implemented yet (see below) |
| 2 | Default, the type folder `R/T` exists | `R/T` |
| 3 | Default, neither | `R` directly - **nothing is created** |
| 4 | `--create-dir`, a semantic name | `R/T/S` |
| 5 | `--create-dir`, the name is the type name | `R/T` (flat) |

- **R** = the OS standard folder: `~/Pictures` (images), `~/Documents`
  (documents), `~/Downloads` (archives), `~/Movies` (video), `~/Music` (audio),
  and `~/Documents` for a type the table does not name (csv, no extension, code,
  folders). The scanned directory is never a destination.
- **T** = `Category::default_name()`: `images` / `documents` / `archives` /
  `videos` / `audio` / `code` / `data` / `folders` / `misc`.
- **Nothing is created without `--create-dir`.** A kind root that does not exist
  is an error naming every missing root and the flag that would create it, with
  nothing moved.
- **A file already under R is left alone** and reported as
  `already in the destination`; `--in-place` is exempt (it files into R).
- **A name already taken at the destination** is refused for that file only and
  reported on its own `!` line; the rest of the batch runs and the exit code
  says something was left behind.
- **`--create-dir`** groups and names as #26 did, then creates the folder under
  the type folder. `--min` / `--max` / `--by-type` / `--name-source` /
  `--in-place` require it (clap `requires`): none of them means anything when
  nothing is created.
- **Parent creation and undo** ride on #25's journal: every created level is a
  row, so `undo --batch` removes `R`, `R/T` and `R/T/S` once they are empty.
- **`--to <base>`** replaces R with one directory (rule 2 applies to it too), and
  **`--in-place`** replaces R with the scanned directory; the two are mutually
exclusive. `--in-place` keeps the #26 shape (`<dir>/S`) because the scanned
  folder is not a type root, so there is no T layer.

## Decisions

- **Rule 1 (match an existing directory under R) is not implemented.** The issue
  leaves its threshold and search depth open, and the measurement behind it shows
  why they matter: of 130 files, exactly one matched at score 1.00 while thirty
  others landed at 0.06-0.35 on folders like
  `~/Documents/Ambient Context/Days/2026-09-23`. Filing a file into that folder
  on a 0.2 score is worse than filing it into `~/Documents`. Rules 2-5 are
  settled and do not depend on the answer, so they ship now and rule 1 lands
  when the owner picks the line (recorded recommendation: `R`'s direct children
  only, score >= 0.6, plus an exact-name or same-directory-parent reason).
- **`~/Documents` for a type the table does not name**, as the issue states. The
  alternative (leave those files in place with a reason) is recorded as open in
  #28; if the owner picks it, `default_dir` returns no root for `Data` / `Code` /
  `Folder` / `Other` and the file is reported instead of filed.
- **Rule 5 exists so a type name is never a folder.** The measured
  `Movies/videos` came from a group whose only candidate was the type name;
  `videos (2)` is the same non-information with a number. In place the type
  folder is the grouping, so rule 5 does not apply there.
- **The default output changed**, so this is a minor release (v0.16.0), not a
  patch: `cluster` no longer creates anything by default.

## Output

Text, one block per destination:

```text
destination: OS standard folders (no directory is created)
images -> /Users/me/Pictures (34 files, 25.7 MB)
1. /Users/me/Desktop/misc/IMG_8478.JPG -> /Users/me/Pictures/IMG_8478.JPG
  ! refusing to overwrite /Users/me/Pictures/IMG_8479.JPG; nothing was changed
```

JSON adds `creates_directory`, per group `kind` and `destination_base`, and keeps
`name` (now `null` when no folder is created), `destination_mode`,
`destination_base`, `create_directories` and `already_in_destination`.

## Verification

- `tests/v123_cluster_destination_test.rs` (14 tests): the default creates no
  directory and files into the standard folders, the existing type folder wins, a
  missing root names the flag, `--create-dir` builds and `undo` removes, a
  collision leaves one file, `--to` with and without `--create-dir`, `--in-place`,
  the `requires` flags, an excluded base, a quoted `~`, and the flag conflict.
- `src/cluster/mod.rs` unit tests: rules 4 and 5 with a fixed namer, plus the
  kind-root table.
- `src/platform/user_dirs.rs` unit test: `expand_home` expands only `~` and `~/...`.
- `tests/v122_cluster_test.rs` keeps the #26 rules under `--create-dir
  --in-place`.

## Paths

A leading `~` in `<dir>` and `--to` is resolved with
`platform::user_dirs::expand_home` before the plan, because a quoted path in a
script arrives unexpanded and would otherwise plan `./~/Archive`. Only `~` and
`~/...` are expanded; `~user` needs an account lookup the shell owns.

