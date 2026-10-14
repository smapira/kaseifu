# Plan Architect review: debug scenarios W0+W1 (20261014h)

Target: `documents/plans/dev/20261014h-debug-scenarios-w0w1.md`
Method: self-performed review against the codebase (dedicated plan-architect
skill is not available in this environment; the checks below were run
directly: read_file on policy/mover/undo/history/pipeline/coordinator/runner/
storage/generic/mock_jev, grep on journal/completions/injection/CLI-harness,
terminal for baseline and OS).

## Verdict: APPROVE with 2 corrections applied during implementation

1. Plan-time validation exists in BOTH layers. `build_move_plan` refuses
   SourceMissing/Collision at plan time (`src/operations/plan.rs:78-100`), not
   only `execute_move` at verify time. Tests S05c/S04/S09-collision were
   rewritten to assert the plan-level refusal (same contract: refusal +
   intact files + 0 rows); the execute-level race stays with S11a/S11b.
   First test run caught this (2 failures), fixed, now 30/30 green.
2. `w0_s02a` macOS gate removed. `context::builder::build` degrades
   gracefully off-macOS (mdls/xattr failures become empty evidence;
   `src/context/builder.rs:49-51`), and the search boundary is injected, so
   the test is portable. This also avoids unused-import warnings on other
   platforms.

## Checklist

- [x] Behavior-scoped tasks (30 tests, one behavior each; W0: 13, W1: 17)
- [x] No new harness invented (phase5 mover pattern, phase4 search injection,
      phase6 coordinator driving, v052 CLI child pattern)
- [x] Fixture paths survive current exclusion rules (anchored system-root
      check, hidden-ancestor allowed, `target/` not a cache name; precedent:
      phase4 pipeline tests pass on the same root)
- [x] CLI tests limited to exit-code-determined cases (no infinite loops, no
      network); key/network-free ordering verified (`src/main.rs:761-780`)
- [x] Scope discipline: no W2-W5, no product-code changes, no new deps
- [x] Drift noted, not fixed: R8 partial-match filter is gone from
      `similarity.rs` (anchored check in `destinations.rs` now); R5 resolved;
      C08 implementable; R9 partial; watch banner shape differs from design doc

## Residual risks for later rounds

- `watch` start/stop E2E and `doctor --check-connection` stay manual
  (infinite loop / network).
- `organize` has no search-injection entry; E2E-through-organize tests would
  need real Spotlight index state and are intentionally not added.
