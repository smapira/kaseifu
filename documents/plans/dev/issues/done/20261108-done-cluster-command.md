# `cluster`: group a flat folder into named subdirectories - done

- status: done
- completed: 2026-11-08 (GitHub issue [#26](https://github.com/smapira/kaseifu/issues/26); commit `e6c305f`)
- milestone: M3 - Workflow semantics (a new command that files several files at
  once and reuses the #25 journal batch)
- next owner: none (verify at merge time)
- blocker: none

## The finding

`organize` answers "one file into one existing place" and `move` asks the human
for the destination, so a directory with 37 flat files (the issue's
`~/Desktop/misc`) has no tool. `scan` is still unimplemented.

## Decision

- Membership is deterministic: type plus name-token similarity. The model is
  not asked about membership, which is what makes a dry run reproducible.
- **The names are chosen, not generated.** The published API offers `noul`,
  `choice`, `score` and `box` questions and no free-text answer, so the issue's
  "let Jev generate the abstract name" cannot be called. Rust generates
  deterministic candidates and one `choice` question per group lets the model
  pick one. This is a recorded deviation (see
  `documents/plans/dev/20261108c-cluster-command.md` and the "Cluster naming"
  section of `documents/jev-api.md`). Without a key or on any failure the first
  candidate (the type name) is used, so the command always runs.
- Existing subdirectories are reported and never touched; `--include-folders`
  and `--recursive` are the opt-ins, and the two are refused together.
- The whole run is one journal batch (`undo --batch` reverses every group and
  removes the directories kaseifu created, once empty).
- `--by-type` groups by kind only (`images/`, `documents/`, `archives/`) for the
  real case of unrelated one-off files that share no name token; it is off by
  default because merging unrelated files is a choice.
- `--name-source auto|type|model` separates the words from their source: `type`
  never calls the model, `model` requires it (falling back with the reason),
  and `auto` keeps the previous behavior. The default stays `auto`; the
  measured trade-off is recorded in the plan.
- Text output is plain, line-oriented and documented in README and `--help`,
  because the output is read by scripts.

## What changed

- `src/cluster/grouping.rs` (new): `Category`, `Member`, `Group`, the greedy
  similarity pass, candidate names, `sanitize_name`.
- `src/cluster/naming.rs` (new): `GroupNamer`, `TypeNamer`, `JevNamer` (one
  `/v1/systemone` request, one `choice` question per group), per-group fallback.
- `src/cluster/mod.rs` (new): collection (`--recursive`, `--include-folders`,
  hidden entries skipped), planning, unique directory names, execution through
  `relocate::execute_batch_in` under one batch id.
- `src/cli` (in `src/main.rs`): the `cluster` command, handler, text report and
  additive JSON document; the `--format` help names `cluster`.
- `src/classifier/jev.rs`: shared `systemone_url` (the endpoint normalization
  used by both the decision and naming calls).
- `src/operations/relocate.rs`: `execute_batch_in` (a caller-chosen batch id).
- `src/skill/kaseifu/SKILL.md` (and the placed `.github` copy), README,
  CHANGELOG, `documents/jev-api.md`.
- `tests/v122_cluster_test.rs` (new, 9 tests); `tests/v070...help_test.rs`
  follows the help line that now names `cluster`.

## Verification (2026-11-08)

- `tests/v122_cluster_test.rs`: 11 passed (deterministic dry run, execute +
  undo, collision suffix, offline fallback, model choice via a local stub,
  include-folders, recursive, by-type, min, conflicting flags).
- `bash scripts/quality_check.sh`: OVERALL PASS (1067 tests, CC >= 16 = 0,
  max CC 15, coverage 85.04%).
- Isolated HOME, real binary, 37-file fixture: 5 groups / 4 ungrouped / 1
  existing directory left alone; execute moved 32 files and created 5 folders;
  `undo --batch` restored every file and removed all 5 folders. With
  `--by-type --min 1` the 14 unrelated files also land by kind
  (`images`, `archives`, `data`, `misc`).
