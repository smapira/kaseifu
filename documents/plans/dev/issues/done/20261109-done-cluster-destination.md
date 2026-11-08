# `cluster`: file each kind into its OS standard folder - done

- status: done
- completed: 2026-11-09 (GitHub issue [#27](https://github.com/smapira/kaseifu/issues/27);
  see `documents/plans/dev/20261109a-cluster-destination.md`)
- milestone: M3 - Workflow semantics (changes where `cluster` files a group, not
  how it decides one)
- next owner: none (verify at merge time)
- blocker: none

## The finding

#26 created the group folders inside the scanned directory (`<dir>/<name>`).
The owner rejected that layout: tidying `~/Desktop/misc` produced
`~/Desktop/misc/images/`, so the Desktop still carried the clutter, one level
down. Tidying means returning scattered files to where they belong.

## Decision

- **The default is the OS standard folder for the kind**: `~/Pictures` (images),
  `~/Documents` (documents), `~/Downloads` (archives), `~/Movies` (video),
  `~/Music` (audio), resolved through `dirs` and `src/platform/user_dirs.rs`. The
  scanned directory is never a destination by default.
- A kind the table does not name (csv, no extension, code, folders) goes to
  `~/Documents`, as the issue states. The alternative - a `misc/` bucket in the
  scan directory - only moves the ambiguity.
- `--to <base>` files everything under one directory (created with its parents,
  every level subject to the never-a-destination rule); `--in-place` keeps the
  #26 layout for a folder that is reorganised in place. The flags are mutually
  exclusive rather than one silently winning.
- A file already under the destination is left alone and reported as
  `already in the destination`, so `cluster ~/Documents` cannot build
  `~/Documents/documents/` around files that are already filed.
- `undo --batch` removes the levels the run created, including a standard folder
  that did not exist before (`~/Movies`), because #25's machinery journals the
  creation per level.
- The default output changed (absolute destinations plus a `create directory:`
  row), so this is a minor release, not a patch.
