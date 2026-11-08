# `cluster`: group a flat folder into named subdirectories (#26)

Last updated: 2026-11-08.

Issue: [#26](https://github.com/smapira/kaseifu/issues/26). Reuses the move
batch of [#25](https://github.com/smapira/kaseifu/issues/25). Implementation:
`e6c305f`.

## Why

`organize` answers "one file into one existing place" and `move` asks the human
for the destination. A directory with 37 flat files (the issue's `~/Desktop/misc`)
has no tool: 4-6 meaningful groups have to be made by hand.

## What the command does

```text
kaseifu cluster <dir> [--min N] [--max N] [--include-folders] [--recursive] [--execute]
```

- Members are the regular files directly under `<dir>`. Existing subdirectories
  are discovered, reported as `directories left alone`, and never moved.
  `--include-folders` makes the direct child folders members instead;
  `--recursive` collects files from below them while leaving the folders in
  place (the two are refused together: one moves folders, the other walks them).
  Hidden entries are skipped and reported.
- Membership is deterministic (`src/cluster/grouping.rs`): category from the
  extension, then greedy buckets by name-token Jaccard (>= 0.33). A member with
  no usable token groups by type alone. Buckets below `--min` go to the
  ungrouped bucket; buckets above `--max` are split.
- Names are deterministic candidates per group (type name, best shared token,
  two tokens, token+type). The model is asked to choose one per group; it never
  invents a name and never sees or returns a path.
- `--execute` creates `<dir>/<name>` for every group (sanitized, `name (2)` on a
  collision) and moves the members through the #25 journal batch. One batch id
  covers every group, so `undo --batch <id>` restores the flat layout and removes
  the directories it created.

## The API constraint that shaped the naming step

The issue asked Jev to *generate* the abstract names. The published API
(`https://api.typesafe.ai/openapi.json`, TypeSafe 0.2.0) offers `noul`, `choice`,
`score` and `box` questions - **there is no free-text answer**, so a naming
prompt that expects generated text cannot be answered. The implementation
keeps the model's semantic role inside the verified contract: Rust generates
the candidates, and one `choice` question per group (`group_1`, ...) asks the
model which name fits. The response is only `answers.group_<n>.choice`, and a
name is accepted only if it is one of the offered candidates.

This is a deliberate deviation from the issue text and is recorded in
`documents/jev-api.md` ("Cluster naming"). Without a key, without a network, or
with any unexpected answer the group's first candidate (the type name) is used,
so the command always produces a plan; the report says which mode was used
(`naming: jev` / `naming: fallback (type-based names: <reason>)`).

## Other decisions

- **A new module, not the classifier.** `DecisionCandidate` answers "pick one of
  these paths"; the cluster question is "what is this group", so it gets its own
  request/response types and its own boundary (`GroupNamer` trait).
- **One journal batch for the whole run.** Per-group batches would make
  `undo --batch` mean "one group"; the run is one intent.
- **Plain text output, no panel.** The report is a list meant to be read in a
  pipe; a panel wraps long paths mid-word. The format is documented in README
  and `--help`: `cluster: <name> (<count> files, <size>)`, `<n>. <source> ->
  <name>/<file>`, then `ungrouped:` / `directories left alone:` / `skipped:` /
  `naming:` / `mode:` (or `batch:` + `undo reverses the moves`).
- **`--by-type` is the escape hatch for unrelated names.** Real data showed 14
  files that shared no token (six unrelated mockups, hash-named photos, four
  archives): the token pass leaves them ungrouped, but the user wants
  `images/` and `archives/`. The flag skips the token pass and groups by kind
  only; it is off by default because grouping unrelated files together is a
  choice, not a default.
- **`--name-source` separates "which words" from "who chose".** The real-index
  measurement (`~/Desktop/misc`, 37 files) showed the model naming groups
  `japanese` and `rakuichi-rakumon` where `images` and `code` would describe the
  folder's role; the type names were stable. `type` never calls the model,
  `model` requires it (still falling back with the reason), and `auto` keeps the
  previous behavior. The default stays `auto` because naming with the model was
  the issue's requested behavior; flipping it is a product decision recorded
  here rather than a silent change.
- **`--min` defaults to 2, `--max` to 20.** A single file is not a group; a type
  larger than 20 is split so no directory grows without bound.
- **A fallback name that collides takes its group's own token**
  (`images-screenshot`) before a number; a model-chosen name is kept as-is and
  takes `(2)` only when it is already used. Existing folders are never candidates
  and no existing directory is entered.

## Verification (2026-11-08)

- `tests/v122_cluster_test.rs` (new, 13 tests): deterministic dry run on the
  37-file fixture (same bytes twice, nothing moved), execute + `undo --batch`
  (files back, created folders gone, existing folder untouched), pre-existing
  `images` -> `images (2)`, no-key fallback, a stub for the naming call (the
  model's choice replaces the fallback, the request carries only file names),
  `--include-folders`, `--recursive`, `--min`, and the
  `--recursive --include-folders` refusal.
- `cluster::grouping` unit tests: category mapping, token filtering, similarity,
  split/ungrouped, sanitization and candidate order.
- `bash scripts/quality_check.sh`: OVERALL PASS (1067 tests, rustfmt 0, clippy 0,
  CC >= 16 = 0, max CC 15, coverage 85.04%).
- `--by-type` covers the real 14 ungrouped files (mockups, hash-named photos,
  archives) by kind; the default plan is unchanged.
- Real binary, isolated HOME, 37-file fixture: dry run planned 5 groups
  (11 images, 2 images, 10 documents, 4 archives, 5 code), 4 ungrouped, 1
  directory left alone; `--execute` created the 5 folders and moved 32 files;
  `undo --batch` printed 37 restore/remove lines and returned the directory to
  37 flat files with `sub1` untouched.

## Measured naming (2026-11-08, 37-file Desktop folder, `--by-type --min 1`)

| group | type name | model name |
|---|---|---|
| 13 images (mockups, screenshots, photos) | `images` | `japanese` |
| 13 documents (md / pdf / numbers) | `documents` | `agents` |
| 4 archives | `archives` | `assets` |
| 1 csv | `data` | `data` |
| 5 html pages | `code` | `rakuichi-rakumon` |
| 1 extensionless file | `misc` | `misc` |

The type names describe the folder's role and stay stable; the model names
follow the content and drift in abstraction level (a language for images, a
project name for generic pages). The measured run is why `--name-source`
exists, and why the default is a product decision: `auto` (the model when a key
resolves) keeps the behavior issue #26 asked for, while `type` is the
predictable choice and needs no key or network.

## Deliberately not in this issue

- Better names: the candidate set is intentionally small and lexical; a richer
  set (dates, sizes, content excerpts) is a follow-up once the model-side naming
  is measured on real directories.
- `--recursive` flattens the files it finds. Keeping a nested structure would
  need a different execution shape (per-file destination), not this batch.
- `scan` (the unimplemented command named in the issue) stays unimplemented;
  `cluster` is a separate command on purpose.
