# Backlog milestones after AI rename Phase 9

Last updated: 2026-10-21.

Purpose: group the remaining plan issues into milestones and make each issue's status discoverable from the directory path. Status directories are documented in `../issues/README.md`.

## Status summary

| Status | Count | Directory |
|---|---:|---|
| Open | 2 | `../issues/open/` |
| Waiting for user/external resource | 7 | `../issues/waiting-user/` |
| Done | 24 | `../issues/done/` |

## M1 - Reliability and CLI clarity

Goal: reduce confusing user-visible behavior before adding larger features.

| Status | Issue | Exit criterion |
|---|---|---|
| Done | [`generation-timeout`](../issues/done/20261015-done-generation-timeout.md) | One retry and user-facing timeout text are implemented and verified. |
| Done | [`json-for-undo-scan-watch`](../issues/done/20261015-done-json-for-undo-scan-watch.md) | `undo`, `scan`, and `watch` have documented add-only JSON behavior (watch: JSON Lines). |
| Done | [`model-output-degeneracy`](../issues/done/20261015-done-model-output-degeneracy.md) | Filename echoes, child-name echoes, and internal-looking titles hold with user-facing reasons. |
| Done | [`noise-name-overmatch`](../issues/done/20261015-done-noise-name-overmatch.md) | Ordinary names such as `my-target` are no longer silently excluded. |
| Done | [`cache-name-refusal`](../issues/done/20261015-done-cache-name-refusal.md) | Cache/temp refusals are target-specific and visible in batch output. |
| Done | [`git-mv-stale-index`](../issues/done/20261019-done-git-mv-stale-index.md) | `organize` names an index that has no entry for the file yet; destination existence stays a `stat`, so a stale index cannot create a collision. |
| Done | [`organize-keywords`](../issues/done/20261019-done-organize-keywords.md) | `organize --keywords` sends user keywords to Jev as `user:` decision material (never into search), the report states what was sent or that nothing was, and every existing priority (rule, `--to`, `--select`) is unchanged. |
| Done | [`organize-folder-target`](../issues/done/20261021-done-organize-folder-target.md) | `organize` and `classify` refuse a folder at the boundary (exit 1, before the policy, the key and the journal) instead of printing a verdict about it; `label` and `watch` keep their folder/directory shapes. |
| Done | [`skill-coverage-and-exit-codes`](../issues/done/20261021-done-skill-coverage-and-exit-codes.md) | The embedded skill covers repository rules, `--keywords` and the index row; `skill check` reads 0 (match) / 1 (stale) / 3 (no file) so CI can tell drift from "never installed". |
## M2 - Media quality

Goal: improve rename quality from real visual and document evidence.

| Status | Issue | Exit criterion |
|---|---|---|
| Done | [`ocr-image-text`](../issues/done/20261015-done-ocr-image-text.md) | Bounded OCR text is added to perception without opening a prompt-injection path. |
| Done | [`pdf-child-excerpt-quality`](../issues/done/20261015-done-pdf-child-excerpt-quality.md) | PDF-heavy folders no longer degrade to type-list names in measured cases. |
| Waiting-user | [`real-media-quality`](../issues/waiting-user/20261015-waiting-real-media-quality.md) | User-provided dry-run evaluation set is available and summarized without committing private paths or content. |

## M3 - Workflow semantics

Goal: complete workflow affordances and document the exact behavior of multi-item operations.

| Status | Issue | Exit criterion |
|---|---|---|
| Done | [`rename-to-directory`](../issues/done/20261015-done-rename-to-directory.md) | ZC-14 `organize --to <dir>` works non-interactively with undo. |
| Done | [`debug-scenarios-w2-w5`](../issues/done/20261015-done-debug-scenarios-w2-w5.md) | Remaining debug scenarios are rechecked against current code and added as tests. |
| Done | [`batch-undo-scope`](../issues/done/20261015-done-batch-undo-scope.md) | Batch undo gets either a migration-backed command or an explicit documented limitation. |
| Done | [`unicode-normalization-conflicts`](../issues/done/20261015-done-unicode-normalization-conflicts.md) | User chooses dependency exception, in-house limited normalization, or documented limitation. |
| Done | [`repo-policy-file`](../issues/done/20261019-done-repo-policy-file.md) | `organize` honors `.kaseifu.toml` rules (rule name and reason in the proposal) with every existing safety intact. |
| Done | [`repo-policy-example`](../issues/done/20261021-done-repo-policy-example.md) | The documented example captures the site (`content/{project}/sites/{site}/**` into `sites/<site>/content/`) and the prose states what each matcher sees; a `{name}` in a destination is refused instead of becoming a directory name. |

## M4 - Measurement and release hygiene

Goal: keep measured claims and release boundaries reproducible.

| Status | Issue | Exit criterion |
|---|---|---|
| Done | [`perf-measurement-gaps`](../issues/done/20261015-done-perf-measurement-gaps.md) | Cold start, large batch, memory, and concurrency gaps have repeatable measurements. |
| Open | [`build-rs-sdk-probe`](../issues/open/20261016-open-build-rs-sdk-probe.md) | A Command Line Tools build degrades with its reason instead of hard-failing. |
| Open | [`label-guidance-touches-state`](../issues/open/20261016-open-label-guidance-touches-state.md) | The `label` guidance path creates no state file. |
| Done | [`license-file`](../issues/done/20261016-done-license-file.md) | The next tag's tarball carries the MIT license text. |
| Waiting-user | [`bottle-distribution`](../issues/waiting-user/20261016-waiting-bottle-distribution.md) | Phase 2 starts after Phase 1 ships (held 2026-10-16). |
| Waiting-user | [`clean-mac-verification`](../issues/waiting-user/20261016-waiting-clean-mac-verification.md) | Xcode-free machine and clean machine confirm the install failure mode and cost. |
| Done | [`release-tagging`](../issues/done/20261015-done-release-tagging.md) | Tagged `v0.10.0` and pushed to `origin`. |
| Done | [`homebrew-phase1`](../issues/done/20261016-done-homebrew-phase1.md) | The tap install path is verified end to end on CI. |

## M5 - User/external blockers

Goal: track tasks that cannot be completed from the Delta worktree alone.

| Status | Issue | Exit criterion |
|---|---|---|
| Waiting-user | [`trec-corpus-evaluation`](../issues/waiting-user/20261015-waiting-trec-corpus-evaluation.md) | NIST corpus is available and the evaluation direction is chosen. |
| Waiting-user | [`snapshot-cleanup`](../issues/waiting-user/20261015-waiting-snapshot-cleanup.md) | Snapshot is deleted or retained with a recorded reason. |
| Waiting-user | [`local-checkout-pull`](../issues/waiting-user/20261015-waiting-local-checkout-pull.md) | Non-Delta checkout is updated or intentionally left stale. |
| Waiting-user | [`rule-content-matching`](../issues/waiting-user/20261021-waiting-rule-content-matching.md) | User decides whether a rule may match file content, not only the path (recommendation: keep path-only, one rule per project/site pair). |

## Milestone order

1. M1 first: these affect trust in day-to-day output and error handling.
2. M2 next when evaluation material is available: quality changes should be driven by measured cases.
3. M3 in parallel when workflow scope is chosen: it is mostly CLI/test work and schema decisions.
4. M4 after feature decisions settle: it records performance and release boundaries.
5. M5 remains blocked until the user or an external organization supplies the missing input.
