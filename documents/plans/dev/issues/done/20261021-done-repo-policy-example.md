# Repository policy example that does what it says - done

- status: done
- completed: 2026-10-21 (GitHub issue [#13](https://github.com/smapira/kaseifu/issues/13), real-use feedback from `seo-operations`)
- milestone: M3 - workflow semantics
- next owner: none (verify at merge time)
- blocker: none

## Objective (as reported)

The README and the `src/repo_policy/mod.rs` example were read as "this content
belongs to that site", but the destination followed the *project* capture
(`sites/<project>/content/`), and the real case from issue #12 - a project whose
path never names its site - could not match the rule at all. The report asked
for the example to work as written, for the two matching bases to be stated, and
for a judgement on whether #12's intent was only partly implemented.

## What changed

- The documented example captures the site from the path:
  `match_glob = "content/{project}/sites/{site}/**"` with
  `destination = "sites/<site>/content/"` and a reason that matches that effect
  (README, module documentation, embedded skill).
- The prose states what each matcher sees: `match_glob` matches the whole path
  relative to the policy root, `match_contains` is a substring test on the same
  relative path, and neither reads the file body. The README adds the
  pinned-site snippet for a project whose path does not name its site (the shape
  that answers the reported case), and `src/repo_policy/toml.rs` shows all four
  schema keys in its example.
- `tests/v111_repo_policy_test.rs`: `b17` pins the documented example end to end
  (rule evaluation plus planned destination) and the "no site segment" case; the
  mechanism fixture keeps its coverage with an honest reason.
- `documents/plans/dev/20261019c-repo-policy-file.md` carries a dated note that
  its example block over-promised (the record itself stays).

## Judgement asked for in the issue

The matcher is not partly implemented: matching the file body was excluded on
purpose (rules must be cheap, deterministic and readable before a run). The
intent behind #12 is expressible today by naming the project in the pattern and
pinning the destination; inferring a site the path does not name is a new
capability and is recorded as a user decision in
`../waiting-user/20261021-waiting-rule-content-matching.md`.

## Verification

- `cargo test --test v111_repo_policy_test`: 17 passed / 0 failed (was 16).
- `cargo test --lib repo_policy`: the compile-time cases include the new
  `{name}`-in-destination refusal.
- Manual sandbox runs: the example plans
  `sites/thch-vape/content/brief.md` for
  `content/cannabis-hearing/sites/thch-vape/brief.md`; the #12 path matches no
  rule; the pinned-site snippet matches it.
- Full suite and `scripts/quality_check.sh`: see
  `20261021-done-organize-folder-target.md` (same round, one worktree).
