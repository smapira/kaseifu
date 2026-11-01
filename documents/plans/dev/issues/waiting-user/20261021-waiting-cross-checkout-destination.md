# Warn on (or refuse) a destination in another working copy of the same repository - waiting for the user

- status: waiting-user
- opened: 2026-10-21 (found 2026-10-19 during the #11 diagnosis)
- github: [#17](https://github.com/smapira/kaseifu/issues/17) (reported 2026-10-21; the discussion and the decision live there)
- milestone: M5 - user/external blockers (the decision is the user's)
- next owner: user
- blocker: a product decision (is a move into another checkout ever intended?)

## The finding

Discovery builds candidates from a machine-wide `mdfind` query (`-onlyin` is
absent, and Spotlight does not return hidden paths), and the plan layer checks
the source and destination shape, the excluded shapes and a collision only
(`operations/plan.rs::checked_destination_path`). Nothing relates the
destination to the source's git working copy, so a file can be planned into
another checkout of the same repository.

Observed 2026-10-19 in `seo-operations`: organizing from
`.delta/worktrees/<id>/...` selected the primary checkout
(`/Users/.../routeflags/seo-operations`) as the destination - the worktree is a
hidden path, so only the visible sibling checkout can be a candidate at all.
The same route is open through `--to` and `--select`.

## What a fix would look like (assessed, not built)

- Walk up from the source and from the destination to a `.git` entry (a
directory, or a file for a worktree or submodule) and stop planning an
automatic move when both exist and differ.
- Render it as REVIEW next to the existing refusals: one
`PlanError::DestinationInAnotherCheckout` variant, one report row and one
add-only `outcome.kind` value. `--to` and `--select` are explicit human
choices, so they would carry the warning instead of being refused.
- No new dependency: the walk is the one `repo_policy::locate_root` already
does, and it costs a few `stat` calls per run.

## Recommendation

Add the REVIEW once the user confirms. The move is recoverable today (`undo`
records both paths), which is why this is warning-shaped rather than a refusal:
some workflows deliberately stage a file into a second checkout.
