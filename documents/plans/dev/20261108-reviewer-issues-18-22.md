# Reviewer record: issues #18-#22

Last updated: 2026-11-08. Reviewer role, applied to the diff
`5dbbbc0..7702afc` (the five issue commits plus the plan commit).

Method: an independent read-only pass over the diff in a separate agent
conversation (no edits, no builds), then the findings below were fixed in a
follow-up commit and re-verified. The reviewer checked each project rule
against `file:line` and reproduced the acceptance criteria by reading the tests.

## Findings

### 🔴 High

1. **`copy_then_remove` deleted the only good copy when removing the source
   failed** (`src/operations/relocate.rs`). `remove_dir_all` unlinks as it goes
   and can fail partway (read-only subdirectory, `uchg`, busy mount); the error
   branch then deleted the *verified* destination, so the files that had already
   been removed existed nowhere. The doc comment and the CHANGELOG claimed the
   opposite, and the branch was untested.
   *Fixed*: the verified copy is kept and the failure is reported as `copied to
   <dest> but the source could not be removed (...); both were left in place`.
   Only a failed copy or a failed verification removes the partial copy. Pinned
   by `relocate::tests::a_source_that_cannot_be_removed_keeps_both_sides`.

### 🟡 Medium

2. **The tests depended on the developer's real standard folders.**
   `pipeline::add_known_dir_priors` called `known_dirs()` with no seam, so every
   test driving the production discovery path picked up this machine's
   `~/Documents`; `phase4_context_test` had been relaxed to `>= 2` to survive it.
   *Fixed*: `pipeline::discover_candidates_with_priors(file, search, options,
   known_dirs)` is the seam; the existing entry point passes the real list.
   `phase4_context_test` passes `&[]` and asserts the exact count again, plus
   that anything after the ranked list is `origin: prior`.

3. **`v118`'s production-path test read the real home.**
   *Fixed*: it now passes an explicit list (and the no-folder case) through the
   seam, so both branches run on every machine.

4. **The `.git` worktree term was not isolated by its tests.** Every fixture
   lived under `target/` inside kaseifu's own working tree, so the term fired for
   both sides of the comparison and the ordering assertion passed on depth alone.
   *Fixed*: `v119_a_worktree_path_does_not_outrank_a_user_folder` asserts the
   reason is present for a checkout and **absent** for a path outside every
   working tree, at equal depth, before it asserts the order.

5. **The zero-candidate LEAVE sentence claimed an index state it did not have.**
   `no candidate destinations found; the index may not know this file yet` was
   printed even when the index *did* know the file (every neighbour unusable) or
   when no search ran at all - the same misreporting #20 exists to fix.
   *Fixed*: three sentences, chosen from `report.target_indexed`
   (`v117_the_zero_candidate_sentence_follows_the_index_state`).

6. **`proposal_problem_reason`'s guard was tautological** (a `Separator`
   problem can only come from `has_separator`), which left the sentence reachable
   from no path and pinned by no test.
   *Fixed*: the guard is gone, and
   `v116_a_separator_from_the_prefix_is_named_as_the_proposal` pins the case where
   the separator comes from a template input (`--prefix 2026/11`) instead of the
   model.

7. **`move` applied the destination predicate to the source.**
   `is_excluded_destination` also refuses hidden and machine-generated names, so
   `kaseifu move ~/Desktop/.env --to ~/Documents/backup` and a
   timestamp-named folder - the folder population #22 measured - were refused as
   `SourceExcluded`, with a sentence that only fit folders.
   *Fixed*: the source uses the tree rules only (`is_out_of_bounds_source`:
   Library trees, system areas, caches, bundles, system roots), and the sentence
   now fits a file and a folder. Pinned by
   `v120_an_excluded_source_is_refused` (a `.cache` tree is still refused).

8. **`--roots` was undocumented** outside the CHANGELOG.
   *Fixed*: one paragraph in the README Spotlight section and the agent-facing
   `SKILL.md` (both copies, byte-identical).

### 🟢 Low

9. **An empty title held for every rename, whatever the template.** A stored
   template that names a file from `{date}`/`{category}` alone does not need a
   title. *Fixed*: the title is checked only when the template uses `{title}`
   (`v116_a_template_without_the_title_does_not_need_one`).
10. **`print_move_report` had two dead parameters** and promised "undo reverses
    the move" in a dry run. *Fixed*: the parameters are gone and the line is
    printed only for a performed move.
11. **`v117_a_rule_hit_never_shows_a_leave_reason` never built a rule hit.**
    *Fixed*: it now builds a real `RuleMatch` and asserts the repo-policy
    section replaced the policy section.
12. **A plan document cited test names that do not exist** (`#19`'s table used
    an older naming). *Fixed* in the plan; the names match the `v11x` tests.

### Recorded, not changed

- `watch` still searches the default scope (`organize_file_with_full`); giving it
  `--roots` needs a watch-config key the issue does not ask for. Stated in the
  plan's open points and in the final report.
- `explanation.target_indexed` duplicates the top-level `target_indexed`. Kept:
  the object is self-contained by design (ZC-5) and both are additive.
- `aggregate_by_directory_with_shape` is at 107 sloc (warn band ≥ 101, CC 7).
  Warn-only in the gate; splitting it is a follow-up, not this change.

## Verified (no change needed)

- No crate dependency added (`Cargo.toml`/`Cargo.lock` untouched in all five
  commits); `dirs`, `walkdir`, `libc` were already there.
- No function at CC 16 or above: the gate reports `CC >= 16: 0`, maximum 15,
  average 2.86.
- `--format json` stayed additive: no key removed or re-typed in the whole diff;
  `origin` is `serde(default)`, `candidate_count`/`target_indexed` appear only
  when a caller carries candidates, and the held report only fills fields that
  already existed.
- The file-only contract of `organize`/`classify` is untouched
  (`plan.rs`/`mover.rs` unchanged, `require_regular_file` still first), and
  `tests/v113_folder_target_test.rs` is green plus a new end-to-end refusal test.
- No new test spawns `mdfind`/`mdls` against the real index or reaches the
  network; every binary-level test sandboxes `HOME`.
- `git diff -w` is byte-identical to the plain diff: no formatting-only churn in
  the five commits, and all subjects are conventional with their issue number.
