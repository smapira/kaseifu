# Plan Architect review: rename response language (20261016c)

Last updated: 2026-10-16. Verdict: **approved, no blocking issues**.
Advisories below are binding on the implementation round.

## Scope checked

Plan: `documents/plans/dev/20261016c-rename-language-plan.md` (B1-B9, T1-T11).

## Findings

1. **Settings pattern fit.** `UserConfig` + `deny_unknown_fields` + `SUPPORTED_KEYS`
   + `apply_setting` + `validate()` is the single extension path used by every
   prior setting (v3.2 metadata, journal-safety flags). The plan follows it
   exactly. Top-level `language` (instead of a `[rename]` section) is the
   correct call: the mandated CLI key is bare `language`, and a section would
   force either a `rename.language` CLI key (forbidden) or a key/TOML mismatch
   that `config show` could not render honestly.
2. **CC gate.** `apply_setting` sits at the CC ceiling (prior QA: max CC 15).
   The plan's arm-count-neutral delegation (`_` -> `apply_other_setting`) keeps
   its CC identical; the new helpers (`is_language_tag`, `set_language`,
   `effective_language`, `validate_language_value`) are each CC <= 7 by
   construction. No new function may use early-return chains that push past 10;
   keep the flat style of the existing setters.
3. **`ResolvedConfig` extension is safe.** Direct struct construction exists only
   in `src/config/resolver.rs` (verified: `grep "ResolvedConfig {"` hits only
   the struct definition and its single construction site; all tests go through
   `resolve_with`/`resolve_with_env`). Adding `language` breaks no call site.
4. **Rename dispatch seam.** `rename_file` (main.rs:1599) and `rename_batch`
   (main.rs:1694) both call `FoundationModelsProvider::production()` with no
   language; `with_language()` already bounds via `MAX_LANGUAGE_CHARS`.
   Applying `.with_language(effective_rename_language())` at both sites covers
   single-file and `--recursive` paths with no signature change to `production()`
   (examples, bench, and `require_model` keep compiling).
5. **Fallback semantics.** `load_user().unwrap_or_default()` in the rename path
   preserves the current robustness (rename never touches the Keychain and never
   fails on config problems); `effective_language` additionally defends against
   hand-edited invalid tags. This matches the existing "observability over
   strictness" posture of the rename path.
6. **Swift prompt.** Only the three `(for ja, ...)` parentheticals hardcode
   Japanese; the `Answer in <language>` line and the `?? "en"` fallback are
   already general. No test pins the removed parenthetical text (verified:
   `grep "for ja" tests/` is empty; v096 pins only the sentence prefix, which
   the plan keeps). Document prompts carry no language-specific wording, so the
   "byte-identical" claim holds by construction.
7. **JSON compatibility.** `config show --format json` gains one top-level key;
   rename/batch JSON builders are untouched. Additive-only holds.
8. **Bench helper.** `perf.rs::request_json` hardcodes `"ja"` while claiming to
   build "exactly like the product"; switching it to `DEFAULT_FM_LANGUAGE` and
   asserting the constant is the minimal honest fix.
9. **Docs coupling.** v051 pins `config/default.toml` as documentation of real
   settings, so the new top-level `language = "en"` line there is required, not
   optional. README Config section gets the one-line example; no help-text
   change (the v070 help scan pins help output, and discoverability is satisfied
   via `config show`).
10. **Standing constraints respected.** No new crate (Cargo files untouched), no
    new file/DB/process, ASCII-only new text, isolated-HOME tests, new file uses
    the next free `v107` prefix (v106 is the highest in `tests/`).

## Binding advisories for Implementer

- A1: Keep `SUPPORTED_KEYS` append-only at the end; existing key order is
  user-visible in the `unknown setting` error.
- A2: `set_language` must store the trimmed tag; `validate_language_value` must
  validate the trimmed tag (plan B3: `" ja "` stores `ja`).
- A3: New test file must follow the v106 CLI harness pattern (isolated HOME,
  `stdin(null)`, per-command deadline) and must assert the TOML file gains no
  sibling files.
- A4: After edits, grep new/edited lines for non-ASCII (em dash) and run the
  full quality gate; report exact commands and results.
