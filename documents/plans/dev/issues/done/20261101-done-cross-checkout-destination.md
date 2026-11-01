# A destination in another working copy of the same repository - done

- status: done
- completed: 2026-11-01 (GitHub issue [#17](https://github.com/smapira/kaseifu/issues/17); found 2026-10-19 during the #11 diagnosis)
- milestone: M1 - reliability and CLI clarity (moved from M5: the decision landed - automatic moves REVIEW, explicit `--to`/`--select` warn - so the remaining work was implementation and verification inside the worktree, not a user blocker)
- next owner: none (verify at merge time)
- blocker: none

## The finding

Discovery builds candidates from a machine-wide `mdfind` query (`-onlyin` is
absent, and Spotlight does not return hidden paths), and the plan layer checked
the source and destination shape, the excluded shapes and a collision only
(`operations/plan.rs::checked_destination_path`). Nothing related the
destination to the source's git working copy, so a file could be planned into
another checkout of the same repository.

Observed 2026-10-19 in `seo-operations`: organizing from
`.delta/worktrees/<id>/...` selected the primary checkout
(`/Users/.../routeflags/seo-operations`) as the destination - the worktree is a
hidden path, so only the visible sibling checkout could be a candidate at all.
The same route was open through `--to` and `--select`.

## Decision

An automatic move (`classify`, `organize`, `watch`) is REVIEW; an explicit
`--to` / `--select` is allowed with one warning line. The reason is the one that
already governed rule destinations: `.kaseifu.toml` may not move a file outside
its workspace (`DestinationOutsideWorkspace` -> REVIEW), and an automatic move
is only taken where it is deterministically safe. A path a human named is not a
guess, and `undo` reverses the move (the journal holds both paths), so refusing
it would remove a workflow the issue says some people use on purpose.

The identity is the **git common directory**: a plain checkout's `.git`
directory, a linked worktree's `commondir` target, a submodule's own git
directory. Two clones of one origin are deliberately *not* compared by their
remote URL: the same URL legitimately appears for an intentional second clone.

## What changed

- `src/operations/checkout.rs` (new): `GitRoots { root, common_dir }`,
  `git_roots` (walk up to `.git`, resolve the directory form and the
  `gitdir:`/`commondir` file form), `git_common_dir`, and
  `other_working_copy` (same common directory, different root). No process is
  spawned; the walk is the one `repo_policy::locate_root` already does.
- `src/operations/plan.rs`: `PlanError::DestinationInAnotherCheckout(PathBuf)`;
  `build_move_plan_with` reports it before the collision check, so the deeper
  reason wins (#11). `build_manual_move_plan` and `build_rule_move_plan` keep
  their behavior - the first is a human decision, the second is confined to its
  workspace.
- `src/presentation/renderers/organize.rs`: the variant renders under the
  existing REVIEW heading with one sentence, and `outcome.kind` becomes
  `destination_in_another_checkout` (add-only; the exit code stays 0 like every
  REVIEW).
- `src/presentation/renderers/select.rs` and `src/main.rs`: the manual plan and
  the moved report carry one ASCII warning line (`Warning: destination is
  another working copy of the same repository (<root>); undo reverses the
  move`), and `--to --format json` adds one `organize_to.checkout` object
  (`another_working_copy`, `root`, `notice`) only when the case applies.
- `tests/v114_cross_checkout_destination_test.rs` (new, 13 tests) plus 6 module
  tests in `checkout.rs`.
- `README.md`, `CHANGELOG.md`, the dev brief
  `../20261101a-cross-checkout-destination.md`.

## Verification

- `cargo test --test v114_cross_checkout_destination_test`: 13 passed / 0 failed.
- `cargo test --lib checkout`: 6 passed / 0 failed.
- `cargo test --all-targets --all-features`: 981 passed / 0 failed / 5 ignored
  (0.13.0 baseline 962; +19).
- `bash scripts/quality_check.sh`: `OVERALL: PASS`.
- Manual sandbox (isolated HOME, no key, stdin closed): the dev brief records
  the commands and results.

## What the fix did not change

- `--format json` keeps every existing key and type; a run without the case
  prints the same document as before.
- A rule move inside its workspace is still `would_move`, and a collision inside
  one working copy is still `destination already exists`.
- Submodules and separate clones are still allowed: their common directories
  differ, so they are different repositories.
