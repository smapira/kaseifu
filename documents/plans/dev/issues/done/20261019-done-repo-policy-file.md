# Repository policy file (`.kaseifu.toml`) - done

- status: done
- completed: 2026-10-19 (GitHub issue [#12](https://github.com/smapira/kaseifu/issues/12), user request)
- milestone: n/a - user-requested feature (reference: the librarian audit of `seo-operations`)
- next owner: none (verify at merge time)
- blocker: none

## Objective (as requested)

A repository can define `pattern -> destination` placement rules in a committed
`.kaseifu.toml`, so kaseifu can answer the governance step the librarian audit
could not: "a content project that names a site belongs to that site", not only
"`content/` is the closest folder".

```toml
[[rule]]
name = "site-owned-content"
match_glob = "content/{project}/**"
match_contains = "sites/thch-vape"
destination = "sites/<project>/content/"
reason = "a content project that names a site belongs to that site"
```

## What changed

- New module `src/repo_policy/` (`mod.rs`, `glob.rs`, `toml.rs`): the policy
  schema, a four-token glob matcher (`?`, `*`, `**`, `{name}`) with named
  captures and a step budget, a `<name>` destination template, load-time
  validation, the bounded workspace lookup, and `evaluate()`.
- `src/operations/plan.rs`: `build_rule_move_plan` (workspace boundary plus the
  existing excluded/directory/collision checks) and the new
  `PlanError::DestinationOutsideWorkspace`.
- `src/operations/organize.rs`: `OrganizeReport.rule`, the early `rule_decision`
  branch, and `rule_outcome`. A rule hit skips Discovery and Jev.
- `src/operations/mover.rs`: `execute_rule_move`, journaling
  `candidate_id = "rule:<label>"` with NULL confidence/margin (the `rename`
  precedent); no schema change, `undo` unchanged.
- `src/presentation/renderers/organize.rs`: one `Repo policy` section in place
  of the classifier rows, REVIEW for an outside-workspace destination, and one
  additive `rule` object in `--format json`.
- `src/main.rs`: `organize` consults the policy before resolving an API key, so
  a governed repository works without credentials; one sentence about
  `.kaseifu.toml` in the long help.
- `tests/v111_repo_policy_test.rs` (new, 16 tests) plus 14 module unit tests and
  `rule: None` in the two existing report literals.
- Dev brief `documents/plans/dev/20261019c-repo-policy-file.md`.

## Verification (2026-10-19, worktree `kaseifu`)

- `cargo test --test v111_repo_policy_test`: 16 passed, 0 failed.
- `cargo test --lib repo_policy`: 14 passed, 0 failed.
- `cargo test --all-targets --all-features`: 930 passed / 0 failed (baseline
  900; +30).
- `bash scripts/quality_check.sh`: `OVERALL: PASS` (rustfmt 0, clippy 0,
  cargo-test 930 passed, CC >= 16 = 0, max CC 15, coverage 84.33% report-only).
- Manual sandbox runs in a synthetic repository with an isolated HOME and no API
  key: rule hit text/Rich/JSON, collision REVIEW, broken policy exit 1,
  `--explain` on a rule hit, and `--to` winning over the rule.
- A repository without `.kaseifu.toml` is unchanged: the same run still reports
  the existing "no API key is configured" guidance.
- No commit, push, tag or GitHub write was performed; the result is the
  uncommitted worktree diff.

## Integration (2026-10-19, after the issue #11 fix landed)

Both changes touched `src/operations/plan.rs`, `src/operations/organize.rs` and
`src/presentation/renderers/organize.rs`, and the three-way merge produced two
conflicts there (the `theme` / `RuleMatch` imports and the new
`organize_file_with_search` seam vs. the rule early-return). Resolved by keeping
both: the rule check now sits at the head of `organize_file_with_search`, so
every entry point (CLI, `watch`, tests with an injected search) sees rules, and
`OrganizeReport` carries both `target_indexed` and `rule` (`rule_decision` sets
`target_indexed: None`, since a rule hit consults no index).

One regression was found in review and fixed: the journal was opened before the
rule check, so an ungoverned `organize` with no API key created an empty
`kaseifu.db` before failing (the same class as
`open/20261016-open-label-guidance-touches-state.md`). The rule is now asked for
first (`organize::rule_hit`), the journal is opened only once a rule applies (or
after the key is resolved on the classifier path), and `organize::rule_report`
keeps the journal injectable for tests. Verified: that run now exits 1 with no
`kaseifu` directory in the isolated HOME.

Post-merge verification: `cargo test --all-targets --all-features` = 937
passed / 0 failed; `bash scripts/quality_check.sh` = `OVERALL: PASS` (rustfmt 0,
clippy 0, CC >= 16 = 0, coverage 84.38%). Independent sandbox runs: rule hit
without a key (dry run and `--execute`), `undo` restoring the rule move, the
journal row `rule:<name>` with NULL confidence/margin, `--to` winning over the
rule, a symlink destination escaping the workspace (REVIEW, names the resolved
path), a never-a-destination rule destination (refused), and a broken policy
(exit 1, file and rule named).
