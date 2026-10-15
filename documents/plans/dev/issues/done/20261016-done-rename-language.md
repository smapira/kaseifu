# Rename response language setting (`config set language <tag>`) — done

- status: done (moved from `open/` 2026-10-16)
- milestone: n/a - user-requested setting (follow-up scope: none)
- next owner: none (verify at merge time)
- blocker: none
- plan: `documents/plans/dev/20261016c-rename-language-plan.md`
- reviews: `documents/plans/dev/issues/20261016-plan-architect-rename-language.md`,
  `documents/plans/dev/issues/20261016-reviewer-rename-language.md`,
  `documents/plans/dev/issues/20261016-qa-rename-language.md`

## Objective (as requested)

Default rename response language is English (`en`); users persist a choice
with exactly `kaseifu config set language <tag>` in the existing user TOML.
No `auth language`, no new file/DB/process/dependency.

## What changed

- `src/semantic/provider.rs`: `DEFAULT_FM_LANGUAGE` `"ja"` to `"en"` + unit test
  (default `en`, `with_language("ja")` override).
- `src/config/loader.rs`: top-level `language: Option<String>` on `UserConfig`;
  `validate()` coverage; `validate_language_value()` + `is_language_tag()` +
  `effective_language()`; `SUPPORTED_KEYS` gains `"language"` (appended);
  `apply_setting` delegates the `_` arm to `apply_other_setting` (arm count and
  CC unchanged); `set_language()` trims then validates.
- `src/config/defaults.rs`: `DEFAULT_LANGUAGE = "en"`.
- `src/config/resolver.rs`: `ResolvedConfig.language` via `effective_language()`.
- `src/main.rs`: single-file and `--recursive` rename dispatch uses
  `production().with_language(effective_rename_language())` (missing/corrupt
  TOML falls back to `en`); `config show` prints `language = <tag>` and adds
  one JSON key (additive-only).
- `swift/kaseifu_fm_bridge.swift`: the three `(for ja, ...)` parentheticals
  generalized to language-neutral wording; `Answer in <language>` kept.
- `config/default.toml` + `README.md`: documented the setting (English default).
- `src/benchmark/perf.rs`: bench request uses `DEFAULT_FM_LANGUAGE`.
- `tests/v090_semantic_provider_test.rs`: default assertion `"ja"` to `"en"`.
- `tests/v107_rename_language_test.rs` (new, 7 tests): default show, set/show
  round-trip, rejection table, unknown-key + legacy TOML, lib-level mapping.

## Verification (2026-10-16, worktree `kaseifu`)

- `cargo test --test v107_rename_language_test`: 7 passed.
- `cargo test --all-targets --all-features`: passed=881, failed=0.
- `bash scripts/quality_check.sh`: OVERALL: PASS (fmt/clippy/test/complexity/
  baseline-gate/report-tests green; CC>=16 = 0, max CC 15; coverage 83.76%
  report-only). No commit/tag/push.
- Live smoke (isolated HOME): fresh show `language = en`; set/show round-trip;
  corrupt TOML shows defaults and rename still previews.
- Known limitation: end-to-end `ja` inference not exercised (on-device tests
  stay `ignore`d); dispatch seam covered by wire tests + inspection.
