# Repository policy file (`.kaseifu.toml`): pattern -> destination rules

Last updated: 2026-10-19. Status: implemented and verified.

## 1. Objective

Issue [#12](https://github.com/smapira/kaseifu/issues/12) measured a gap in the
librarian audit of `seo-operations`: kaseifu answers file-level placement
(`content/` is the best home, confidence 0.96) but cannot answer repo-level
governance ("this project belongs to `sites/thch-vape/`, or it belongs in
`archive/`"), because the file-level input carries no repository policy. The
user configuration (`~/Library/Application Support/kaseifu/config.toml`) tunes
thresholds, metadata and language; a pattern -> destination rule has no place
there.

kaseifu now reads a repository policy file, committed next to the code it
governs:

```toml
[[rule]]
name = "site-owned-content"
match_glob = "content/{project}/**"
match_contains = "sites/thch-vape"
destination = "sites/<project>/content/"
reason = "a content project that names a site belongs to that site"
```

> Corrected 2026-10-21 (issue
> [#13](https://github.com/smapira/kaseifu/issues/13)): this block stays as the
> record of what was built and verified (B4 pins its destination), but the
> destination follows the *project* capture, not the site, and the `reason`
> therefore over-promised. The example the docs print now captures the site
> itself (`match_glob = "content/{project}/sites/{site}/**"` with
> `destination = "sites/<site>/content/"`); see
> `20261021a-real-use-feedback.md`.

- A rule hit is stronger evidence than the classifier: it decides the file
  without Discovery and without a model call, and the proposal states the rule
  name and the author's reason.
- A repository without the file behaves exactly as before: the lookup finds
  nothing, no policy is read, nothing is written, and the verdict is unchanged.
- A rule never widens what kaseifu may do: dry run by default, `--execute`
  only, journal plus `undo`, never-a-destination rules, collision refusal, and
  a destination confined to the workspace that holds the policy file.
- `.kaseifu.toml` is read-only input. kaseifu never creates or rewrites it.

## 2. Behavior units (as implemented and verified)

The vocabulary is the issue's: a rule states a pattern (`match_glob`), optional
path substrings (`match_contains`), a destination (`destination`, with `<name>`
placeholders fed by `{name}` captures) and a `reason`.

| ID | Behavior |
|---|---|
| B1 | A repository without `.kaseifu.toml` has no rule: `repo_policy::evaluate` returns `None` and `organize` runs the classifier path unchanged (verified in-process and by the unchanged CLI key guidance). |
| B2 | The nearest `.kaseifu.toml` at or above the file's directory governs it: `repo/sub/.kaseifu.toml` wins over `repo/.kaseifu.toml` for files under `repo/sub`, and `repo/.kaseifu.toml` still governs the rest of `repo`. |
| B3 | The lookup never ascends above the home directory: a policy file at the sandbox root does not govern a file under `home/`, while the same file is governed when no bound is passed. |
| B4 | `match_glob` plus `match_contains` decide the hit: `content/{project}/**` with `match_contains = "sites/thch-vape"` hits `content/cannabis-hearing/sites/thch-vape/brief.md` (destination `sites/cannabis-hearing/content`), and misses both `content/cannabis-hearing/brief.md` (no `sites/thch-vape`) and `docs/.../sites/thch-vape/brief.md` (outside the glob). Every `match_contains` string must appear. |
| B5 | A rule hit skips Discovery and the classifier: `organize_file_with_full` with a Jev mock that fails when called reports `WouldMove` to the rule destination, `rule` set, `decision.destination_id = None`, `candidate_path = None`, and the file untouched. |
| B6 | A file outside every rule keeps the classifier path: `rule` is `None`, the decision is the mock's `NONE`, and no rule data reaches the report. |
| B7 | A dry run is still the default for rule hits: `WouldMove` and zero journal rows. |
| B8 | `execute` moves the file, journals one row with `candidate_id = "rule:site-owned-content"` and NULL confidence/margin, and `undo::undo_last_move` restores the original path. |
| B9 | A destination that resolves outside its workspace (here through a symbolic link) is refused with `DestinationOutsideWorkspace`, rendered as REVIEW, and nothing moves. |
| B10 | An authoring mistake (`destination = "../elsewhere"`) is an error that names the policy file and the rule, and `organize` fails closed with no journal row. |
| B11 | A never-a-destination folder (`.cache`) is refused: `DestinationExcluded`, "destination is not a usable folder". |
| B12 | A collision is refused and rendered as REVIEW; the existing file keeps its bytes. |
| B13 | A destination directory that does not exist is refused (`DestinationNotADirectory`); kaseifu creates no directory. |
| B14 | `.kaseifu.toml` is read-only input: the bytes are identical after a dry run and after an executed move, and the sandbox still holds exactly one policy file. |
| B15 | The report names the rule: text shows a `Repo policy` section (rule, pattern, destination, reason) in place of the classifier rows, and `--format json` adds one `rule` object (name, match_glob, destination, reason) while `source`, `mode`, `decision`, `policy`, `candidate_path` and `outcome` stay present. |
| B16 | The CLI works without an API key on a rule hit: `organize <file>` and `organize <file> --format json` both exit 0 with a rule report, and the file stays where it is (dry run). |

Module-level unit tests cover the matcher and the policy compiler: `?`/`*`/`**`/`**/`/`{name}`, whole-path anchoring, the first-matching-rule rule, positional labels, empty and malformed patterns, empty `match_contains` entries, unknown keys, absolute and escaping destinations, and a placeholder without a capture.

## 3. Design decisions

- **Workspace = nearest `.kaseifu.toml`, bounded by `$HOME`.** A file is
governed by its own repository, not by the current working directory, so
`organize ~/projects/x/notes.md` works from anywhere. The walk stops after
checking `$HOME`, so a policy file at a volume root cannot govern a person's
files. `home` is a parameter (`None` in tests that want the unbounded walk),
which is what makes the rule testable without touching a real home.
- **A rule hit bypasses Discovery and the classifier (no model call).** An
explicit rule is the strongest available evidence, and skipping the model makes
rule-governed repositories deterministic, offline and free. Rejected: run the
classifier and let the rule override its verdict (same outcome, one extra model
call and one more failure mode), and run the classifier only when the rule is
"ambiguous" (a rule either matches or it does not).
- **A refusal is final; there is no fallback.** If a matched rule's destination
cannot be used, the file stays and the report says why. Falling back to the
classifier would move a file somewhere the rule author did not name, which is
worse than doing nothing. It also keeps the refusal path free of network calls.
- **The match haystack is the relative path, not the file body.**
`match_contains` sees every path segment (directories plus the file name).
Rejected: read file contents (encoding, binary formats, document size and a
privacy question for no rule in the reviewed policies). Recorded in the module
docs so the limit is deliberate and visible.
- **Captures are named in the glob (`{name}`), not positional.** The issue's
sketch pairs `match_glob = "content/*"` with
`destination = "sites/<site-id>/content/"`; a positional star has no name to
address, so the site id could not come from it. One extra token
(`content/{project}/**`) makes the capture explicit, and a placeholder that no
capture provides is a load-time error, so the sketch's shape fails loudly
instead of expanding to an empty path segment.

- **All `match_contains` strings must appear (AND).** A rule hit is a strong
claim, so the stricter reading is the safer one; "any of" can be written as
separate rules.
- **`*` never crosses `/`; `**` does, and `**/` may also match nothing.** This
is the shape users expect from `.gitignore`; the alternative (a `*` that
crosses separators) makes `content/*` swallow the whole repository.
- **Authoring mistakes fail at load; environment refusals fail at plan time.**
A malformed file, an unknown key, an absolute or `..` destination, an empty
`match_contains` entry or a placeholder without a capture is an error that
names the file and the rule: a committed policy is code-like input, and
silently ignoring it would hide a broken governance decision. A destination
that resolves outside the workspace, an excluded shape, a missing directory or
a collision is a per-file refusal (REVIEW or NO MOVE), because those depend on
the filesystem, not on the author's intent.
- **The destination must already exist as a directory.** kaseifu never creates
destination directories (unchanged); a rule naming a directory that does not
exist yet is reported, not created. Rejected: create the directory chain with
the move (a new mutation the issue did not ask for).
- **The journal records the rule, without a schema change.** A rule move writes
`rule:<label>` into the existing `candidate_id` column and leaves confidence and
margin NULL, exactly the way `rename` records its naming rule. Rejected: add a
`rule_name` column (a migration outside this issue) and NULL everything
(`--to`-style, which loses the audit trail this feature exists for). `undo` and
`doctor` need no change.
- **The report replaces the classifier sections.** A rule hit has no selection,
confidence or margin, so the report shows one `Repo policy` section instead of
`DECISION`/`POLICY` rows of zeros, and `--explain` (a threshold view) is
skipped. `--format json` is add-only: one `rule` object appears and every
existing key stays, with `policy: "none"` and `decision.action: "none"`
saying honestly that no classifier verdict exists. `outcome` remains the
authority on what happens to the file.
- **Scope: `organize` and `watch`; explicit human destinations win.** Both call
the shared entry point, so both honor rules. `--to <dir>` and `--select` name a
destination themselves and never consult the policy. `classify`/`inspect` stay
classifier views (rules are execution policy, not classification).
`watch` still resolves the API key before it starts watching (existing
behavior): a rule-only watch workflow with no key is not supported by this
change and is recorded as a limitation.
- **Reuse instead of new code.** No new dependency: the matcher and the
template are ~150 lines with unit tests, TOML rides the existing `toml` crate,
`serde` rides `serde`, containment reuses `benchmark::is_inside` (normalized
paths, already used by the benchmark scatter), never-a-destination reuses
`discovery::is_excluded_destination`, and the source/directory/collision checks
reuse `plan::checked_destination_path`. Rejected: a `glob`/`regex` crate (a new
dependency for four tokens) and hand-copying the exclusion or containment
rules (two implementations of one safety rule is how rules drift apart).
- **`organize` opens the journal before it resolves the API key.** The rule
check needs the journal for the execute path, and a rule hit must not depend on
credentials, so the order is policy, key, classifier. The only visible
difference is on a failing run in an ungoverned repository: an `organize` that
stops at "no API key is configured" now leaves the (empty) journal file that
every successful `organize` already creates. No row is written, and
`--format json` still owns the whole document.


## 4. File changes

| # | File | Change |
|---|---|---|
| T1 | `src/repo_policy/mod.rs` | New module: rule and policy types, compiler (globs, templates, static destination checks), workspace lookup, `evaluate`, module unit tests. |
| T2 | `src/repo_policy/glob.rs` | New: the four-token matcher with named captures and a step budget, the `<name>` destination template, unit tests. |
| T3 | `src/repo_policy/toml.rs` | New: `.kaseifu.toml` schema (`[[rule]]`, `deny_unknown_fields`, `match_contains` as one string or a list) and parsing. |
| T4 | `src/lib.rs` | Register `pub mod repo_policy`. |
| T5 | `src/operations/plan.rs` | Add `PlanError::DestinationOutsideWorkspace` and `build_rule_move_plan` (workspace boundary plus the manual-move checks); doc tweak on the shared helper. |
| T6 | `src/operations/organize.rs` | `OrganizeReport.rule`, the early `rule_decision` branch in `organize_file_with_full`, `rule_outcome`, `home_directory`. |
| T7 | `src/operations/mover.rs` | `MoveJournal::Rule` plus `execute_rule_move` (selection `rule:<label>`, NULL classifier numbers). |
| T8 | `src/presentation/renderers/organize.rs` | `Repo policy` section and rule rows, REVIEW for an outside-workspace destination, `destination_outside_workspace` JSON kind, one additive `rule` JSON object. |
| T9 | `src/main.rs` | `handle_organize` consults the policy before resolving an API key; extracted `print_organize_report`; one sentence about `.kaseifu.toml` in the `organize` long help. |
| T10 | `tests/v111_repo_policy_test.rs` | New: 16 integration tests (B1-B16) in synthetic repositories. |
| T11 | `tests/organize_output_test.rs`, `tests/v070_zero_config_p1_explain_test.rs` | The two report literals gained `rule: None` (no expectation changed). |

No dependency, schema or help-scan change: `Cargo.toml` is untouched, the
journal schema is untouched, and the new help sentence is ASCII and free of the
words the whole-help scan forbids (`tests/v070_zero_config_p1_help_test.rs`
still passes).

## 5. Verification (2026-10-19, worktree `kaseifu`)

- `cargo test --test v111_repo_policy_test`: 16 passed, 0 failed.
- `cargo test --lib repo_policy`: 14 passed, 0 failed.
- `cargo test --all-targets --all-features`: 930 passed / 0 failed (baseline 900;
  +30 = 14 module unit tests plus 16 integration tests).
- `bash scripts/quality_check.sh`: `OVERALL: PASS` (rustfmt 0 violations, clippy 0
  warnings, cargo-test 930 passed, CC >= 16 = 0, max CC 15, coverage 84.33%
  report-only).
- Manual sandbox runs in `target/v111-manual` (a synthetic repository, isolated
  HOME, `KASEIFU_KEY_SOURCE=env`, no API key, stdin closed except for the one
  deliberate rich-TTY panel): rule hit text panel, rule hit plain text, rule hit
  `--format json`, collision (REVIEW), broken policy (exit 1 with the file and
  rule named), `--explain` on a rule hit (no threshold section), and `--to`
  winning over the rule. The sandbox was removed afterwards.
- The CLI `--execute` flag was deliberately not run (the request asks for dry
  runs only). The execute path (move, journal row, undo) is verified through the
  library in B8 with an in-memory journal and a sandbox destination.
- No real user repository, Keychain entry, journal or home file was read or
  written: every test and manual run used an isolated HOME and a synthetic
  repository, and the tests that need a home bound inject it.

## 6. Review notes

Plan architect (before implementation, against the codebase): checked that
`discovery::is_excluded_destination` is the single destination authority, that
`plan::build_manual_move_plan` is the existing precedent for a destination that
is not in the discovery candidate list, that `benchmark::is_inside` already
implements normalized containment, and that `marker`-style parsing conventions
(unknown keys rejected, values validated, one clear message per mistake) come
from `config::loader`. Two consequences were folded into the plan: do not invent
a candidate id for a rule destination (use the plan path that already exists),
and keep the change to `src/operations/plan.rs` additive (one enum variant plus
one new builder) so the in-flight issue #11 work on that file merges cleanly.

Reviewer (after implementation): every new behavior has one owner
(`repo_policy` parses/matches/resolves, `plan` decides whether a destination may
be used, `organize` orchestrates, the renderer prints), the report change is
additive, the diff has no unrelated edit, and the ASCII/no-color/no-prompt rules
hold. Feedback raised and fixed: `rule_decision` documented a vague reference
("the two cases the design doc names") instead of naming collision and
outside-workspace, and the shared helper comment said "both plan builders" after
a third one was added.

QA: `cargo test --all-targets --all-features` (930 passed / 0 failed),
`cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, and `bash scripts/quality_check.sh` (`OVERALL: PASS`). Feedback raised
and fixed: the first version of the recursive matcher measured CC 17, which the
baseline gate correctly rejected as a new function at or above 16; the matcher is
now a small state struct whose per-token functions each stay well below it (max
CC in `src/` is 15, and no function is at or above 16). A second QA pass
confirmed the same three commands plus the manual sandbox runs listed above.

Integration (2026-10-19, after the issue #11 fix): the three-way merge produced
two conflicts (the `theme` / `RuleMatch` imports, and the new
`organize_file_with_search` seam vs. the rule early-return). Both sides were kept:
the rule check runs at the head of `organize_file_with_search`, and the report
carries `target_indexed` and `rule` together. Review also caught and fixed one
regression: the journal was opened before the rule check, so an ungoverned
`organize` without an API key created an empty `kaseifu.db` before failing; the
rule is now asked for first (`organize::rule_hit`) and the journal is opened only
when a rule applies or the classifier path reaches it. Post-merge:
`cargo test --all-targets --all-features` = 937 passed / 0 failed and
`bash scripts/quality_check.sh` = `OVERALL: PASS`. Independent sandbox runs also
confirmed `--to` winning over a rule, a symlink destination escaping the
workspace (REVIEW), a never-a-destination rule destination (refused), the
journal row `rule:<name>`, and `undo` restoring a rule move.
