# Reviewer acceptance: debug scenarios W0+W1 (20261014h)

Scope: `tests/v091_debug_scenarios_w0_test.rs` (13),
`tests/v091_debug_scenarios_w1_test.rs` (17),
plan `documents/plans/dev/20261014h-debug-scenarios-w0w1.md`.
Method: self-performed acceptance (dedicated reviewer skill unavailable here).

## Verdict: ACCEPT

- [x] All 12 W0+W1 scenarios implemented as automated tests (30 cases), all
      pass on Darwin arm64
- [x] Assertions use `file:line`-grounded observables (error variants, journal
      rows/columns, exit codes, disposition reasons), not DESIGNED copies
- [x] No product-code edits (pure test + docs increment; `git status` shows no
      `src/` modification from this round)
- [x] Isolation respected: in-memory journals, `target/` fixtures with Drop
      cleanup, isolated HOME/cwd CLI children with 20s deadlines, Mock/injected
      Jev only, no `~/Desktop/test/`, no Keychain, no network
- [x] Untouched areas verified: `src/benchmark/`, `src/semantic/`, `swift/`,
      `build.rs`, `schemas/fm-*.json`, `src/bin/` intact; `discover` still
      hidden; no new commands/flags
- [x] No secrets in code/tests/plans; ASCII-only output in tests

## Findings (no product bug requiring a fix)

- The 2 initial failures were test-side grounding errors (plan-time
  validation), not product bugs. No minimal-fix edit was needed: product diff
  is zero.
- User-decision items (unchanged behavior, reported only): R1/R2 no
  auto-recovery, R8 filter now anchored (over-exclusion likely reduced but
  S14 re-measurement stays W3), R9 JSON missing for scan/undo/watch, F-05/F-12
  unwired settings.
