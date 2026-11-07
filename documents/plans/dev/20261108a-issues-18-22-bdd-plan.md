# GitHub issues #18-#22: reasons, discovery scope and a folder move path

Last updated: 2026-11-08. Status: implementing.

Source: GitHub issues #21, #20, #18, #19, #22 (smapira/kaseifu, all open on
2026-11-08). The issue bodies are the specification: the
`documents/plans/dev/issues/open/2026....md` files they name do not exist in
this tree, so nothing here is inferred from them.

Out of scope: #16 (waiting for a product decision, "keep as is"), #17 (closed).

Constraints kept by every issue: no new crate dependency (`dirs = "6"` and
`walkdir = "2"` are already in `Cargo.toml`), no function at CC 16 or above
(the complexity gate in `scripts/quality_check.sh`), no test against the real
home directory or the Jev network, `--format json` stays additive, and the
`organize`/`classify` file-only contract plus
`tests/v113_folder_target_test.rs` stay untouched. Dry run stays the default
everywhere and `--execute` stays the only mutation path.

## 1. Order of work

The recommended order is kept: #21 -> #20 -> #18 -> #19 -> #22.

- #21 is a `fix:` and smallest; it also states the vocabulary ("no title could
  be drawn ...", "proposal withheld") that #20 reuses for its LEAVE reasons.
- #20 touches only the organize report and its renderer, so it lands before the
  discovery changes that would otherwise rewrite the same report rows.
- #18 adds the known-directory prior, #19 then adds the path-shape terms that
  consume that prior (the known-directory bonus is #18's vocabulary).
- #22 is independent of discovery and lands last, so a regression there cannot
  hide behind the discovery work.

One commit per issue on the worktree branch (`main`), Conventional Commits:
`fix:` (#21), `feat:` (#20, #18, #22, and #19 which also carries the benchmark
record), `docs:` only if an issue needs nothing else.

## 2. Behavior units (BDD)

### #21 - `label`: a held proposal must name the real cause

Scenario data: fixtures are written under the system temp directory; the
provider is `FakeSemanticProvider`, so nothing needs Apple Intelligence.

| ID | Behavior | Test |
|---|---|---|
| B21-1 | An image whose model title contains `/` (fixture `cbd/common scale.png`, title `cbd/common scale`) is held with a reason that names the model title, and no `path separator` wording. | `v116_a_title_separator_names_the_title` |
| B21-2 | A text file whose title contains `:` behaves the same, so the mapping is not image-only. | `v116_a_document_title_separator_names_the_title` |
| B21-3 | A model answer with no title (image and document) is held as `no title could be drawn from this file`, never `missing: title` or a validator reason. | `v116_an_empty_title_is_held_as_no_title`, `v116_a_document_without_title_is_held_as_no_title` |
| B21-4 | When image perception is unavailable, the image hold says so (`image perception is unavailable (<reason>)`) so "no proposal" and "perception missing" are distinguishable. | `v116_a_held_image_reports_the_missing_perception` |
| B21-5 | A held report carries the perception object when perception ran, and the resolved date source when the date was already resolved (`date_source: "modified"` instead of `""`). | `v116_a_held_plan_reports_what_it_knew` |
| B21-6 | The normal proposals are byte-identical: text `2026-11-07_hello_world.txt` (P02, `modified`), folder `2026-10-19_image_files`, image `{date}_{title}.png` shape. | `v116_normal_proposals_do_not_change` |
| B21-7 | `--format json` keeps every historical key; a held report now fills `perception`/`date_source` (values, no key added or removed). | `v116_json_keeps_its_keys` |
| B21-8 | A genuinely bad *target* name keeps the validator wording (the fix is about the model's title, not about hiding the validator). | `validate::tests::filename_shapes_are_checked` (existing) |

### #20 - `organize`: LEAVE says why nothing was proposed

| ID | Behavior | Test |
|---|---|---|
| B20-1 | LEAVE with zero candidates prints `no candidate destinations were found (... index may not know this file yet ...)` in the rich and plain views. | `v117_zero_candidates_says_so` |
| B20-2 | LEAVE with candidates below the review threshold prints `below the review threshold (...)`, lists at most three candidates (`D1 <dir>`) and names `--select`. | `v117_below_the_threshold_lists_the_candidates` |
| B20-3 | The two messages are different strings for the two data sets (the issue's core ask). | `v117_the_two_leave_reasons_differ` |
| B20-4 | `--explain` adds `Candidates <n>` and `Index <indexed|not indexed|unknown>` rows; the JSON `explanation` object gains `candidate_count` and `target_indexed`. | `v117_explain_reports_candidates_and_index` |
| B20-5 | A REVIEW / AUTO MOVE report is unchanged (no LEAVE notice is printed for a decided verdict). | `v117_a_decided_verdict_keeps_its_rows` |
| B20-6 | `--format json` keeps every historical key; only the `explanation` object grows. | `v117_json_stays_additive` |
| B20-7 | `tests/user-test-plan.csv` carries the two LEAVE scenarios. | review of the CSV |

### #18 - OS standard directories as candidates (prior)

| ID | Behavior | Test |
|---|---|---|
| B18-1 | `user_dirs::known_dirs()` returns only standard directories that exist, as `(UserDirKind, PathBuf)`, from `dirs` (macOS standard paths, XDG on Linux). | `src/platform/user_dirs.rs` module tests |
| B18-2 | The extension/UTI table maps `.pdf/.md/.csv` to Document, `.png/.jpg` to Picture, `.mp4` to Video, `.mp3` to Audio, `.zip/.dmg` to Download; unknown extensions map to nothing. | module test `the_extension_table_covers_the_standard_kinds` |
| B18-3 | A target with no Spotlight candidate and an unindexed target gets the matching standard directory as a candidate with `origin: "prior"`, score 0.0 and a reason naming the standard folder. | `v118_an_unindexed_file_still_gets_the_standard_folder` |
| B18-4 | A standard directory that does not exist adds nothing and never panics (the candidate list is exactly what the search produced). | `v118_a_missing_standard_folder_adds_nothing` |
| B18-5 | Existing candidate IDs and order are unchanged when priors are appended (`D1..Dn` stay, priors continue the numbering). | `v118_prior_ids_continue_after_the_ranked_candidates` |
| B18-6 | `origin` is additive in JSON and is `"spotlight"` for every historical candidate. | `v118_origin_is_additive` |
| B18-7 | Before/after indicators are recorded (`src/benchmark/` fixture ranking). | plan section 6 + `benchmark::candidate_prior` test |

### #19 - Spotlight scope and path-shape terms

| ID | Behavior | Test |
|---|---|---|
| B19-1 | The `mdfind` invocation carries `-onlyin <root>` for every root, home first by default; the argument vector is unit-tested (no process). | `spotlight::tests::the_default_scope_is_the_home_directory`, `...::extra_roots_are_added_once` |
| B19-2 | `--roots DIR` (repeatable) on `organize`, `classify` and `discover` replaces the scope with the named roots; without the flag the home directory is the scope. | `v119_onlyin_reaches_the_search_boundary`, `v119_roots_flag_scope` |
| B19-3 | A directory deeper below the home is penalised (`depth`), so a shallow standard folder wins a tie. | `v119_depth_penalty_orders_otherwise_equal_directories` |
| B19-4 | A directory inside a git working tree (an ancestor holds `.git`) is penalised, so a repository path with equal filename similarity does not outrank a user folder. | `v119_a_worktree_path_does_not_outrank_a_user_folder` |
| B19-5 | A directory that is the standard folder for the target's kind gets the prior bonus (#18 vocabulary). | `v119_the_standard_folder_gets_the_prior_bonus` |
| B19-6 | Penalties never remove a candidate (score floor 0.0) and never reorder ties non-deterministically. | `v119_shape_terms_are_penalties_not_exclusions` |
| B19-7 | Before/after ranking indicators are recorded in this plan. | plan section 6 |

### #22 - `kaseifu move`: a folder move path without a decision

| ID | Behavior | Test |
|---|---|---|
| B22-1 | `kaseifu move <dir> --to <dir>` is a dry run: it prints the plan and moves nothing; `--execute` moves the folder. | `v120_move_is_a_dry_run_by_default`, `v120_execute_moves_a_folder` |
| B22-2 | A same-volume folder move is reversed completely by `kaseifu undo` (tree identical). | `v120_undo_restores_the_folder` |
| B22-3 | The journal row records `kind = dir` (and `file` for a file move); rows written before the column read as no kind (= file). | `v120_the_journal_records_the_kind`, `v120_old_rows_still_read` |
| B22-4 | A copy-then-delete move that fails half way leaves the source untouched and no partial copy behind. | `v120_a_failed_copy_leaves_the_source_and_no_residue` |
| B22-5 | A destination on the never-a-destination list (`~/Library/**`, system trees, caches) is refused, and so is a source inside one. | `v120_an_excluded_destination_is_refused`, `v120_an_excluded_source_is_refused` |
| B22-6 | No decision is involved: no key, no network, no discovery (the command takes the Jev out of the path). | `v120_move_never_touches_the_classifier` |
| B22-7 | `organize`/`classify` keep refusing a folder (their contract is unchanged). | `tests/v113_folder_target_test.rs` (existing, green) |
| B22-8 | Batch undo and old journals keep working (additive `kind` migration). | `v120_undo_batch_still_works` |

## 3. Decisions taken here (and the alternatives rejected)

1. **#21 does not change the validator.** `NameProblem::Separator` is correct for
a name that really contains a separator; what was wrong is that the *model's
title* was validated as if it were the target's name. The planner therefore
checks the title before the template runs and reports it as the model's answer
(`the model's title contains a path separator`), and reports a missing title as
`no title could be drawn from this file` (the wording folders already used).
2. **#21 fills `perception`/`date_source` in held reports instead of adding
keys.** The issue measured `perception: null` and `date_source: ""` on a held
plan; the fields existed, they were simply hardcoded empty because the held arm
was written before the plans carried them. `date_source` is genuinely unknown
when a hold happens before date resolution (injection, missing title), and the
report then keeps `""` - it is never invented.
3. **#20 keeps the report as the single source of truth.** `OrganizeReport`
gains the candidate list it was already resolving a path from, so the renderer
can say "zero" vs "below the threshold" without re-running discovery. The
threshold comes from `Thresholds` (already passed to the renderer), not from a
constant copy.
4. **#18 injects the prior in the pipeline, not in `discovery::discover_with`.**
`discover_with`/`discover_with_outcome` stay byte-identical (their tests pin
behavior), and the production entry points
(`pipeline::discover_candidates_with_options`) append the prior candidates with
IDs continuing after the ranked ones, so a decision that named `D1` still names
the same directory. Priors are only appended when the standard directory exists
and the list is bounded, so nothing about an indexed run renumbers.
5. **#18/#19 keep score semantics separate.** A prior candidate carries score
0.0, zero evidence and `origin: prior`, and it is never merged into a Spotlight
cluster: the classifier sees it as a separate evidence line, and the path-shape
bonus (#19) is the only place where "is a standard folder" raises a *ranked*
directory. Mixing a prior into a Spotlight score would make an unindexed machine
look indexed.
6. **#19 scopes the search at the `mdfind` call, not after it.** Filtering hits
in Rust only ever sees the truncated 10k window, which is exactly what the issue
measured. `-onlyin` is applied while Spotlight searches; `--roots` is a
repeatable flag (a config key would add a tenth surface to
`SUPPORTED_KEYS` for one list, and the issue allows either).
7. **#19 penalises by path shape, never excludes.** Depth, "inside a git working
tree" and "is the standard folder" are the same three facts
`destinations.rs` writes as path shapes; the penalty is capped, floored at 0.0,
and a penalised directory stays a candidate (the issue says "除外ではなく減算").
8. **#22 adds a separate command, as the issue asks.** `organize --to` keeps
refusing a folder: the #14 decision (folders need their own journal, undo,
`--select` and `--to` semantics) is not overturned. `kaseifu move` is the
low-level path: no discovery, no classifier, no key, dry run by default.
9. **#22 moves across volumes with copy -> verify -> delete** (same shape as
`cp` + `rm`), and the undo path reuses the same helper, so a cross-volume move
is reversible too. A failure removes the partial copy and keeps the source; the
journal row stays `failure` with the reason.
10. **#22 records `kind` additively.** Old rows have no column and read as "no
kind", which is `file`: every move before this change moved a file or a folder
through the rename path, and `undo` already decides by `is_file()/is_dir()` at
the recorded path. Nothing rewrites history.

## 4. Phases and verification gates

| Phase | Work | Gate |
|---|---|---|
| P1 | #21 (title reason, held facts, tests) | `cargo test --test v116_label_proposal_reason_test` + `cargo test --lib rename` |
| P2 | #20 (leave reasons, explain rows, CSV) | `cargo test --test v117_leave_reason_test` + `cargo test --test organize_output_test` |
| P3 | #18 (`user_dirs`, prior injection, origin) | `cargo test --test v118_known_user_dirs_test` + `cargo test --test v042c_destination_exclusion_test` |
| P4 | #19 (`-onlyin`, `--roots`, path-shape terms, benchmark) | `cargo test --test v119_spotlight_scope_test` + `cargo test --test discovery_scoring_test` |
| P5 | #22 (`move`, journal `kind`, transfer helper, docs) | `cargo test --test v120_move_command_test` + `cargo test --test v113_folder_target_test` |
| P6 | Whole tree | `bash scripts/quality_check.sh` -> `OVERALL: PASS` |

## 5. Files

| File | Issue | Role |
|---|---|---|
| `src/rename/service.rs` | #21 | Title check before the template, hold facts on `PlanOutcome::Held`, image perception clause |
| `src/main.rs` | #21, #19, #22 | Held report fields; `--roots`; `move` subcommand and its handler |
| `src/rename/validate.rs` | #21 | Unchanged rules, one helper exposed for the title check |
| `src/operations/organize.rs` | #20 | `OrganizeReport::candidates` |
| `src/presentation/renderers/organize.rs` | #20 | LEAVE reasons, top-3 list, `--explain` rows |
| `src/presentation/renderers/explain.rs` | #20 | `candidate_count`, `target_indexed` |
| `src/platform/user_dirs.rs` | #18 | New: `UserDirKind`, `known_dirs()`, `kind_for_extension()` |
| `src/models/candidate.rs` | #18 | `origin` (additive, `spotlight` default) |
| `src/discovery/mod.rs`, `candidates.rs`, `scoring.rs` | #18, #19 | Prior injection, path-shape terms |
| `src/classifier/pipeline.rs` | #18 | Append priors in the production path |
| `src/platform/macos/spotlight.rs` | #19 | `-onlyin` argument builder, `search_in` |
| `src/operations/plan.rs`, `mover.rs`, `undo.rs` | #22 | Directory sources for the manual move path, transfer helper |
| `src/storage/history.rs` | #22 | `kind` column (additive migration) |
| `tests/v116..v120_*` | all | One integration test file per issue |
| `tests/user-test-plan.csv` | #20 | Two LEAVE scenarios |
| `README.md`, `CHANGELOG.md`, `src/skill/kaseifu/SKILL.md` | #20, #22 | The two new behaviors a user reads |

## 6. Benchmark record (required by #18 and #19)

Both issues ask for before/after indicators. The measurement is a fixture
ranking, not the developer's index: a real `mdfind` corpus cannot be used
without touching the machine's home directory and index, which the constraints
forbid. The fixture is the shape the issue measured:

- target: `Vaporwave Japanese Vape Shop Email Mockup.png` (no index entry),
- same-name evidence in a deep repository tree
  (`~/OS/home/Codes/github.com/routeflags/agent-bank/public/system/images/11/email`),
- the same amount of evidence in `~/Pictures/2024`,
- `~/Documents` present but empty of evidence.

| Indicator | Before | After | Issue |
|---|---|---|---|
| candidates for an unindexed target with no hits | 0 | 1 (Picture prior, `origin: prior`) | #18 |
| rank of the repository path vs `~/Pictures/2024` (equal filename evidence) | tie, decided by path asc (repository path wins: `OS` < `Pictures`) | `~/Pictures/2024` first (depth 10 + worktree penalty) | #19 |
| confidence band of a LEAVE run with hits | unchanged | unchanged (penalties change the rank, not the Jev numbers) | #19 |

The numbers themselves are pinned in two module tests so a later coefficient
change is a deliberate one: `benchmark::candidate_prior::the_prior_takes_an_\nunindexed_file_from_zero_to_one_candidate` and
`benchmark::candidate_prior::the_shape_terms_move_the_repository_path_down`.
Both run on a fixture (the issue's directory shape), never on the developer's
index: a real `mdfind` corpus cannot be measured without reading the machine's
index, which the constraints forbid. Coefficients: `DEPTH_BASE_LEVELS 3`,
`DEPTH_PENALTY_PER_LEVEL 0.015` (capped at 8 levels = 0.12),
`WORKTREE_PENALTY 0.18`, `KNOWN_DIR_BONUS 0.10`.

## 7. Open points (not blocking the implementation)

- `--roots` on `watch`: the watcher reuses `organize_file_with_search` with the
default scope; giving it roots needs a watch-config key, which the issue does
not ask for. Left as a follow-up.
- A config-file form of `--roots`: rejected for this change (see decision 6),
worth revisiting if `watch` needs it.
- The `date_source` of a hold that happens before date resolution stays `""`
(both are honest: nothing was resolved yet). If a consumer needs
`"unresolved"`, that is a new key, not a value change.
