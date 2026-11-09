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
  file into the OS standard folder for its type, at the deepest folder of its type
  chain that already exists (`R/<class>/<extension>`, then `R/<class>`, then `R`),
  and never into the scanned directory.
- **The extension is the type.** A kind the platform table does not name
  (`html`, `sh`, `psd`, `ttf`, `opml`, `smmx`, `yml`, no extension) is a document
  by destination, and its extension is the folder under `documents`; the
  `code` / `data` / `misc` folders were removed. The same chain applies to the
  kinds the table knows (`images/png`, `videos/mp4`), which is what the owner
  asked for.
- **Classification and destination stay separate.** `class_name()` keeps
  `code` / `data` and is `null` for a file nothing names (reported as `kind`),
  `class_folder()` gives the folder (`documents`, always); one value would lose
  the classification as soon as a folder name is reused.
- A standard folder that does not exist is an error naming every missing root and
  `--create-dir`, with nothing moved.
- A name already taken at the destination is refused for that file only; the rest
  of the batch runs and the exit code says something was left behind.
- `--create-dir` builds the folders under the type chain (`R/T/E/S`), and a name
  that is only the type name lands flat in `R/T/E` - a folder called `mp4` inside
  `.../mp4` adds nothing. In place the type folder is the grouping, so the folder
  is always created there.
- `--min` / `--max` / `--by-type` / `--name-source` / `--in-place` require
  `--create-dir`: they only describe a folder that is being made.
- A file already under its kind root is reported as
  `already in the destination`; `--in-place` is exempt.
- `undo --batch` removes the levels the run created (including a `~/Movies` that
  did not exist), because #25's machinery journals each created level.
- **Rule 1 of #28 is not implemented**: matching an existing directory under the
  kind root needs the threshold and depth the issue leaves open, and the
  measurement shows 0.2-scored date folders would swallow files. Recommended
  line: direct children of the kind root, score >= 0.6.
