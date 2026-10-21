# `organize --keywords KEY KEY`: user keywords as Jev decision material

Last updated: 2026-10-19. Status: implemented and verified (reviews in section 6).

## 1. Objective

The user asked for:

```text
kaseifu organize --keywords KEY KEY
```

`organize` (and its read-only twin `classify`) accepts `--keywords`, repeatable
and multi-value, and hands every keyword to Jev as **additional decision
material** for the destination choice. Keywords are user input, not derived
signals: they ride the existing `state.semantic_signals` wire channel as
`user: <KEY>` entries, no new request field is introduced (an unverified field
carries server-side rejection risk), and nothing else about the flow changes.
Dry run stays the default, `--execute` stays the only mutating path, and a run
without `--keywords` is byte-for-byte what it is today.

What the flag is NOT: it is not a search term. Discovery's `mdfind` query is
built from the normalized file context and never sees a keyword (decision D1),
so the candidate set a user knows does not shift because they annotated one
run.

Evidence read before planning (`read_file`/`grep` on actual sources):
`src/classifier/jev.rs:301` (`SystemOneRequest`/`StateDto` wire shape),
`src/classifier/pipeline.rs` (`decide_from_candidates` builds the
`DecisionContext`), `src/models/decision.rs` (`semantic_signals` is
`skip_serializing`; the adapter maps it), `src/discovery/search_terms.rs`
(`extract_terms` feeds the mdfind query), `src/operations/organize.rs` (rule
check before Jev; `OrganizeReport`), `src/presentation/renderers/organize.rs`
and `classify.rs` (section/JSON construction),
`documents/jev-api.md` (verified `/v1/systemone` mapping table),
`tests/v070_zero_config_p1_cli_test.rs` (CLI fixture with fake `mdfind` + mock
HTTP Jev server), `tests/v111_repo_policy_test.rs` (rule-wins fixtures),
`scripts/quality_check.sh` (CC >= 16 baseline gate).

## 2. Behavior units (BDD, as planned and verified)

| ID | Behavior |
|---|---|
| B1 | `organize <file> --keywords KEY KEY` sends every keyword to Jev: the captured `/v1/systemone` request carries `state.semantic_signals` entries `user: KEY KEY` (prefix `user: `), and the request body gains **no** new top-level field (`"keywords"` never appears as a request key). |
| B2 | Both spellings work: `--keywords a b` (one flag, several values) and `--keywords a --keywords b` (repeated flag) accumulate into one list. |
| B3 | Without `--keywords` nothing changes: no `keywords` key in `--format json`, no `user: ` entry on the wire, no `Keywords` section in text - the no-flag regression is pinned by absence assertions. |
| B4 | Keywords never reach Discovery: with an injected search boundary, no captured mdfind query contains a keyword (or the `user: ` prefix); the candidates come from the file context exactly as before. |
| B5 | Empty and whitespace-only values are rejected at parse time: `--keywords ""` and `--keywords "   "` exit 2 with `keyword must not be empty`; values are edge-trimmed (`" invoice " -> invoice`). |
| B6 | A repository rule wins: with `.kaseifu.toml` deciding the file, `organize <file> --keywords a` exits 0 **without an API key**, reports the rule, and the report says the keywords were not used (`keywords.sent_to_jev: []`, text `not used (Jev was not consulted)`); no classifier call happens. |
| B7 | `--to` and `--select` conflict with `--keywords` at parse time (exit 2, `cannot be used with`): both paths never consult Jev, and `--select --format json` prints a bare candidate array that cannot carry an add-only report key. |
| B8 | The report says what Jev received: text/panel gain a `Keywords` section with `Provided` and `Sent to Jev` rows iff the flag was given; `--format json` gains exactly one add-only key, `keywords: {provided: [...], sent_to_jev: [...]}`, with `sent_to_jev` holding the literal wire entries (`user: a`) or `[]` when Jev was not consulted (rule hit, or no candidates to decide among). Every existing key keeps its type. |
| B9 | `classify --keywords` is the same code path: the JSON decision object gains the same `keywords` key additively (with `--explain` the `explanation` key stays), the Rich panel gains the `KEYWORDS` section, and `--verbose` prints a `== User keywords ==` block on stderr. |
| B10 | Help stays clean: `organize --help` and `classify --help` document `--keywords` (including "not search terms" and the `--select`/`--to` conflict), are ASCII, and the whole-help forbidden-word scan (`tests/v070_zero_config_p1_help_test.rs`) stays green. |
| B11 | Tests never touch a live Jev or the on-device model: wire assertions run against the std-only mock HTTP server, decision-path assertions use `MockJevIntegration` (plus a recording decorator), Discovery uses an injected search boundary or a fake `mdfind` on `PATH`, and HOME/working directories are isolated. |

## 3. Design decisions

- **D1: keywords go to Jev only, never into Discovery's search terms.** The
request names Jev ("send keywords to Jev as decision material"), and the discovery
query is derived from the normalized context before a `DecisionContext` exists,
so keywords structurally cannot leak into it. Reasons beyond scope: user text
inside an `mdfind` query is a query-injection surface (quotes and operators),
`normalize_terms` silently drops keywords containing filtered substrings
(`screenshot`, `png`, ...), and changing the candidate set would move the
verdicts of runs that only annotated them. Rejected: append keywords to
`NormalizedFileContext.semantic_signals` (Discovery leak + marker filter).
- **D2: wire encoding is `user: <KEY>` entries in the existing
`state.semantic_signals` list.** No new top-level request field, so the
verified OpenAPI contract (`documents/jev-api.md`) keeps accepting our body;
the prefix marks origin (existing signals are `M:`/`K:`/`F:`/`X:`/`FB:` or
bare). Rejected: a `state.keywords` field (unverified; server may reject).
- **D3: the domain carries keywords as a dedicated
`DecisionContext.user_keywords` field; the adapter prefixes them.** Content
signals and user input stay distinguishable for the domain and the verbose
view, and the adapter - the place that already composes the wire list
(availability markers) - owns the encoding. This also keeps keywords immune
to `is_availability_marker`: a keyword like `M:GPS available` must not be
filtered out of the request. Rejected: appending `user:` strings into
`DecisionContext.semantic_signals` (loses origin; collides with the marker
filter).
- **D4: injection point is a `KeywordJevIntegration` decorator at the Jev
boundary (new `src/classifier/keywords.rs`), not a pipeline parameter.** The
trait signature stays untouched, every pipeline/organize signature stays
untouched (no churn across `watch`, `benchmark`, and the existing tests that
call them), and `organize` and `classify` wrap the same way = the required
"one code path". The decorator records whether `decide` was actually called,
which is how the report distinguishes "sent" from "not consulted". Rejected:
threading `keywords: &[String]` through ~12 pipeline functions (signature
churn, every caller and test updated for no behavioral gain).
- **D5: reports carry the data (`OrganizeReport.keywords`,
`ClassifyReport.keywords: KeywordDelivery {provided, sent}`); renderers read
it; the CLI sets it after each run.** No renderer signature changes, JSON
insertion is the established add-only pattern, and the rule-hit path (which
returns before the decorator exists) sets `provided, sent: []` explicitly.
- **D6: priority.** Repository rule > keywords (runtime; ignored but reported
- the user is told, per the request). `--to`/`--select` > keywords (parse-time
conflict; the repo already fails fast on incompatible flag combinations
`--to`/`--select`/`--explain`, and the `--select --format json` document is a
bare array where an unused-keywords note cannot be added). A run with no
candidates consults no Jev either; the report shows `sent_to_jev: []` there
as well (honest "Jev was not consulted").
- **D7: `classify` gets the flag too.** It is the read-only twin through the
same pipeline; previewing how keywords move the verdict (`classify`) before
committing (`organize --execute`) is the natural use, and the marginal cost is
one clap field on the shared decorator path. The user named `organize`; both
commands changed, same path, both tested.
- **D8: value handling.** Values are edge-trimmed and rejected when empty
after trimming; no deduplication (a repeated keyword is sent as given), no
length cap (the server owns payload policy), no normalization (verbatim user
words).
- **D9: report wording is ASCII and generic** - `not used (Jev was not
consulted)` covers both non-consulting paths (rule hit and no candidates);
the surrounding section (Repo policy / no-ranking decision) already explains
which one happened.

## 4. File changes

| # | File | Change |
|---|---|---|
| T1 | `src/classifier/keywords.rs` | New: `USER_KEYWORD_PREFIX`, `KeywordDelivery {provided, sent}` (+ `unused`, `to_json`, `rows`), `KeywordJevIntegration` decorator (records consultation, appends `user_keywords` to a cloned context). |
| T2 | `src/classifier/mod.rs` | Register `pub mod keywords`. |
| T3 | `src/models/decision.rs` | `DecisionContext.user_keywords` (`skip_serializing`), `with_user_keywords` builder, `user_keywords()` accessor. |
| T4 | `src/classifier/jev.rs` | `signals_for_jev` appends `user: <KEY>` entries (between content signals and availability markers; no-op when empty). |
| T5 | `src/operations/organize.rs` | `OrganizeReport.keywords` field (default empty in both constructors). |
| T6 | `src/classifier/pipeline.rs` | `ClassifyReport.keywords` field (default empty). |
| T7 | `src/presentation/renderers/organize.rs` | `KEYWORDS` section (text + panel) iff provided; one additive `keywords` JSON object. |
| T8 | `src/presentation/renderers/classify.rs` | `KEYWORDS` section in the Rich panel iff provided. |
| T9 | `src/main.rs` | `--keywords` on `Organize` (conflicts with `--select`/`--to`) and `Classify`, value parser (trim/non-empty), `OrganizeOptions.keywords`, decorator wiring in `handle_organize`/`handle_classify`, report assignment, additive `keywords` key in classify JSON, `== User keywords ==` in the verbose observer, help text. |
| T10 | `tests/v112_organize_keywords_test.rs` | New: 12 integration tests (`b1`-`b12`) covering B1-B11 (wire mock server, `MockJevIntegration` + recording decorator, injected/fake search, CLI fixtures with isolated HOME). |
| T11 | `tests/organize_output_test.rs`, `tests/v070_zero_config_p1_explain_test.rs`, `tests/v110_stale_index_notice_test.rs`, `tests/v111_repo_policy_test.rs`, `tests/v31_render_test.rs` | The exhaustive report literals gain `keywords: Default::default()` (no expectation changes). |
| T12 | `README.md`, `CHANGELOG.md`, `documents/jev-api.md` | Organize-section note, `[Unreleased] / Added` entry, wire-mapping table row for `user:` keywords. |
| T13 | `documents/plans/dev/issues/done/20261019-done-organize-keywords.md`, `documents/plans/dev/issues/README.md`, `documents/plans/dev/milestones/20261015a-backlog-milestones.md` | Done issue, Done table row (20 -> 21), milestone count + M1 row. |

No new dependency, no config file, no schema change: `Cargo.toml` and the
journal are untouched; `schemas/jev-decision-v1.json` describes the response
(verdict) and is unaffected.

## 5. Verification (2026-10-19, worktree `kaseifu`)

- `cargo test --test v112_organize_keywords_test`: 12 passed, 0 failed.
- `cargo test --all-targets --all-features`: 955 passed / 0 failed (baseline
  937; +18 = 12 integration tests plus 6 module unit tests in
  `src/classifier/keywords.rs`). The known environmental failure of
  `v097_cli_never_waits_for_input_without_a_terminal` (on-device Foundation
  Models returning `GenerationError Code=-1` on this machine) did not occur in
  these runs; the test passed. Nothing in v112 depends on the real model or a
  live Jev: every test uses the loopback mock HTTP server, `MockJevIntegration`,
  an injected search closure or a fake `mdfind` on `PATH`.
- `bash scripts/quality_check.sh`: `OVERALL: PASS` with every row green:
  rustfmt 0 violations, clippy 0 warnings, cargo-test 955 passed / 0 failed,
  complexity analyzer 0.0.25, baseline-gate `no new/worsened CC>=16` (max CC 15,
  CC >= 16 count 0), report self-tests green, coverage 84.67% report-only.
- Manual sandbox runs in `target/v112-manual` (synthetic repository, isolated
  HOME, `KASEIFU_KEY_SOURCE=env`, no API key): rule hit + `--keywords` as a
  Rich panel (`KEYWORDS` section with `Provided` / `Sent to Jev  not used (Jev
  was not consulted)`), the same run with `--format json` (trimmed values,
  `sent_to_jev: []`, every historical key present), `--to` + `--keywords`
  rejected at parse time (exit 2, `cannot be used with`), and `--keywords ""`
  rejected (exit 2, `keyword must not be empty`). The sandbox was removed.
- No commit, push, tag or GitHub write was performed; the result is the
  uncommitted worktree diff. Tests touched no real user repository, Keychain
  entry, journal or home file (isolated HOMEs, fixtures under `target/`,
  loopback server, fake `mdfind`).

## 6. Review notes

Plan Architect (pre-implementation review of this plan against the codebase):
- Confirmed the wire contract claim: `StateDto` is `{target, semantic_signals}`
  and `DecisionContext.semantic_signals` is `skip_serializing` (mapped by the
  adapter only), so `user_keywords` must also be `skip_serializing` and the
  adapter is the single place that composes `state.semantic_signals`
  (`src/classifier/jev.rs::signals_for_jev`). Adding the prefix between
  content signals and availability markers keeps no-flag payloads byte-stable,
  which `tests/v0401_discovery_fix_test.rs` pins.
- Confirmed the Discovery isolation: `extract_terms` runs on
  `NormalizedFileContext` inside `discover_candidates_with_options`, before
  `DecisionContext::from_normalized`; keywords attached afterwards cannot reach
  the mdfind query. B4 tests this at the injected-search boundary rather than
  trusting the argument.
- Confirmed reuse: the decorator mirrors the existing
  `ObservingJevIntegration` pattern (boundary wrapper, trait untouched); the
  CLI fixture pattern (fake `mdfind` on `PATH` + std-only mock HTTP server +
  isolated HOME) already exists in `tests/v070_zero_config_p1_cli_test.rs` and
  is reused; report-literal updates mirror how `rule: None` (v111) and
  `target_indexed` (v110) were added to the same literals.
- Confirmed report-struct field additions are the smaller diff: the 7 exhaustive
  literals across 5 test files take a one-line `keywords: Default::default()`
  versus ~4 new renderer wrapper signatures in the rejected alternative.
- Flagged (folded into the plan): keep `handle_organize`/`handle_classify` at
  or below 7 parameters (clippy's `too_many_arguments` fires at 8+) and keep the
  new section/JSON insertion inside existing `if provided` guards so no-flag
  output is bit-identical; both are reflected in T7-T9 and B3.
- Flagged (folded into the plan): `num_args = 1..` on a flag before a required
  positional swallows later positionals (`--keywords a b file` reads `file` as
  a keyword). The documented form puts `<file>` first; the clap error for a
  missing positional is still explicit. Accepted as clap-standard greedy
  behavior and pinned by B2/B5 tests using the documented order.

Reviewer (after implementation, against the diff and the running binary):
- The injection point held up under review: `KeywordJevIntegration` mirrors
  `ObservingJevIntegration` (boundary wrapper, `JevIntegration` untouched), the
  trait's other implementations (`MockJevIntegration`, `RealJevIntegration`)
  compile unchanged, and `watch`/`benchmark` never see keywords (their reports
  keep the default empty `KeywordDelivery`, so their output is bit-identical).
- Add-only claims were checked key by key: `keywords` appears only when the
  flag was given (CLI test asserts the exact historical key set otherwise), the
  wire body's top-level keys stay `[state, model, questions]`, and
  `classify --explain` still emits `explanation` alongside the new key. Feedback
  raised and fixed during development: the first draft of the CLI conflict test
  passed a destination value to the value-less `--select` (clap reported
  `unexpected argument` instead of the conflict); the test now builds the two
  argument shapes separately, and both assert exit 2 plus `cannot be used with`.
- Safety rules hold: parse-time rejection happens before any file, key or
  journal access; the rule path still resolves no API key; output is ASCII with
  no em dash; no prompt exists anywhere on the flag's paths (non-TTY is the
  normal case in tests); help text carries no forbidden words (the v070
  whole-help scan passes).

QA: `cargo fmt --all -- --check` clean; `cargo clippy --all-targets
--all-features -- -D warnings` clean; `cargo test --all-targets
--all-features` = 955 passed / 0 failed; `bash scripts/quality_check.sh` =
`OVERALL: PASS` (rows above). One quality-gate iteration happened: the first
run reported `rustfmt FAIL` because the test file was written after the
previous format pass; `cargo fmt --all` fixed it and the full gate then passed.
Baseline-gate note: no new or worsened function reached CC 16 (the largest new
functions are the value parser and the decorator at trivial CC).
