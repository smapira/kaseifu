# `cluster`: file each kind into its OS standard folder - done

- status: done
- completed: 2026-11-09 (GitHub issues [#27](https://github.com/smapira/kaseifu/issues/27)
  and [#28](https://github.com/smapira/kaseifu/issues/28); see
  `documents/plans/dev/20261109a-cluster-destination.md`)
- milestone: M3 - Workflow semantics (changes where `cluster` files a file, not
  how it decides one)
- next owner: none (verify at merge time)
- blocker: rule 1 of #28 (match an existing directory under the kind root) waits
  on the threshold/depth decision the issue leaves open

## The finding

#26 created the group folders inside the scanned directory (`<dir>/<name>`).
The owner rejected that layout twice: first because tidying `~/Desktop/misc`
produced `~/Desktop/misc/images/` (the clutter moved one level down), then
because filing into `~/Pictures/images` repeats the parent every run and queues
`documents (2)` once the folder exists.

## Decision

- **A run creates nothing unless `--create-dir` is given.** The default files each
  file into the OS standard folder for its type, into the extension folder when it
  already exists (`R/<extension>`), and into `R` otherwise, and never into the
  scanned directory.
- **The type folder is the extension, and nothing else.** The standard folder
  already says what the files are, so the `images` / `documents` / `code` / `data`
  / `misc` levels are gone (owner decision, 2026-11-09): an html file is
  `kind: code` and is filed under `~/Documents/html`, a workbook is
  `kind: documents` and is filed under `~/Documents/xlsx`.
- **A kind nothing names has no folder.** `psd`, `ttf`, `webloc` and a file with
  no extension are not turned into a `misc` bucket: they go flat into the OS
  standard folder (`~/Documents/x.ttf`), and `xls` / `xlsx` stay in the document
  family with `pdf` / `md` / `numbers` rather than being unknown.
- **The classification is not a folder.** `class_name()` keeps `code` / `data`
  and is `null` for a file nothing names; the report carries it as `kind` and the
  extension is the only folder, so nothing has to be kept in two places.
- A standard folder that does not exist is an error naming every missing root and
  `--create-dir`, with nothing moved.
- A name already taken at the destination is refused for that file only; the rest
  of the batch runs and the exit code says something was left behind.
- `--create-dir` builds the folders under the extension folder (`R/E/S`), and a
  name that is only the type name lands flat in `R/E` - a folder called `mp4`
  inside `.../mp4` adds nothing. In place the extension folder is the grouping,
  so the folder is always created there.
- `--min` / `--max` / `--by-type` / `--name-source` / `--in-place` require
  `--create-dir`: they only describe a folder that is being made. The default
  therefore files a group of one, while the creating mode leaves a group below
  `--min` as `ungrouped` and in place - on the measured Desktop folder, 130
  files by default and 62 with `--create-dir` (the rest are one-file types).
- A file already under its kind root is reported as
  `already in the destination`; `--in-place` is exempt.
- `undo --batch` removes the levels the run created (including a `~/Movies` that
  did not exist), because #25's machinery journals each created level.
- **Rule 1 of #28 is deferred, not dropped**: matching an existing directory
  under the kind root needs the threshold and depth the issue leaves open, and
  the measurement shows 0.2-scored date folders would swallow files (one exact
  match at 1.00, thirty candidates at 0.06-0.35). It becomes an opt-in
  `--match-existing` once real match data justifies a line
  (`../open/20261109-open-cluster-match-existing.md`).
- **`xls` / `xlsx` are documents**, like `pdf` / `md` / `txt` / `numbers`: a
  workbook is not an unknown type, and the measured `ToDo.xlsx` is now
  classified `documents` instead of `kind: null`.
- **A root that holds only directories is a separate issue.** `cluster <dir>`
  fails with `no files to cluster` and reports neither the directories it left
  alone nor the skipped `.DS_Store`
  (`../open/20261109-open-cluster-directory-only-root.md`); it does not change
  any file, so it is not a release blocker.
