# Plan Architect review: issues #18-#22

Last updated: 2026-11-08. Scope: `20261108a-issues-18-22-bdd-plan.md`.

Reviewed against the code as it stands at `5dbbbc0` (v0.13.1): the planner
(`src/rename/service.rs`), the organize report and its renderers, the discovery
chain (`discovery/{mod,candidates,scoring,destinations}.rs` ->
`classifier/pipeline.rs` -> `platform/macos/spotlight.rs`), the move/undo path
(`operations/{plan,mover,undo}.rs`, `storage/history.rs`) and the tests that pin
them.

## 🔴 High

1. **#22 must not widen `plan.rs`/`mover.rs`.** The plan's file table named them,
   but `plan::joined_destination_path` and `mover::verify_preconditions` are the
   *organize* contract (`if !source.is_file()` at `plan.rs:219`, the same check at
   `mover.rs:47`), and `tests/v113_folder_target_test.rs` pins that a folder is
   refused there. Widening them is exactly the change #14 decided against.
   *Adjustment*: a new `src/operations/relocate.rs` owns the judgement-free path
   (`plan_relocate`, `execute_relocate`, `transfer`), the organize path is
   untouched, and `undo` switches its single `std::fs::rename` to the shared
   `transfer` helper so a cross-volume move stays reversible.

2. **#18's prior must not be injected in `discovery::discover_with_outcome`.**
   Nine test files drive `discover_with`/`discover_with_outcome` with synthetic
   hits and assert exact candidate lists (`discovery_age_test` len 2,
   `discovery_scoring_test` len 4, `v042c_destination_exclusion_test` len 1 and
   `is_empty()`). A prior whose availability depends on the machine's home
   directory would make those counts machine-dependent.
   *Adjustment*: `discover_with*` stay byte-identical; the prior is appended in
   `pipeline::discover_candidates_with_options` (the production path used by
   `organize`/`classify`/`watch`/`--select`), with IDs continuing after the
   ranked ones so a decision that named `D1` still names the same directory.
   The prior list comes from `user_dirs::known_dirs()` and is injectable, so the
   new test can pass an explicit list instead of the real home.

## 🟡 Medium

3. **#21's `Held` change is mechanical, keep it that way.** `PlanOutcome::Held`
   is constructed at 18 sites and destructured with `..` everywhere in tests, so
   adding fields is safe; adding a `HoldFacts` struct with `Default`/`none()`
   keeps every pre-perception site a one-line edit and makes "not reached yet"
   explicit instead of `None` meaning three different things.

4. **#20's threshold must come from `Thresholds`, not a 0.60 literal.**
   `organize_view::render_panel_with` already receives the effective thresholds
   (ZC-5), so the "below the threshold" sentence uses `thresholds.review_confidence`;
   a second copy would drift from the policy the report states.

5. **#19 needs no signature change.** Every pipeline entry point already accepts
   an injectable `Fn(&str, usize) -> Result<Vec<String>>` search closure, so
   `--roots` can be a capturing closure built in `main.rs`; the default scope
   belongs in `spotlight::search` itself, which `watch`, `--select`, `discover`
   and the pipeline all call, so the scope is fixed in one place. The argument
   vector is built by a pure function so it can be asserted without `mdfind`.

6. **#19's penalties belong to the directory score, not the file score.**
   `score_file` ranks a single hit (filename/token/extension) and is used by
   `similarity` before any directory exists; depth and "inside a worktree" are
   properties of a directory. The terms therefore land in
   `candidates::ScoredDir`, next to the age adjustment that already modifies a
   directory score, and `aggregate_by_directory*` keep their current signature
   through a new "shape context" parameter on one internal function.

## 🟢 Low

7. **A folder moved into its own subtree must be refused.** Not in the issue, but
   `kaseifu move ~/a --to ~/a/b` with `--execute` would copy a tree into itself;
   `benchmark::is_inside` already answers the containment question.

8. **Help and skill files are pinned.** `v070_zero_config_p1_help_test` walks
   every visible subcommand's help for internal vocabulary, and
   `v109_skill_distribution_test` compares `src/skill/kaseifu/SKILL.md` with
   `.github/skills/kaseifu/SKILL.md`, so the new `move` help text and both skill
   copies must be written together.

## Verdict

Plan approved with adjustments 1, 2 and 6 applied to the plan's file table and
decisions (already reflected in `20261108a-issues-18-22-bdd-plan.md`). No issue's
acceptance criteria were dropped; nothing in the plan contradicts an existing
pattern.
