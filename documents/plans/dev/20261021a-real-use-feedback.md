# Real-use feedback on 0.13.0 (issues #13, #14, #15)

Last updated: 2026-10-21. Status: implemented and verified (see section 5).

## 1. What was reported

The `seo-operations` agent used kaseifu 0.13.0 for a librarian audit (dry runs
only, no `--execute`) and filed three GitHub issues. Every claim was reproduced
here first, in a sandbox under `$HOME` with an isolated HOME, no API key and no
network, before any change was made.

| Issue | Report | Reproduced |
|---|---|---|
| [#13](https://github.com/smapira/kaseifu/issues/13) | The README and module example do not do what their `reason` says, and the real case from #12 cannot match at all. | Yes: with the example as printed, `content/cannabis-hearing/sites/thch-vape/brief.md` plans `sites/cannabis-hearing/content/brief.md` (the *project* capture), and `content/cannabis-rescheduling-hearing-2026/brief.md` matches no rule (no site segment in the path). |
| [#14](https://github.com/smapira/kaseifu/issues/14) | `organize <dir>` runs the file pipeline and is undocumented. | Yes: a folder produced a full rule/decision report with exit 0; `--to --execute` was refused later by the plan layer (`SourceNotAFile`). |
| [#15](https://github.com/smapira/kaseifu/issues/15) 1 | The agent skill does not cover `.kaseifu.toml`, `organize --keywords` or the index disclosure. | Yes (the skill is the contract an agent reads, and it predates all three). |
| [#15](https://github.com/smapira/kaseifu/issues/15) 2 | The README describes three different destination refusals in one sentence. | Yes: `..` and an absolute path are exit 1 errors naming the policy file and the rule; only a symbolic link resolves to REVIEW. |
| [#15](https://github.com/smapira/kaseifu/issues/15) 3 | `skill check` exits 1 both for a stale copy and for a missing file. | Yes. |

One further trap was found while reproducing: a `{site}` written in a
`destination` (the pattern spelling instead of the `<site>` template spelling)
became a literal directory name - the documented example written for this round
fell into it and the new test caught it.

## 2. Behavior units (as implemented and verified)

| ID | Behavior |
|---|---|
| B1 | `organize` refuses a folder in every shape (`plain`, `--execute`, `--to`, `--select`, `--select --format json`, `--keywords`) with exit 1, before the repository policy is read, before the key is resolved, before the journal is opened and before any network call; the message names the folder and `kaseifu label <folder>`. |
| B2 | `classify` refuses a folder the same way (`classify works on one file`), while `inspect` still reports one on purpose (directory entries, metadata) and `watch --path <dir>` still takes a directory tree. |
| B3 | A file is untouched by the new check: a rule hit still decides it, the dry run still reports `would_move`, and the long help of both commands states that the target is one regular file. |
| B4 | The documented example captures the site from the path (`content/{project}/sites/{site}/**` into `sites/<site>/content/`) and plans `sites/thch-vape/content/brief.md`; a project path without a site segment matches no rule (that project pins its site in a rule of its own). |
| B5 | A `{name}` in a `destination` is a load error that names the policy file and the rule, instead of quietly becoming a directory named `{name}`. |
| B6 | `skill check` exits 0 for a matching copy, 1 for a stale one and 3 when there is no file at the path; the message and the JSON report for the first two are unchanged. |
| B7 | The embedded skill covers the repository policy, `--keywords`, the index disclosure, the one-file rule and exit code 3; the repository's own copy stays byte-identical to it (refreshed with the binary, not by hand). |
| B8 | The README states the three destination refusals separately (read-time error for absolute / `.` / `..` / `{name}`, REVIEW for a resolved symlink escape, and the existing checks for a missing directory, an excluded folder and a collision). |

## 3. Design decisions

- **D1: a folder is refused, not supported.** Issue #14 offered both. The plan
  layer already refuses to move a folder (`PlanError::SourceNotAFile`), and the
  journal, `undo`, `--select` and `--to` are all file-level; "support folders"
  would mean defining what a folder move means for each of them. Refusing at
  the boundary removes no working capability and answers the reported risk (a
  plausible verdict, exit 0, about a folder).
- **D2: the example was wrong, not the matcher.** `match_glob` and
  `match_contains` see the path relative to the policy root and never the file
  body (the #12 design: rules must be cheap, deterministic and readable before
  a run). The example's `reason` claimed site ownership while its destination
  read the *project* capture, and the reported real case has no site segment to
  match - so the docs now show a truthful example plus the pinned-site rule that
  expresses "this project belongs to that site". Inferring a site the path does
  not name would mean reading the file body in rule matching: a new capability,
  recorded as a user decision
  (`issues/waiting-user/20261021-waiting-rule-content-matching.md`).
- **D3: exit code 3 for a missing copy.** `skill check` is the one command a CI
  step reads a code from, and "refresh this copy" is a different repair from
  "this repository never had one". 3 keeps the existing 0/1/2 meanings intact
  (1 stays "failed", 2 stays clap usage) and matches the reference
  implementation's third code.
- **D4: `{name}` in a destination is refused.** The module already follows "a
  typo fails loudly instead of silently changing where files go" (unknown keys,
  empty entries, absolute destinations). A literal `{site}` directory is exactly
  that class of mistake, and the docs cannot catch it - the parser can.
- **D5: the tests keep the mechanism fixture.** `tests/v111` keeps `SITE_RULE`
  (named capture + `match_contains` AND-filter) with an honest reason and gains
  `b17` for the documented example, instead of rewriting the expected
  destination of eight existing tests: both shapes are now pinned, and the
  reporter's observation (b4/b5 fix the project-capture destination) stays true
  and explained.

## 4. Files

- `src/main.rs`: `require_regular_file` and its two call sites
  (`dispatch_organize`, `handle_classify`), the help paragraphs, `CheckOutcome`
  handling in `handle_skill_check`, `exit_with_code`.
- `src/skill/mod.rs`: `CheckOutcome::{Compared, Missing}`, `check` returns it,
  one unit test.
- `src/repo_policy/mod.rs`: the example and the matching/workspace paragraphs,
  the `{name}`-in-destination refusal, one unit test case.
- `src/repo_policy/toml.rs`: the schema example (all four keys, pinned site).
- `src/skill/kaseifu/SKILL.md` plus the distributed
  `.github/skills/kaseifu/SKILL.md` (byte-identical, refreshed with the binary).
- `README.md`: the one-file rule for `organize`/`classify`, the corrected
  policy example and prose, the separated destination refusals, the
  `skill check` exit codes.
- `tests/v113_folder_target_test.rs` (new, 5 tests), `tests/v111` (`b17`,
  honest fixture reason), `tests/v109` (missing copy = exit 3),
  `tests/user-test-plan.csv` (T42 expectation).
- `CHANGELOG.md` (Unreleased), this brief, the three issue records and the
  waiting-user record.

## 5. Verification (2026-10-21, worktree `kaseifu`)

- `cargo test --test v113_folder_target_test`: 5 passed, 0 failed.
- `cargo test --test v111_repo_policy_test`: 17 passed, 0 failed.
- `cargo test --lib`: 254 passed, 0 failed.
- `cargo test --all-targets --all-features` and `bash scripts/quality_check.sh`:
  recorded in the issue records.
- Manual sandbox runs (isolated HOME, no key, stdin closed): the folder refusal
  in all six shapes, `classify` refusing a folder, `inspect` reading one, the
  policy examples against both path shapes, the three destination refusals
  (`..`, absolute, symlink), and `skill check` at 0 / 1 / 3.
- No commit, push, tag or GitHub write was performed; the result is the
  uncommitted worktree diff.
