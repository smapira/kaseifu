# QA check: rename response language (20261016c)

Last updated: 2026-10-16. Verdict: **approved** (gate green, no open defects).

## Exact verification commands and results

- `cargo check --all-targets`: clean (one interim error fixed during the round:
  indexing `Bytes` in `is_language_tag`, replaced with `.next().is_some_and`).
- `cargo fmt --all`: applied once (whitespace only), then `violations 0`.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean after fixing
  `field_reassign_with_default` in the new test (struct-update syntax).
- `cargo test --test v107_rename_language_test`: **7 passed** (default show
  text+JSON additive, set/show round-trip + single-file TOML, rejection table +
  TOML identical, unknown-key + legacy TOML, effective-language mapping, tag
  shape, resolved-config language). One interim failure fixed: leading-dash
  values need `--` to reach the validator (clap refuses them first); the raw
  `-en` case is pinned as a safe nonzero-exit rejection.
- Neighbor suites: v090_semantic_provider (11), v090_fm_bridge, v051, v33,
  key_source_cli, key_source, `--lib` (230): all green.
- `cargo test --all-targets --all-features`: **passed=881, failed=0**.
- `bash scripts/quality_check.sh`: **OVERALL: PASS** (rustfmt PASS, clippy PASS,
  cargo-test PASS 881/0, complexity PASS, baseline-gate PASS no new/worsened
  CC>=16, report-tests PASS, coverage PASS report-only 83.76%). Complexity:
  1468 functions, CC>=16 = 0, max CC 15.
- `v070_zero_config_p1_help_test` + `v080_zero_config_finish_test` +
  `completion_test`: 18 + 8 + 5 passed (help/phase-word/completion intact).

## Constraint audit

- ASCII-only new output; no em dash (added diff lines scanned: 0 non-ASCII).
- No new crate dependency; `Cargo.toml`/`Cargo.lock` unmodified.
- No new config file/DB/cache/process (v107 asserts the isolated HOME holds
  exactly one file, the user TOML).
- No function CC >= 16 (gate baseline check green; `apply_setting` arm count
  unchanged by design).
- Tests use isolated HOME + temp dirs only, stdin closed, 10s deadlines.
- New CLI integration file uses the next free prefix (`v107`; highest was v106).
- No commit, tag, or push performed.

## Remaining limitations (not blockers)

- End-to-end rename-in-`ja` inference is not exercised here: on-device inference
  tests are `ignore`d without Apple Intelligence, so the dispatch seam
  (`with_language(effective)`) is verified by unit/v090 wire tests plus
  reviewer inspection of the two call sites.
- Swift prompt changes are verified by inspection + kept prefix assertions
  (v096); the byte-level Swift prompt tests run where the bridge builds.
