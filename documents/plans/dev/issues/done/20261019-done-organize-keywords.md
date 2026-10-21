# `organize --keywords` (user keywords as Jev decision material) - done

- status: done
- completed: 2026-10-19 (user request; delivered as an uncommitted worktree diff,
  no GitHub issue was created this round)
- milestone: M1 - Reliability and CLI clarity
- next owner: none (verify at merge time)
- blocker: none

## Objective (as requested)

```text
kaseifu organize --keywords KEY KEY
```

`organize` accepts `--keywords` (repeatable, multi-value, non-empty values) and
sends every keyword to Jev as decision material for the destination choice.
Keywords ride the existing `state.semantic_signals` wire channel as `user: <KEY>`
entries: no new request field, no change to Discovery's search terms, and a run
without the flag is exactly what it was before.

## What changed

- New module `src/classifier/keywords.rs`: `USER_KEYWORD_PREFIX` (`user: `),
  `KeywordDelivery {provided, sent}` (report data plus its additive JSON and
  text rows), and the `KeywordJevIntegration` boundary decorator that attaches
  the keywords to a cloned `DecisionContext` and records whether `decide` was
  called. `JevIntegration` and every pipeline signature stay unchanged, so
  `organize` and `classify` share one code path.
- `src/models/decision.rs`: `DecisionContext.user_keywords` (`skip_serializing`),
  `with_user_keywords`, `user_keywords()`. Content signals stay separate, so
  the availability-marker filter can never drop a keyword.
- `src/classifier/jev.rs`: `signals_for_jev` appends the prefixed entries
  between the content signals and the availability markers; with no keywords
  the payload is byte-identical to before.
- `src/operations/organize.rs`, `src/classifier/pipeline.rs`:
  `OrganizeReport.keywords` / `ClassifyReport.keywords` (default empty).
- `src/presentation/renderers/organize.rs`, `classify.rs`: a `Keywords` /
  `KEYWORDS` section and one additive `keywords` JSON object, only when the
  flag was given.
- `src/main.rs`: `--keywords` on `Organize` (conflicts with `--select`/`--to`)
  and `Classify`, the trim/non-empty value parser, decorator wiring in
  `handle_organize`/`handle_classify`, `== User keywords ==` in the verbose
  observer, and the additive classify-JSON key.
- `tests/v112_organize_keywords_test.rs` (new, 12 tests) plus
  `keywords: Default::default()` in the five existing report literals.
- Docs: `documents/plans/dev/20261019d-organize-keywords.md` (plan, BDD,
  decisions, review notes), README organize section, CHANGELOG `[Unreleased] /
  Added`, `documents/jev-api.md` mapping-table row.

## Verification (2026-10-19, worktree `kaseifu`)

- `cargo test --test v112_organize_keywords_test`: 12 passed, 0 failed.
- `cargo test --all-targets --all-features`: 955 passed / 0 failed (baseline
  937; +18 = 12 integration tests plus 6 module unit tests). The known
  environmental `v097_cli_never_waits_for_input_without_a_terminal` failure did
  not occur in this run (it passed); no test relies on the on-device model.
- `bash scripts/quality_check.sh`: `OVERALL: PASS` (rustfmt 0, clippy 0,
  cargo-test 955 passed / 0 failed, CC >= 16 = 0 new, max CC 15, coverage 84.67%
  report-only).
- Manual sandbox runs (synthetic repository, isolated HOME, no API key): rule
  hit + keywords text panel (`Sent to Jev  not used (Jev was not consulted)`),
  rule hit + keywords `--format json` (trimmed values, `sent_to_jev: []`),
  `--to` + `--keywords` rejected with exit 2, `--keywords ""` rejected with
  exit 2.
- No commit, push, tag or GitHub write was performed; the result is the
  uncommitted worktree diff. No real user repository, Keychain entry, journal or
  home file was touched by tests (isolated HOMEs, synthetic fixtures, loopback
  mock server, fake `mdfind` on `PATH`).

## Decisions that shaped it

- Jev only, never Discovery (the request says "Jev's decision material"; user
  text in an mdfind query is an injection surface and `normalize_terms` would
  silently drop some keywords).
- Existing wire channel with a `user: ` prefix, not a new field (server
  rejection risk).
- A dedicated `DecisionContext.user_keywords` field mapped by the adapter, not
  entries merged into `semantic_signals` (origin stays visible; the marker
  filter cannot eat a keyword).
- Priority: repository rule wins (keywords reported as not used, key-less run
  still works); `--to`/`--select` conflict at parse time (both decide without
  Jev, and `--select --format json` prints a bare array that cannot carry an
  unused-note).
- `classify` got the same flag on the same code path (read-only preview of how
  keywords move the verdict).
