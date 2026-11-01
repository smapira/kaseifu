# A destination in another working copy of the same repository

Last updated: 2026-11-01. Status: implemented and verified.

Source: GitHub issue #17 (2026-11-01), found 2026-10-19 while diagnosing #11.
Follows `20261019b-git-mv-stale-index.md`, which recorded the same observation
and left it as a product decision.

## 1. Objective

Stop an automatic move from putting a file into a different checkout of the
repository it belongs to, and say so either way:

- an automatic move (`classify`, `organize`, `watch`) whose destination belongs
  to another working copy of the source's repository is **REVIEW**, not a move,
- `--to` and `--select` stay **allowed** (a human named the path, and `undo`
  reverses the move) and carry one warning line plus an additive JSON key,
- the decision is taken from the filesystem alone: no `git` process, no new
  dependency, a few `stat` calls per run.

Constraints kept: dry run stays the default, `--execute` stays the only mutation
path, journal and `undo` semantics are untouched, `--format json` stays additive
(no existing key removed or retyped), no new crate, CC stays below 16, output
stays ASCII.

## 2. Behavior units (as implemented and verified)

| ID | Behavior | Verified by |
|---|---|---|
| B1 | A destination in the *same* working copy is planned as before (`would_move`). | `v114_a_destination_in_the_same_working_copy_still_moves`, `v114_a_rule_move_inside_the_worktree_still_moves` |
| B2 | A destination in another working copy of the same repository is `PlanError::DestinationInAnotherCheckout`, rendered as REVIEW with `destination is in another working copy of the same repository: <dir>` and `outcome.kind = destination_in_another_checkout`, exit 0, and `--execute` moves nothing. | `v114_the_primary_checkout_of_a_worktree_is_another_working_copy`, `v114_organize_reviews_a_cross_checkout_destination`, `v114_execute_moves_nothing_when_the_destination_is_another_checkout`, `v114_the_checkout_review_renders_as_review` |
| B3 | A second clone (`.git` directory elsewhere) is a different repository and stays allowed. | `v114_another_clone_is_a_different_repository` |
| B4 | A submodule (`.git` file with `gitdir:` and no `commondir`) is its own repository and stays allowed. | `v114_a_submodule_is_a_different_repository` |
| B5 | A destination outside git, or whose nearest `.git` is another repository, changes nothing. | `v114_a_destination_without_its_own_checkout_is_allowed`, `outside_git_there_is_no_working_copy` (module) |
| B6 | Another checkout plus a same-name file reports the checkout, not `destination already exists`; a collision inside one checkout is still a collision. | `v114_a_collision_does_not_hide_the_cross_checkout_reason` |
| B7 | `--to` is allowed, prints one ASCII warning line, and `--format json` adds one `checkout` object only when the case applies. | `v114_a_manual_plan_allows_another_checkout`, `v114_to_warns_and_still_moves` |
| B8 | `--select` renders the same line through the shared select views. | `v114_the_select_plan_shows_the_same_notice` |
| B9 | A rule move inside its workspace is still `would_move` (and moves with `--execute`, journaled). | `v114_a_rule_move_inside_the_worktree_still_moves` |
| B10 | The helper resolves the directory form, the `gitdir:` file form, a relative `commondir`, the nearest entry, and "outside git". | 6 module tests in `src/operations/checkout.rs` |

## 3. Decision, and the alternatives that were rejected

Decision (as settled in the issue, not re-litigated here): automatic moves
REVIEW, explicit `--to`/`--select` warn and stay allowed. It mirrors the rule
destination boundary (`.kaseifu.toml` cannot leave its workspace ->
`DestinationOutsideWorkspace` -> REVIEW): an automatic move is only taken where
it is deterministically safe, and a path a person typed is a decision, not a
guess.

The identity is the **git common directory** (`git rev-parse --git-common-dir`
without the process):

- `.git` is a directory -> that directory is the common directory,
- `.git` is a file -> the first `gitdir: <path>` line; `<gitdir>/commondir`,
  when present, holds the common directory relative to `<gitdir>`, otherwise
  `<gitdir>` itself is the common directory,
- "another working copy" = same common directory **and** different root (the
  directory holding `.git`),
- comparison on normalized paths (`benchmark::normalize_path`), so symlinked
  spellings of one directory agree.

Rejected alternatives:

1. **Compare `.git/config` remote URLs.** Two clones of one origin are
   legitimately separate checkouts (a worktree for a second branch, a staging
   copy), and the same URL is what an intentional clone *looks* like; the check
   would refuse moves that are exactly what the user meant.
2. **Refuse `--to`/`--select` too.** The move is reversible (`undo` records both
   paths) and the issue asks whether the workflow exists; refusing would remove
   it. A warning keeps the workflow and still discloses the destination.
3. **A new configuration key to allow the case.** One more setting for a
   one-line disclosure; the explicit flags already are the opt-in.
4. **`git rev-parse --git-common-dir` as a process.** A subprocess per plan (or
   per path) for information three `stat` calls and two small file reads give.
5. **Merge the check into `checked_destination_path`.** That helper is shared
   with the rule builder, and rules must keep their current behavior; the check
   is also ordered *before* the collision, which that helper's order does not
   express.

## 4. Files

| File | Role |
|---|---|
| `src/operations/checkout.rs` | New. `GitRoots`, `git_roots`, `git_common_dir`, `other_working_copy`, plus 6 module tests. |
| `src/operations/mod.rs` | Registers the module. |
| `src/operations/plan.rs` | `PlanError::DestinationInAnotherCheckout(PathBuf)`; `joined_destination_path` (shape checks + joined path) so `build_move_plan_with` can report the checkout before the collision. Rule and manual builders unchanged. |
| `src/presentation/renderers/organize.rs` | REVIEW heading, `plan_error_message_with` sentence, `outcome.kind = destination_in_another_checkout`. |
| `src/presentation/renderers/select.rs` | `cross_checkout_notice` + the optional notice on the four manual views. |
| `src/main.rs` | `other_checkout_root`, the notice on `--to`/`--select`, the additive `checkout` object in `organize_to_json`. |
| `tests/v114_cross_checkout_destination_test.rs` | New, 13 tests (see B1-B9). |
| `documents/plans/dev/issues/done/20261101-done-cross-checkout-destination.md` | Issue record (moved from `waiting-user/`), M5 -> M1 reason. |
| `documents/plans/dev/issues/README.md`, `documents/plans/dev/milestones/20261015a-backlog-milestones.md` | Status tables and counts (Waiting 8 -> 7, Done 24 -> 25). |
| `README.md`, `CHANGELOG.md` | One organize subsection and one `### Fixed` entry. |
| `src/skill/kaseifu/SKILL.md` (+ the byte-identical `.github/skills/kaseifu/SKILL.md`) | The moving-a-file section states the REVIEW and the `--to`/`--select` warning. |

## 5. Verification

| Command | Result |
|---|---|
| `cargo test --test v114_cross_checkout_destination_test` | 13 passed / 0 failed |
| `cargo test --lib checkout` | 6 passed / 0 failed |
| `cargo test --all-targets --all-features` | 981 passed / 0 failed / 5 ignored (baseline 962; +19) |
| `bash scripts/quality_check.sh` | `OVERALL: PASS` |

Manual sandbox `$HOME/kaseifu-v114-manual` (a primary checkout plus a linked
worktree, hand-written `.git` file and `commondir`; isolated `HOME`,
`KASEIFU_KEY_SOURCE=env`, `KASEIFU_NO_COMPLETION_UPDATE=1`, stdin closed, ASCII
`NO_COLOR`), removed afterwards:

| Run | Result |
|---|---|
| `organize <worktree>/inbox/note.md --to <primary>/notes` | exit 0; `Warning: another working copy of the same repository (<primary>); undo reverses the move` as the last line; source untouched |
| the same with `--format json` | `organize_to.checkout = { another_working_copy: true, root: <primary>, notice: ... }` next to every historical key |
| the same with `--execute` | the file lands in the other checkout; `kaseifu undo` prints `restored: <worktree>/inbox/note.md` and the file is back |
| `--to <worktree>/filed` (same checkout) | exit 0, no warning line, no `checkout` key |
| `organize <worktree>/inbox/note.md` with a rule `inbox/** -> filed` | `outcome.type = would_move`, rule named |
| `organize ... --execute` against a local mock Jev (`127.0.0.1`, fake `mdfind`) | exit 0, `policy = AUTO MOVE D1`, `outcome.kind = destination_in_another_checkout`, nothing moved, and the text view prints `== REVIEW ==` with the sentence |

No user file, journal entry, Keychain item or completion directory on the
machine was involved: every run used the sandbox `HOME`, and the classifier path
talked only to `127.0.0.1`.

Residual risk: the check reads only the `.git` entry, so a checkout that was
copied without its `.git` (an export, a tarball) is a different repository; that
is the honest answer with the evidence available, and such a destination is
allowed exactly as before.
