# Issue directory map

Last updated: 2026-10-19.

This directory has two kinds of records:

- Review records in this directory root: plan-architect, reviewer, and QA notes for completed implementation rounds. These stay in the root so old plan references remain valid.
- Backlog issues in status subdirectories:
  - `open/`: actionable without an external blocker.
  - `waiting-user/`: blocked by a user decision, user-provided material, credentials, external agreement, or work outside the Delta worktree.
  - `done/`: completed follow-up issues retained for traceability.

Milestones are tracked in `../milestones/20261015a-backlog-milestones.md`.

## Current backlog by status

### Open

| Issue | Milestone | Next action |
|---|---|---|
| [`20261016-open-build-rs-sdk-probe.md`](open/20261016-open-build-rs-sdk-probe.md) | M4 | Harden the `build.rs` SDK probe so a Command Line Tools build degrades with its reason instead of hard-failing. |
| [`20261016-open-label-guidance-touches-state.md`](open/20261016-open-label-guidance-touches-state.md) | M1 | Stop the `label` guidance path from creating the journal database. |

### Waiting for user or external resource

| Issue | Milestone | Waiting on |
|---|---|---|
| [`20261015-waiting-real-media-quality.md`](waiting-user/20261015-waiting-real-media-quality.md) | M2 | User-provided real media sample set and privacy boundary. |
| [`20261015-waiting-trec-corpus-evaluation.md`](waiting-user/20261015-waiting-trec-corpus-evaluation.md) | M5 | NIST corpus agreement and evaluation-direction decision. |
| [`20261015-waiting-snapshot-cleanup.md`](waiting-user/20261015-waiting-snapshot-cleanup.md) | M5 | Decision to delete or retain the Phase 5b snapshot outside the worktree. |
| [`20261015-waiting-local-checkout-pull.md`](waiting-user/20261015-waiting-local-checkout-pull.md) | M5 | Permission/action to update the non-Delta local checkout. |
| [`20261016-waiting-bottle-distribution.md`](waiting-user/20261016-waiting-bottle-distribution.md) | M4 | Held 2026-10-16: start Phase 2 (prebuilt Homebrew bottles) after Phase 1 ships. |
| [`20261016-waiting-clean-mac-verification.md`](waiting-user/20261016-waiting-clean-mac-verification.md) | M4 | Needs a Mac without Xcode (CLT only) and a clean Mac for the install failure mode and cost. |

### Done

| Issue | Milestone | Result |
|---|---|---|
| [`20261015-done-generation-timeout.md`](done/20261015-done-generation-timeout.md) | M1 | Implemented one retry for generation timeouts and user-facing error text. |
| [`20261015-done-json-for-undo-scan-watch.md`](done/20261015-done-json-for-undo-scan-watch.md) | M1 | Implemented add-only JSON for `undo`/`scan` and JSON Lines for `watch`. |
| [`20261015-done-ocr-image-text.md`](done/20261015-done-ocr-image-text.md) | M2 | Added bounded Vision OCR text to image perception with injection holds. |
| [`20261015-done-pdf-child-excerpt-quality.md`](done/20261015-done-pdf-child-excerpt-quality.md) | M2 | PDF excerpts now decode text strings instead of dictionary noise. |
| [`20261015-done-rename-to-directory.md`](done/20261015-done-rename-to-directory.md) | M3 | Implemented non-interactive `organize --to <dir>` with undo. |
| [`20261015-done-debug-scenarios-w2-w5.md`](done/20261015-done-debug-scenarios-w2-w5.md) | M3 | Ported W2-W5 scenarios as v103/v106 tests; no product change needed. |
| [`20261015-done-batch-undo-scope.md`](done/20261015-done-batch-undo-scope.md) | M3 | Batch undo via `batch_id` migration and `undo --batch`. |
| [`20261015-done-unicode-normalization-conflicts.md`](done/20261015-done-unicode-normalization-conflicts.md) | M3 | NFC/NFD conflicts held via the approved crate exception. |
| [`20261015-done-perf-measurement-gaps.md`](done/20261015-done-perf-measurement-gaps.md) | M4 | Cold start, batch, memory and concurrency measured via bench. |
| [`20261015-done-model-output-degeneracy.md`](done/20261015-done-model-output-degeneracy.md) | M1 | Centralized title-echo holds with user-facing reasons. |
| [`20261015-done-noise-name-overmatch.md`](done/20261015-done-noise-name-overmatch.md) | M1 | Exact-match noise names; `my-target` stays listed. |
| [`20261015-done-cache-name-refusal.md`](done/20261015-done-cache-name-refusal.md) | M1 | Own-name cache refusals, visible held items in batch. |
| [`20261015-done-release-tagging.md`](done/20261015-done-release-tagging.md) | M4 | Tagged `v0.10.0` and pushed to `origin`. |
| [`20261016-done-label-subcommand.md`](done/20261016-done-label-subcommand.md) | n/a (user request) | `rename` subcommand is now `label` (`rename` stays a hidden alias). |
| [`20261016-done-license-file.md`](done/20261016-done-license-file.md) | M4 | Added the MIT `LICENSE` file; the next tag's tarball carries it. |
| [`20261016-done-homebrew-phase1.md`](done/20261016-done-homebrew-phase1.md) | M4 | `brew tap`/`trust`/`install`/`test` verified end to end on CI (v0.12.3). |
| [`20261016-done-rename-language.md`](done/20261016-done-rename-language.md) | n/a (user request) | Rename language defaults to `en`, settable via `config set language <tag>`. |
| [`20261019-done-skill-distribution.md`](done/20261019-done-skill-distribution.md) | n/a (user request) | `kaseifu skill` emits the embedded agent skill and checks copies by sha256. |
| [`20261019-done-git-mv-stale-index.md`](done/20261019-done-git-mv-stale-index.md) | M1 | `organize` discloses an index that has no entry for the file yet; destination existence stays a `stat`. |
| [`20261019-done-repo-policy-file.md`](done/20261019-done-repo-policy-file.md) | M3 | `.kaseifu.toml` rules decide `organize`/`watch` before the classifier; destinations stay inside the workspace. |

## Maintenance rules

- New actionable follow-ups start in `open/` with `status`, `milestone`, and a clear next action.
- Move an issue to `waiting-user/` when the next safe step requires user input, an external resource, or an action outside the Delta worktree.
- Move an issue to `done/` when the implementation or decision is complete, and record the verification or decision that closed it.
- Keep root-level review records stable unless a specific plan reference is also updated.
