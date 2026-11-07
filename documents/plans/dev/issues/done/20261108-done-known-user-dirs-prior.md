# OS standard directories as candidates for an unindexed file - done

- status: done
- completed: 2026-11-08 (GitHub issue [#18](https://github.com/smapira/kaseifu/issues/18); commit `3e3ef49`)
- milestone: M1 - Reliability and CLI clarity (an unindexed file produced no
  candidate at all, which is day-to-day behaviour)
- next owner: none (verify at merge time)
- blocker: none

## The finding

Destination eligibility had only a refusal side (`src/discovery/destinations.rs`:
`~/Library/**`, bundles, caches, system trees). There was no vocabulary for "this
is the standard place for this kind of file", so `~/Documents` and a repository
checkout scored the same, and a file Spotlight had not indexed yet had **no**
candidate at all: 6 measured Desktop files all came back LEAVE, two of them with
`destination_id: NONE` and `target_indexed: false`.

`dirs = "6"` was already a dependency but only `home_dir()` and `data_dir()` were
used; `document_dir()` / `picture_dir()` / `download_dir()` / `video_dir()` /
`audio_dir()` had no call sites.

## Decision

- The standard folder is a **prior**, not a Spotlight score: `origin: prior`,
  score 0.0, `evidence_count: 0`, reason `standard folder for <kind> (prior)`.
  It is never mixed into a directory cluster, so a machine that has no index
  still reads as unindexed instead of pretending a hit.
- It is appended **after** the ranked candidates and continues the ID numbering,
  so every ID a decision can name keeps its meaning.
- Nothing is added when the type maps to no standard folder, when the folder does
  not exist, when it is already a candidate, when it is on the never-a-destination
  list, or when the list is at `MAX_CANDIDATE_DESTINATIONS`.
- One code path for macOS and Linux (the `dirs` crate resolves the macOS folders
  and XDG on Linux), because on Linux this prior is the only way a file becomes
  negotiable without an index.
- `discover()` / `discover_with()` keep their exact numbers (pinned callers); the
  prior is added by the production pipeline and the diagnostic `discover`.

## What changed

- `src/platform/user_dirs.rs` (new): `UserDirKind`, `known_dirs()`,
  `kind_for_extension()` for the types this tool already knows (`.pdf/.md/.csv`
  -> documents, `.png/.jpg` -> pictures, `.mp4` -> video, `.mp3` -> audio,
  `.zip/.dmg` -> downloads).
- `src/models/candidate.rs`, `src/models/decision.rs`: additive `origin` field
  (`spotlight` / `prior`); the default keeps every existing construction site.
- `src/discovery/mod.rs`: `add_known_dir_priors` and
  `add_standard_dir_priors` (over an explicit list), `discover_with_priors`.
- `src/classifier/pipeline.rs`: `discover_candidates_with_priors`, so the
  production path offers the prior while tests inject their own list.
- `src/benchmark/candidate_prior.rs` (new): the before/after indicator.
- `tests/v118_known_user_dirs_test.rs` (new, 6 tests).

## Verification (2026-11-08)

- `tests/v118_known_user_dirs_test.rs`: 6 passed (prior offered for `.pdf`,
  nothing for an unknown extension, a missing folder adds nothing, an existing
  candidate is not duplicated, IDs continue, the list stays bounded).
- `src/benchmark/candidate_prior.rs`:
  `the_prior_takes_an_unindexed_file_from_zero_to_one_candidate` (0 -> 1); the
  numbers are recorded in plan §6.
- Isolated HOME with an `mdfind` stub returning nothing: `.pdf` yields one
  candidate (`~/Documents`, `origin: prior`, score 0.0, evidence 0); an unknown
  extension yields `[]`.
- `bash scripts/quality_check.sh` at `96b946b`: OVERALL PASS (1044 tests).

## Follow-ups (tracked outside this record)

- [`#23`](https://github.com/smapira/kaseifu/issues/23) carries the two
  residual items: the prior's coefficient calibration against a real index and
  a search scope for `watch` / `config` (this record's prior itself is done).
- `watch` does not accept a scope; the prior reaches it through the same
  pipeline.
