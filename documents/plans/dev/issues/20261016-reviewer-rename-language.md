# Reviewer check: rename response language (20261016c)

Last updated: 2026-10-16. Verdict: **approved** (no rework).

## What was reviewed

Full working-tree diff for plan `20261016c` (10 modified files + new
`tests/v107_rename_language_test.rs`), plus a live CLI smoke on an isolated
HOME and a corrupt-TOML fallback probe.

## Checks

| Check | Result |
|---|---|
| Objective fidelity | Default is `en`; `config set language <tag>` persists top-level `language` in the existing user TOML; no `auth language`, no new file/DB/process/dependency (`Cargo.toml`/`Cargo.lock` untouched). |
| Rename wiring | `rename_file` and `rename_batch` both build `production().with_language(effective_rename_language())`; `production()` signature unchanged (examples/bench/tests compile). |
| Validation | Empty/overlong/malformed tags rejected with `invalid language ...`; whitespace trimmed; TOML untouched on rejection (pinned by v107). Leading-dash values without `--` are refused by clap parsing itself (nonzero exit, no write); with `--` they reach the validator. |
| Compat | Legacy TOML without `language` loads (means `en`); `storage.database` still `unknown setting`; API-key-via-`config set` refusal untouched. |
| Swift prompt | Exactly the three `(for ja, ...)` parentheticals generalized; `Answer in <language>`, `?? "en"` fallback, and all data/instruction lines unchanged; document prompts byte-identical (no title line on that path). |
| JSON | `config show --format json` gains one top-level `language` key; rename/batch JSON builders untouched. |
| Help/ASCII | No help-text change (v070/v080/completion suites green); zero non-ASCII characters in added lines (scanned the added diff lines); no em dash. |
| Non-TTY/dry-run | No change to preview/execute gating; corrupt TOML falls back to `en` in both `config show` (pre-existing `unwrap_or_default`) and rename (verified live: rename proceeds instead of failing). |
| Minimal diff | No unrelated refactoring; `perf.rs` change is the honest minimum (it claims to build "exactly like the product"); only default-pinning assertions updated (`with_bridge` default in v090), explicit-`ja` fixtures left alone. |

## Live smoke (isolated HOME, `KASEIFU_KEY_SOURCE=env`, stdin closed)

- Fresh `config show` prints `language = en`; `config set language ja` prints `Updated language` and `config show` prints `language = ja`.
- Corrupt TOML: `config show` exits 0 on defaults; `rename <file>` (dry run) proceeds and previews instead of failing.

## Note for QA

Model-dependent inference stays `ignore`d on this machine; the wire assertion
for the default/override pair is covered by unit + v090 tests with the fake
bridge, and the dispatch seam is two identical one-line call sites inspected
above.
