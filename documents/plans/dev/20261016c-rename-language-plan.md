# Rename response language (`config set language`) — BDD implementation plan

Last updated: 2026-10-16. Status: planned (pending Plan Architect review).

## 1. Objective

- Default rename response language becomes English (`en`).
- Users change it persistently with the existing settings framework using exactly
  `kaseifu config set language <tag>` (top-level key, no hierarchy).
- Do NOT create `kaseifu auth language` or any new config file, DB, cache, or
  external process.
- Rename single-file and recursive batch paths send the effective language.

## 2. Behavior units (BDD)

Input/output pairs the implementation must satisfy. `<tag>` examples use
`en` and `ja`; invalid examples are quoted inline.

| ID | Behavior |
|---|---|
| B1 | Fresh HOME (no user TOML): `config show` prints `language = en`; `config show --format json` carries `"language": "en"` and keeps every existing key (`jev`, `classifier`, `operations`, `inactive_settings`, `metadata`, `api_key_configured`). |
| B2 | `config set language ja` exits 0 with `Updated language`; the user TOML gains top-level `language = "ja"`; `config show` (text and JSON) reports `ja`. Setting `en` again round-trips. No file besides the existing user `config.toml` is created or modified. |
| B3 | `config set language ""`, a 17+ character tag, and malformed tags (`en_us`, `-en`, `en-`, `en--us`, `e n`, `123`) each fail with a user-facing `invalid language ...` error and a nonzero exit, leaving the TOML byte-identical. Leading/trailing whitespace is trimmed and then accepted (`" ja "` stores `ja`). |
| B4 | A pre-existing TOML without `language` still loads (effective `en`). `config set storage.database x` is still rejected as `unknown setting`. API-key-shaped keys are still refused with the `auth set-key` guidance. |
| B5 | `FoundationModelsProvider::production()` and `with_bridge()` default to `en`; `.with_language("ja")` sends `ja` on the wire (`request["language"]`). `rename_file` and `rename_batch` in `src/main.rs` build the provider with the effective language, so single-file and `--recursive` renames use it. A missing or corrupt TOML falls back to `en` instead of breaking rename. |
| B6 | The Swift prompt keeps `Answer in <language>` and the `language` fallback (`?? "en"`); the three `(for ja, Japanese words; ...)` parentheticals (folder, video-frames, image-content lines) become language-neutral. Document prompts stay byte-identical. |
| B7 | `--format json` stays additive-only: `config show` JSON gains exactly one key (`language`); rename/batch JSON documents are untouched. |
| B8 | No help text gains internal phase words; no new output contains non-ASCII (no em dash); the help-scan test stays green. |
| B9 | Non-TTY safety and dry-run defaults are untouched: rename still previews without `--execute`; config commands work with stdin closed. |

## 3. Design decisions

- **Top-level `language` field on `UserConfig`** (not `[rename] language`).
  Rationale: the required CLI key is exactly `language`, so a 1:1 key-to-TOML
  mapping avoids an alias indirection (`language` CLI key vs `rename.language`
  TOML key) that `config show` could never render honestly.
- **No `RenameCli` flag, no `RenameOptions` field, no `auth language`.** The
  setting is persisted configuration, not a per-invocation override; the
  rename paths read it from the user TOML at dispatch time.
- **Validation shape (BCP47-like):** trim; require 1..=16 chars
  (`MAX_LANGUAGE_CHARS` from the wire contract, referenced not duplicated);
  subtags of 1..=8 ASCII alphanumerics joined by single hyphens; first
  character ASCII alphabetic. Accepts `en`, `ja`, `en-US`, `zh-Hant`.
  Stored trimmed, case preserved.
- **`production()` keeps its signature.** Its built-in default flips `ja` to
  `en` (safe even for paths that forget the setting); the rename dispatch
  applies `.with_language(effective)` explicitly.
- **`config show` renders a bare `language = <tag>` line** (top-level, like the
  TOML) and a top-level JSON `"language"` key. Both are additive.

## 4. File changes

| # | File | Change |
|---|---|---|
| T1 | `src/semantic/provider.rs` | `DEFAULT_FM_LANGUAGE`: `"ja"` to `"en"`. Add unit tests: default sends `en`; `with_language("ja")` sends `ja`. Existing explicit-`"ja"` `build_request` tests stay (explicit passthrough is still valid). |
| T2 | `src/config/loader.rs` | New top-level `language: Option<String>` on `UserConfig` (`skip_serializing_if` none, so unset files stay byte-identical). `validate()` checks it. New `validate_language_value()` + `is_language_tag()` + `effective_language()` (validated tag or `"en"`). `SUPPORTED_KEYS` gains `"language"` (appended last). `apply_setting` keeps its arm count: the `_` arm delegates to a new `apply_other_setting()` which handles `"language"` else `reject_unknown_key` (no new CC >= 16 risk). New `set_language()` trims then validates. |
| T3 | `src/config/defaults.rs` | New `DEFAULT_LANGUAGE: &str = "en"`. |
| T4 | `src/config/resolver.rs` | `ResolvedConfig` gains `language: String` (constructed only here, so no call-site breakage); resolved via `loader::effective_language()`. |
| T5 | `src/main.rs` | `rename_file` and `rename_batch` use `FoundationModelsProvider::production().with_language(effective_rename_language())`. New helper `effective_rename_language()` = `loader::effective_language(&load_user().unwrap_or_default())` (corrupt TOML falls back to `en`). `handle_config_show` adds the text line and the additive JSON key. No help-text changes. |
| T6 | `swift/kaseifu_fm_bridge.swift` | Generalize the three `(for ja, Japanese words; ...)` parentheticals to language-neutral wording; keep the `\(language)` interpolation and the `?? "en"` fallback. ASCII only. |
| T7 | `config/default.toml` | Document `language = "en"` top-level with a `config set language` comment (documentation-only file; v051 pins it mirrors real settings). |
| T8 | `README.md` | Config section: add `kaseifu config set language ja` example plus one line stating English is the default. |
| T9 | `src/benchmark/perf.rs` | `request_json` uses `DEFAULT_FM_LANGUAGE` instead of hardcoded `"ja"` (bench claims to build "exactly like the product"); its language test asserts the constant. |
| T10 | `tests/v090_semantic_provider_test.rs` | `with_bridge` default assertion `"ja"` to `"en"`. Fixture-style explicit `"language": "ja"` JSON (v090 bridge, v093) stays. |
| T11 | `tests/v107_rename_language_test.rs` (new) | CLI integration with isolated HOME + temp dirs, stdin closed, deadlines (v106 pattern): B1 default show (text+JSON additive), B2 set/show round-trip + TOML content + no extra files, B3 rejection table + TOML unchanged, B4 unknown-key rejection + legacy TOML compat, lib-level `effective_language`/`is_language_tag` table. |

## 5. Explicit non-goals

- No `auth language` subcommand; no per-invocation `--language` flag.
- No environment-variable override for the rename language.
- No migration of existing TOML files (missing key means `en`).
- No wire-contract change (`fm-rename-v1` already carries `language`).

## 6. Verification

- `cargo test --test v107_rename_language_test` (new), plus updated
  `v090_semantic_provider_test`, `v051_config_honesty_test`, v033/v032/key-source suites.
- Full `cargo test --all-targets --all-features`.
- `bash scripts/quality_check.sh` ends `OVERALL: PASS` (rustfmt, clippy `-D warnings`, CC gate: no function >= 16).
- Manual grep: no em dash / non-ASCII in new or edited lines; `--help` scan clean.
- Swift prompt wording verified by reviewer inspection (+ macOS-only inference
  tests stay `ignore`d; byte-prefix test in v096 still matches the kept prefix).

## 7. Tool basis

- Read with LSP/file reads: `src/semantic/provider.rs`, `src/semantic/contract.rs`, `src/config/{loader,defaults,resolver}.rs`, `src/main.rs` (rename + config show/set), `src/rename/service.rs`, `swift/kaseifu_fm_bridge.swift` prompt lines, `src/benchmark/perf.rs`, `config/default.toml`, `tests/v090*`, `tests/v051*`, `scripts/quality_check.sh`.
- No external docs needed (no new dependency; BCP47 shape is inline).
