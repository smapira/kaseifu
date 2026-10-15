# Backlog BDD implementation plan (M1-M4, 8 issues)

Last updated: 2026-10-15. Owner: product-manager (attached subagent, worktree `kaseifu`).

Scope order follows `milestones/20261015a-backlog-milestones.md`. Three M1 items
(`noise-name-overmatch`, `cache-name-refusal`, `model-output-degeneracy`) are
NOT in scope: already implemented in the parent thread (uncommitted in the live
worktree, also present here as unstaged edits to `src/context/directory.rs`,
`src/normalize/noise.rs`, `src/rename/batch.rs`, `src/rename/rules.rs`,
`src/rename/service.rs`). Do not touch those files except where a later issue
requires additive hooks (unicode folds in `batch.rs::folded_name`, OCR text in
`service.rs::plan_image_rename`).

Tool basis: `grep`/`read_file` over `src/main.rs`, `src/presentation/`,
`src/operations/undo.rs`, `src/watch/runner.rs`, `src/semantic/perception.rs`,
`src/semantic/provider.rs`, `src/rename/service.rs`, `src/rename/batch.rs`,
`src/rename/document.rs`, `src/storage/history.rs`, `src/benchmark/perf.rs`,
`src/operations/plan.rs`, `swift/kaseifu_fm_bridge.swift`; `Cargo.toml` dep
scan (no `flate2`, no `unicode-normalization` vendored); network check
(`crates.io` reachable with a UA); `python3 -c "import PIL"` ok, `zlib` ok;
`rust-code-analysis-cli` and `cargo-llvm-cov` installed. Platform: macOS arm64
with `swiftc` 6.3.3, so Swift-bridge and real-device measurements are runnable.

Standing constraints (every issue): ASCII output only, no em dash in new code,
no new config file / DB / cache / external process, no function CC >= 16,
`bash scripts/quality_check.sh` ends `OVERALL: PASS`, `--format json` is
add-only, NO_COLOR and non-TTY work, non-TTY never blocks on input, tests use
isolated HOME + temp dirs only, new integration tests take the next free
`v099+` prefix, no commit/tag/push.

Test prefix allocation (verified `v090`-`v098` taken in `tests/`):

| Prefix | Issue |
|---|---|
| `v099` | `json-for-undo-scan-watch` |
| `v100` | `ocr-image-text` |
| `v101` | `pdf-child-excerpt-quality` |
| `v102` | `rename-to-directory` |
| `v103` | `debug-scenarios-w2-w5` |
| `v104` | `batch-undo-scope` |
| `v105` | `unicode-normalization-conflicts` |
| M4 perf | unit tests inside `src/benchmark/perf.rs` + measured doc numbers |

## M1 - `json-for-undo-scan-watch` (`tests/v099_json_undo_scan_watch_test.rs`)

Contract decision: `undo` prints ONE JSON document; `scan` prints ONE JSON
document; `watch --format json` prints JSON Lines (one object per line, no
wrapping array, because the stream never ends). Warnings/diagnostics go to
stderr in JSON mode (existing contract: stdout is the parseable payload).
Text output is byte-identical to today.

Shapes (all ASCII, `serde_json::to_string_pretty` for the single docs,
single-line `to_string` per watch line):

- `undo --format json`: `{"undo": {"restored": [{"to": str, "undone_move_id": str, "operation_id": str}], "failures": [{"error": str}], "status": "restored|empty|partial|failed", "pending_warning": bool}}`.
  - Given an empty journal, When `undo --format json` runs non-TTY, Then stdout parses and `status` is `empty` and `restored` is `[]`.
  - Given one journaled move, When `undo --format json` runs, Then the file is restored AND stdout parses with one `restored` entry carrying all three keys.
  - Given `--last 2` with one move, Then the second step yields a `failures` entry and `status` is `partial`.
  - Given a pending row, Then stdout still parses and the warning text goes to stderr, not stdout.
  - Existing text (`restored: ...`, `nothing left to undo`, `undo failed: ...`, pending warning on stdout) is unchanged in non-JSON mode.
- `scan --format json`: `{"scan": {"target": str, "status": "not-implemented", "detail": "scanning is not implemented yet"}}`.
  - Given any path, When `scan <path> --format json` runs, Then stdout is one parseable doc with `status` `not-implemented` (honest: scan is still a banner-only placeholder; no fake discovery data).
  - Text mode keeps printing `Scanning: <path>` (non-rich) / the rich banner.
- `watch --format json`: start line `{"watch": {"event": "start", "roots": [...], "mode": "dry-run|execute"}}`, then per event `{"watch": {"event": "detected|ignored|...", "path": str}}`.
  - Given `--format json`, When the start banner prints, Then the line parses as one JSON object (every watch line is exactly one newline-terminated object).
  - Unit scope: new pure helpers (`watch_start_json`, `watch_event_json`) tested directly; the infinite `run_loop_with` is NOT driven in tests (it never returns). Thread the format from `handle_watch`/`print_watch_start` into the loop print sites (`detected`, `ignored`, `log_outcome`).
  - Text mode lines are unchanged.
- Help: `--help` format paragraph becomes `` `json` applies to inspect, classify, organize, doctor, setup, auth, config, completions --install, uninstall, undo and scan. `watch` emits one JSON object per line with --format json. `` Update `tests/v070_zero_config_p1_help_test.rs::format_help_states_which_commands_support_json` to the new sentence (no internal phase words; that test scans all help).
- Non-TTY: `undo --format json </dev/null` and `scan --format json </dev/null` exit 0 without blocking.

## M2 - `ocr-image-text` (`tests/v100_ocr_image_text_test.rs` + unit tests)

Contract decision: `PerceptionInfo` gains `text: Vec<String>` (`#[serde(default)]`, always serialized; add-only). `schema_version` stays `fm-rename-v1`. OCR runs for still images only (not video frames: per-frame OCR would multiply the measured 0.397 s cost; recorded here as the cost-cap decision). Swift uses `VNRecognizeTextRequest` at `.fast` level (cost cap) on the same bounded thumbnail recipe; Rust bounds again (MAX_OCR_TEXTS 8, MAX_OCR_CHARS 120) and keeps ASCII-printable content only (CLI output must stay ASCII; recorded here).

- Given a bridge payload with `"text": ["OPEN", "9:00 - 18:00"]`, When `perception::parse` runs, Then `info.text` holds the bounded fragments and `labels` parsing is unchanged (existing keys untouched).
- Given 20 fragments / 500-char fragment, When parsed, Then at most MAX_OCR_TEXTS survive at most MAX_OCR_CHARS each (bound test).
- Given OCR text containing an injection phrase (`ignore previous instructions`), When `plan_image_rename` runs, Then the proposal is held with `probable prompt injection; proposal withheld` (same reason as file-name hits; untrusted-input rule extended to OCR text).
- Given OCR text present, When the request is built, Then `perception_facts` includes the bounded text (update `PerceptionInfo::is_empty` to cover `text`, extend `FmPerception` with an additive `text` field) and `rename --format json` shows it under `perception` (extend `perception_json` in `main.rs` with additive `"text"` key).
- Real-device evidence: PIL-generated sign image vs blank image; `rename --format json` before/after; record titles + `text` contents. Fake bridge gains OCR text injection for the hold test.
- Swift `vision_perceive` keeps returning the same envelope on OCR failure (empty `text`, labels intact): OCR never fails perception.

## M2 - `pdf-child-excerpt-quality` (`tests/v101_pdf_excerpt_quality_test.rs` + unit tests in `document.rs`)

Decision after recon: no inflate library is available without a new crate, so FlateDecode streams stay opaque (recorded limitation). The fix targets what IS reachable: dictionary/xref noise filtering plus literal/hex string extraction.

- Given an uncompressed PDF with `(Quarterly Report Q3) Tj` inside `<< /Type /Catalog ... >>` dictionaries and an `xref` table, When `extract_signals(Pdf, bytes)` runs, Then signals contain `Quarterly Report Q3` and contain no `/Type`, `Catalog`, `xref`, `endobj` noise (unit test).
- Given hex strings `<48656C6C6F> Tj`, When extracted, Then `Hello` is recovered (hex decode unit test).
- Given PDF escapes `(a\\(b\\) c)`, When extracted, Then `a(b) c` is recovered (escape unit test).
- Given a FlateDecode-compressed content stream, When extracted, Then no garbage signals leak (documents the opaque-stream limitation; signals fall back to whatever literal/hex strings exist outside streams).
- Given a folder of 3 synthetic PDFs, When folder planning runs repeatedly (model non-determinism: 3+ runs), Then collected titles are recorded before/after; acceptance is no type-list title in the after runs (plus the existing degeneracy holds as backstop). Folder evidence flows through the same `extract_signals`, so no separate folder-code change is expected; if the runs show no gain, the fallback is a Swift prompt line preferring child names over type words (record the measured choice).

## M3 - `rename-to-directory` (`tests/v102_rename_to_directory_test.rs`)

- `organize --to <dir>` (standalone; conflicts with `--select` and `--explain`): non-interactive move of the file into `<dir>`.
- Given an existing plain directory, When `organize <file> --to <dir>` runs (dry run), Then it reports the plan (destination directory, no move). With `--execute`, Then the file moves, the journal row is written via the existing `mover::execute_manual_move` path (NULL candidate/confidence/margin like `--select` moves), and `undo` restores it.
- Given an excluded destination (bundle dir, library tree, hidden name - reuse `discovery::is_excluded_destination`), When `--to` points there, Then refusal names the reason and nothing moves.
- Given a missing path / a file (not a dir) / the file's own parent dir (no-op), When `--to` points there, Then refusal with reason, nothing moves (no-op moves are refused, never silently accepted).
- Given `--format json`, When combined with `--to`, Then stdout parses (candidate/plan JSON shape reused from the select/organize views, add-only).
- `SELECT_NON_TTY_GUIDANCE` is reworded to name the now-real `--to <dir>` (it currently promises "a future release will add --to <dir>").
- Non-TTY `organize <file> --to <dir> --execute </dev/null` exits 0 without blocking; help gains no internal phase words.

## M3 - `debug-scenarios-w2-w5` (`tests/v103_debug_scenarios_w2w5_test.rs`, split further if it grows past a readable size)

- For each scenario in W2 (S07,S08), W3 (S14,S27,S21,S22,S23,S24,S25), W4 (S26,S17,S15,S16,S19,S20), W5 (S18,C01-C12,C08): first recheck the design expectation against current code at HEAD; write the test to CURRENT behavior when the design is stale (R5 journal paths fixed by ZC-3, R8 `is_system_directory` removed, C08 completions now implemented).
- Product-code change only if a scenario exposes a real bug; then the fix gets its own new issue file in `open/` and the test pins the fixed behavior.
- Stale-expectation corrections are listed in the acceptance report (which R notes were overridden and why).

## M3 - `batch-undo-scope` (`tests/v104_batch_undo_scope_test.rs`)

Decision (PM, recorded here for the issue file): option (1) - nullable `batch_id` migration + `undo --batch <id>`. Rationale: additive migration (`ALTER TABLE ... ADD COLUMN`, error ignored when the column exists), old DBs keep working (NULL batch), `undo --last N` semantics byte-identical, and batch-atomic undo removes the counting trap the issue names. No new DB file, no `operation` value change.

- Given an old journal without the column, When the new binary opens it and runs `undo --last 1`, Then nothing breaks (migration is best-effort idempotent; unit test builds a legacy-schema DB file and opens it).
- Given one batch apply of 3 renames, When it finishes, Then the 3 rows share one `batch_id` and stdout still shows the `undo --last 3` hint (unchanged) plus the batch id.
- Given `undo --batch <id>`, When run, Then exactly that batch's rows are undone newest-first; an interleaved single rename after the batch is left alone (the case `--last N` gets wrong).
- Given an unknown batch id, When `undo --batch` runs, Then it reports a nothing-to-undo refusal without touching anything.
- `--format json` for `undo` (from the M1 issue) covers `--batch` too (same `{"undo": ...}` shape with the selected rows).

## M3 - `unicode-normalization-conflicts` (`tests/v105_unicode_normalization_test.rs` + unit tests)

Owner decision already recorded (2026-10-15): option (2), `unicode-normalization` crate as a one-issue exception. `cargo add unicode-normalization` (network verified), `Cargo.toml`/`Cargo.lock` change ONLY by this addition.

- Given two proposals differing only by NFC vs NFD, When `hold_conflicts` runs, Then both are held as `Conflict` (unit test with explicit escapes in source).
- Given ASCII-only differing names, When `hold_conflicts` runs, Then behavior is unchanged (no new holds; existing batch tests stay green).
- `folded_name` becomes NFC-normalized then lowercased; the doc comment on `hold_conflicts` is updated (it currently says folding needs a table the tool does not ship).
- Real-device note: create the NFC/NFD pair on the APFS volume and record preview-vs-apply behavior before/after in the acceptance report.

## M4 - `perf-measurement-gaps` (bench extension + measured numbers in docs)

- `kaseifu-bench perf` gains: `--warmup` separation (first availability call + first generation timed apart from the steady samples = cold-start isolation), `--batch-plan N` honest scope (per-item non-generation overhead via walk+plan with a failing-fast path plus a small real generation batch of 3 for peak RSS via `getrusage`; labeled as such, no 50-generation 9-minute runs), larger `folder_entries` values for the sampling curve, and a manual two-process concurrency run recorded in docs (no new harness: the product constraint bans external processes, bench stays single-process).
- Acceptance: new numbers appended to the Phase 9 perf plan doc section 7 follow-up with min/median/max on this machine, bench flags in `--help`.
- Unit tests for any new pure timing helpers.

## Plan Architect review (self-review; no `/plan-architect` skill in this environment, procedure followed directly)

- Reuse check: `--to` reuses `is_excluded_destination`, `checked_destination_path` order, `mover::execute_manual_move`, existing JSON views. OCR reuses `bound_chars`, `contains_prompt_injection`, `perception_facts`, `perception_json`. PDF stays inside `extract_signals`. Unicode touches one function. Batch undo reuses the journal only. Perf stays inside `kaseifu-bench`. No new abstractions invented.
- Risk flags: (a) Swift OCR edit must not change the envelope on failure - mitigated by empty-text fallback + contract tests. (b) `run_loop_with` signature change ripples to existing callers/tests - check callers first, prefer a small bool param. (c) `PerceptionInfo::is_empty` semantic change (text-only counts as non-empty) - intended, covered by tests. (d) `ALTER TABLE` on user DBs - additive nullable, idempotent, tested with legacy file. (e) 50-generation bench too slow - scoped to 3 + overhead split, honestly labeled. (f) watch JSON Lines: multi-line pretty JSON would break line parsing - enforce single-line `to_string`.
- BDD completeness: every issue lists Given/When/Then with concrete data, edge cases (empty journal, pending rows, unknown batch, opaque streams, non-TTY, NO_COLOR), and exact verification commands.

## Verification per issue (all issues)

1. `cargo test --all-targets --all-features` (exit 0).
2. `bash scripts/quality_check.sh` ends `OVERALL: PASS`.
3. Real-CLI spot checks with isolated HOME (`HOME=$TMPDIR/...`) and `</dev/null` for non-TTY, recorded with exact commands and outputs.
4. Issue file moved `open/` -> `done/` with a `## Verification` note (date, test counts, quality-gate result, spot checks) per `issues/README.md` maintenance rules; `issues/README.md` tables and the milestone status table updated in the same change.
