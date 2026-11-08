# `cluster`: file each kind into its OS standard folder (#27)

Last updated: 2026-11-09.

Issue: [#27](https://github.com/smapira/kaseifu/issues/27). Reverses the
in-place layout #26 chose; reuses [#18](https://github.com/smapira/kaseifu/issues/18)'s
extension table and [#25](https://github.com/smapira/kaseifu/issues/25)'s created
directory machinery.

## Why

#26 created the group folders inside the scanned directory (`<dir>/<name>`). The
owner rejected that layout: tidying `~/Desktop/misc` produced
`~/Desktop/misc/images/`, so the Desktop still carried the clutter, one level
down. Tidying means returning scattered files to where they belong, not growing
a tree inside the folder they were cluttering.

## What the command does now

```text
kaseifu cluster <dir> [--min N] [--max N] [--include-folders] [--recursive]
                    [--by-type] [--name-source type|auto|model]
                    [--to <base> | --in-place] [--execute]
```

- **Default**: each group folder is created under the OS standard directory for
the group's kind - `~/Pictures` (images), `~/Documents` (documents),
  `~/Downloads` (archives), `~/Movies` (video), `~/Music` (audio) - resolved
  through `dirs` and `src/platform/user_dirs.rs`. The scanned directory is never
  a destination.
- **A kind the table does not name** (csv, a file with no extension, code,
  folders) goes to `~/Documents`. The owner's rule is that the scanned directory
  is not a destination, so a document folder is a better answer than leaving the
  clutter where it was; the choice is recorded here rather than in a comment.
- **`--to <base>`** files every group under one directory instead. The base and
  its missing parents are created, every created level passes the
  never-a-destination rule, and a base that rule refuses (`~/Library/**`, caches,
  system trees) is refused before anything is planned or moved.
- **`--in-place`** keeps the #26 layout for a folder that is being reorganised in
  place. `--to` and `--in-place` are mutually exclusive.
- **Membership, names and the journal are unchanged** from #26: the same
  deterministic grouping, the same candidate names, one batch id, `undo --batch`
  reverses the moves and removes the created levels - now including the standard
  folders the run made (for example a `~/Movies` that did not exist yet).
- **A file already under the destination is left alone** and listed as
  `already in the destination`: running `cluster ~/Documents` must not build
  `~/Documents/documents/` around files that are already filed. In place, the
  rule does not apply, because a folder inside the scanned directory is the point
  of the flag.

## Output

Text, one block per group:

```text
cluster: images (11 files, 1.2 MB)
  examples: cbd-chart-1.png, cbd-chart-2.png
  create directory: /Users/me/Pictures/images
1. /Users/me/Desktop/misc/cbd-chart-1.png -> /Users/me/Pictures/images/cbd-chart-1.png
```

The `create directory:` row is the one `move` prints (a shared helper), and the
destination is absolute because the folder is no longer inside the scanned
directory. `--in-place` keeps the #26 shape (`-> images/cbd-chart-1.png`) and
leaves the row out.

JSON adds `destination_mode` (`default_dirs` / `to` / `in_place`),
`destination_base` (the `--to` value, `null` otherwise), per group
`destination_base` and `create_directories`, and `already_in_destination`. Every
previously published key keeps its meaning.

## Decisions

- **The default is a breaking change, so it is a minor release (v0.16.0).** A
  caller scripting the text report sees absolute destinations now; the opt-in for
  the old shape is `--in-place`.
- **`~/Documents` for an undecided kind** rather than a `misc/` bucket in the
  scan directory: the issue names this explicitly, and a `misc` folder in
  `~/Documents` would only move the ambiguity.
- **The destination is resolved by kind, not by extension.** A group is already
  one kind (`png` and `jpg` land together), so the mapping is
  `Category -> UserDirKind`; a unit test pins it to
  `kind_for_extension()` for every kind both tables know.
- **`--to` wins nowhere and loses nowhere**: it conflicts with `--in-place`
  instead of silently overriding it.

## Verification

- `tests/v123_cluster_destination_test.rs` (7 tests): the default mapping for
every kind, execute and undo across the standard folders, `--in-place` layout and
  text, a file already under the destination, `--to` with created parents and
  undo, an excluded base, and the flag conflict.
- `tests/v122_cluster_test.rs` keeps the #26 rules under `--in-place` where they
  pin that layout.
- `bash scripts/quality_check.sh`.
