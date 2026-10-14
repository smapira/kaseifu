# Debug scenarios Wave W0+W1 plan (20261014h)

Canon (read-only, do not edit):

- v1.1: `documents/plans/user/20261013b.md` (S01-S20)
- v1.2: `documents/plans/user/20261013c.md` (S21-S28, C01-C12)

Design docs (history, HEAD 6819ac9 = v0.4.2, do not edit this round):

- `documents/plans/dev/20261014d-debug-scenario-design.md` (index, D01-D16, Waves W0-W5, R1-R12, F-01-F-14)
- `20261014d-scenarios-S01-S10.md`, `S11-S20.md`, `cli-scenarios-S21-S28.md`, `cli-scenarios-C01-C12.md`
- `20261014d-existing-test-coverage.md`, `20261014d-cli-coverage-map.md`

Baseline: HEAD `9be5f71` (v0.8.0) + 2 uncommitted increments (TREC bench, AI rename P0-2).
`cargo test --all-targets --all-features` = 568 passed / 0 failed,
`bash scripts/quality_check.sh` = OVERALL: PASS. Machine: Darwin arm64.

---

## 0. Map (W0-W5, this round = W0+W1 only)

| Wave | Scenarios | State |
|---|---|---|
| W0 | S05, S01, S04, S02, S06 (file-operation safety) | implement now |
| W1 | S28, S03, S11, S12, S13, S09, S10 (remaining P0) | implement now |
| W2 | S07, S08 | out of scope (later round) |
| W3 | S14, S27, S21, S22, S23, S24, S25 | out of scope |
| W4 | S26, S17, S15, S16, S19, S20 | out of scope |
| W5 | S18, C01-C12 (bulk), C08 | out of scope; C08 now implementable (see G-02) |

Deliverables:

1. This plan (`documents/plans/dev/20261014h-debug-scenarios-w0w1.md`)
2. 3 issues (`issues/20261014-plan-architect-debug-scenarios-w0w1.md`,
   `20261014-reviewer-debug-scenarios-w0w1.md`, `20261014-qa-debug-scenarios-w0w1.md`)
3. Tests: `tests/v091_debug_scenarios_w0_test.rs`, `tests/v091_debug_scenarios_w1_test.rs`
   (`v090_*` taken, `v091_*` is free)
4. Minimal fixes for real bugs found by scenarios (line is drawn in section 4)

---

## 1. Phase 0: grounding vs current code + drift list

Design docs are v0.4.2 measurements. Re-verified each against current code.
Drift is recorded here, design docs are NOT rewritten.

- G-01 journal (S01/S05): default is now absolute `<app_dir>/kaseifu.db`
  (`src/storage/history.rs:16,28-36`, `journal_path()`). `LEGACY_DATABASE_PATH`
  (`data/kaseifu.db`) is doctor-only, never read/written.
  `src/config/storage.rs:20-30` uses `dirs::data_dir()` (follows HOME), so an
  isolated HOME isolates the journal. R5 resolved via ZC-3. S01/S05
  preconditions are redefined to isolated-HOME or in-memory journals.
- G-02 completions (C08): implemented (`src/main.rs:1669-1701`,
  `Cargo.toml:28` clap_complete v4, zsh install only). Design F-09 "unplanned"
  is resolved. C08 is implementable in W5 (out of this round).
- G-03 `--format json` (R9): `organize` now emits JSON
  (`handle_organize` prints `organize_view::json_report`); `scan`/`undo`/`watch`
  stay text-only. R9 partly resolved. W0/W1 assert no JSON there.
- G-04 watch banner (S28): current shape is `generic::watch_line`
  (`src/presentation/renderers/generic.rs:75-83`), not the design doc string
  `watching <roots> (dry-run: true)`. S28 stdout expectations use the current
  banner. E2E start-banner execution is NOT auto-tested (infinite loop, see
  decision 2).
- G-05 undo pending warning (S05/S13): `print_pending_warning` in `handle_undo`
  surfaces `pending_entries()` + doctor hint, no auto-recovery
  (`src/main.rs`, `src/storage/history.rs:183-197`). R1/R2 "no auto-recovery"
  stays as intended (not reopened). S05 detection assertions include it.
- G-06 new features: `organize --select/--yes/--explain`, `classify --explain`,
  `uninstall`, `completions`. `discover` still `#[command(hide = true)]`
  (`src/main.rs:130`). New commands are out of W0/W1 scope (only
  `--execute`/`--dry-run` `conflicts_with` is used for S28 exit codes).
- G-07 injection points (S02/S06/S28): `classify_with_search` /
  `classify_report_with_search[_and_options]` (`src/classifier/pipeline.rs`)
  allow Spotlight-boundary injection. `organize_file_with_full` has NO search
  injection, so deterministic move tests drive `mover`/`plan`/`undo` directly
  (established `tests/phase5_execute_test.rs` pattern). No live-Jev tests.
- G-08 MockJev (S02): `configured_decision` / `select_unknown` (D99) /
  `select_invalid_path` / `select_nan` / `configured_error`
  (`src/classifier/mock_jev.rs`). D999 case built via `configured_decision`.
- G-09 CLI test method (S28): established pattern `CARGO_BIN_EXE_kaseifu` +
  isolated HOME/cwd + 20s deadline (`tests/v052_zero_config_cli_test.rs:28-80`).
  No new harness invented.
- G-10 scan contract (S21): still banner-only (`src/main.rs:529-544`). Stays W3.
- G-11 mover order (S05/S10/S11): verify, record_pending, rename,
  mark_success/mark_failure (`src/operations/mover.rs:79-147`). Rename failure
  leaves a failure row. S10 failure-row expectation holds.
- G-12 policy boundary (S03): `>=` (`src/policy/decision.rs:76-79`),
  NONE/None leads to Leave (`:51-56`), defaults 0.85/0.40/0.60. 8-row table.
- G-13 watch defaults (S06/S28): `WatchConfig::default` = 500ms / 1s / 10s,
  managed = [journal_path()] (`src/watch/coordinator.rs:32-40`).
- G-14 undo selection (S13): newest `success AND undone_at IS NULL`
  (`src/storage/history.rs:140-154`), `--last` loop with break
  (`apply_single_undo` in `src/main.rs`). 3-branch refinement holds.
- G-15 watch validation order (S28): `is_dir` check runs before key resolution
  (`src/main.rs:761-780`), so CLI validation tests need no key/network.
- G-16 watch failure isolation (S28-12): `process_request` maps observe errors
  to `Failed`, releases in-flight, loop continues
  (`src/watch/runner.rs:39-108`). Testable at library level.

Tool basis: read_file (main.rs, mover.rs, undo.rs, history.rs, decision.rs,
pipeline.rs, coordinator.rs, runner.rs, storage.rs, generic.rs, mock_jev.rs),
grep (journal_path, completions, MockJevIntegration, CARGO_BIN_EXE),
terminal (uname, git log/status, ls tests).

---

## 2. Design decisions (options + basis, one choice each)

1. Expectation grounding/drift: adopt G-01 to G-16. Journals use isolated HOME
   or in-memory; C08 promoted to implementable (W5); scan contract stays.
2. CLI process tests: follow the v052 pattern (binary + isolated HOME/cwd,
   20s deadline, key env vars removed). Infinite-loop starts (`watch` running,
   `doctor --check-connection` network) are NOT auto-tested. S28 covers
   exit-code-determined validation only (no --path: 2, file path: 1, missing: 1,
   --execute --dry-run conflict: 2); start/stop/side-effects go to library
   level (coordinator/runner).
3. Isolated fixtures replace design section 6 real files (`~/Desktop/test/` NOT
   used): `CARGO_MANIFEST_DIR/target` fixtures + Drop cleanup, sha256 compare,
   mode-bit ops, xattr seeding only to stabilize search terms for pipeline use
   (S02). Equivalent because assertions (collision, permission, hash, journal
   rows, exit codes) do not depend on fixture provenance.
4. Bug/fix line: fix only minimal real product bugs with fail-to-pass evidence.
   Structural changes (R1/R2 auto-recovery, R8 filter rework, R9 remainder,
   F-05/F-12 unwired settings) are NOT changed; tests assert the current
   contract and findings go to the report as user decisions.

---

## 3. BDD tasks (behavior units; normal + abnormal + edge)

Legend: `[W0-S05a]` = Wave-Scenario-branch. Files:
`tests/v091_debug_scenarios_w0_test.rs` (W0),
`tests/v091_debug_scenarios_w1_test.rs` (W1).
Fixture rule: `target/v091-fixture-<tag>-<pid>` + Drop cleanup; journals are
in-memory (`OperationHistory::open_in_memory()`).

### W0

- [W0-S05a] interrupt right after record_pending: 1 pending row visible via
  `pending_entries()`, `last_successful_move()` is None (not undoable),
  file unmoved.
- [W0-S05b] interrupt right after rename (manual record_pending + fs::rename):
  file moved, 1 pending row, `last_successful_move()` None. Detection only,
  no auto-recovery (R1 holds).
- [W0-S05c] interrupt before record (execute_move on missing source):
  SourceMissing refusal, 0 journal rows.
- [W0-S05d] undo-side interruption (move ok, manual undo record_pending +
  rename, then mark_success + mark_undone): mid-state has pending undo row and
  original move still undoable; completion never double-restores.
- [W0-S01] move/undo consistency: execute_move then undo_last_move restores the
  original path, sha256 equal at all 3 points, journal has 1 move-success row +
  1 undo-success row, original move undone_at non-NULL.
- [W0-S04] destination collision: same-name different-content destination makes
  execute_move refuse with Collision. Both sha256 unchanged, 0 journal rows.
- [W0-S02a] unknown id via pipeline: injected search (D1/D2 fixed) + D999 mock
  makes `classify_with_search` return Err (rejected decision), no move/journal,
  no fallback.
- [W0-S02b] unknown id units: `validate_decision` rejects D999/path-form/empty
  and allows D1/NONE.
- [W0-S06a] debounce coalescing: multiple same-path events inside default 500ms
  window yield at most 1 processing request.
- [W0-S06b] stability gate: changing fingerprint yields no request; 1s after
  quiescence yields exactly 1.
- [W0-S06c] in-flight + self suppression: re-notify during processing is
  DuplicateInFlight; post-move source/destination events are SelfGenerated and
  never requeued.
- [W0-S06d] missing/dir/managed suppression: Missing / Directory / ManagedData;
  default managed_paths contains current `journal_path()`.
- [W0-S06e] default pinning: `WatchConfig::default()` is 500ms / 1s / 10s.

### W1

- [W1-S28a] CLI arg validation (isolated-HOME child): no --path: exit 2;
  --path file: exit 1 (`watch path is not a directory`); --path missing: 1;
  --execute --dry-run: 2. usage/Error on stderr, no JSON on stdout.
- [W1-S28b] both-path self suppression: after
  `record_self_operation([src, dst])` both submits are SelfGenerated.
- [W1-S28c] failure isolation: `process_request` on a missing path returns
  Failed, releases in-flight, later work still possible.
- [W1-S03] policy boundary, 8 rows: (0.849,0.400) Review, (0.850,0.399) Review,
  (0.850,0.400) AutoMove, (0.950,0.100) Review, (0.600,0.000) Review,
  (0.599,0.000) Leave, (0.999,0.999,NONE) Leave, no destination Leave.
  AutoMove payload is an ID, never a path.
- [W1-S11a] race window, squatter: execute_move refuses Collision, both files
  intact, 0 journal rows.
- [W1-S11b] race window, dest removed: execute_move refuses
  DestinationNotADirectory, source intact, 0 rows.
- [W1-S12] recreated original: after a successful move, recreate same-name
  different-content at source, undo_last_move refuses Collision. Both intact,
  no undo row, original undone_at stays NULL.
- [W1-S13i] double undo, single move: 2nd is NothingToUndo; never restores twice.
- [W1-S13ii] double undo, two moves: 2nd restores the 2nd-newest success move;
  undo row source/destination are the reverse of the chosen move.
- [W1-S13iii] --last 3 loop with 1 success: stops after 1 restore, exactly 1
  undo row.
- [W1-S09] dry-run purity: `mover::dry_run` returns WouldMove, FS and journal
  (0 rows) unchanged. Non-execute never calls execute_move
  (`actually_move = execute && !dry_run` contract from handle_organize).
- [W1-S10] permission refusal: 0o555 dest (skip when root) makes execute_move
  fail Io. Source intact, no dest, 1 failure row, `last_successful_move()`
  None. Library-level Err return stands in for the CLI Move-failed path
  (process does not abort abnormally).

### Out of scope (W2-W5, later rounds)

S07/S08 (W2); S14/S27/S21/S22/S23/S24/S25 (W3); S26/S17/S15/S16/S19/S20 (W4);
S18/C01-C12 (W5). C08 promoted to implementable but not handled now.

---

## 4. Constraints (all rounds)

- No new Rust crate dependencies (dev-deps included). No interactive --select tests.
- `cargo fmt --all -- --check` clean, `cargo clippy --all-targets --all-features
  -- -D warnings` clean. No new CC>=16 functions, no max-CC regression,
  complexity gate PASS.
- ASCII output only (`+--` / `|`), no em dash, no TUI, NO_COLOR / non-TTY
  compliant, no `--format json` keys removed, `discover` stays hidden,
  `KASEIFU_KEY_SOURCE` semantics unchanged.
- No internal type names / phase words / pipeline words in help (no new commands).
- No secrets in code/tests/logs/JSON/plans. Never print `.env` contents.
- Never read/write real `~/Desktop/test/`, real Keychain, real completion dirs,
  real `~/Library/Application Support/kaseifu/`. Never move/delete/rename real
  files. Mock/injected Jev only, no live keys/network.
- Keep all 568 existing tests green. Do not touch `src/benchmark/`,
  `src/semantic/`, `swift/`, `build.rs`, `schemas/fm-*.json`, `src/bin/`.
  Shared files (e.g. `src/main.rs`) change only for minimal bug fixes.
- No commit / push / tag.

---

## 5. Verification

1. `cargo fmt --all -- --check` (0 violations)
2. `cargo clippy --all-targets --all-features -- -D warnings` (0 warnings)
3. `cargo test --all-targets --all-features` (568 + new, 0 failed)
4. `bash scripts/quality_check.sh` (OVERALL: PASS, new fns CC<16)
5. Reviewer self-check: new tests assert `file:line`-grounded observables, not
   `status: DESIGNED` copies.
