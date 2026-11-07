# A held `label` proposal names its real cause - done

- status: done
- completed: 2026-11-08 (GitHub issue [#21](https://github.com/smapira/kaseifu/issues/21); commit `20d5094`)
- milestone: M1 - Reliability and CLI clarity (a held rename is day-to-day
  output, and the message pointed at the wrong cause)
- next owner: none (verify at merge time)
- blocker: none

## The finding

`kaseifu label ~/Desktop/cbd-common-scale-5y.png --format json` answered
`"proposal": null`, `"perception": null`, `"date_source": ""` and the reason
`name must not contain a path separator`. Nothing in that answer was about the
file: the separator came from the model's *title*, and the validator was the
first component to complain about the assembled proposal, so its complaint was
reported as the cause. A reader could only conclude that the target's own name
was bad.

## Decision

- The model's title is checked **before** the template fills it in
  (`title_hold_reason`), and only when the template actually uses `{title}`: a
  stored template built from `{date}` and `{category}` is not held for a title
  it never needed.
- A hold carries the facts it already had (`HoldFacts`): the perception it ran
  and the date source it resolved. `None` means "not reached yet", never
  "unavailable"; `date_source` stays `""` only for a hold that happened before
  the date was resolved, so the empty value is never invented.
- "no title" and "perception unavailable" are different reasons, and an image
  hold names the perception failure (`failed: image-undecodable: ...`) instead
  of collapsing both into an empty proposal.

## What changed

- `src/rename/service.rs`: title check in `finish_plan` / `finish_directory_plan`,
  `HoldFacts` (perception + date source) on `PlanOutcome::Held`, per-path
  reasons (`title_hold_reason`, `no_title_reason`, `proposal_problem_reason`,
  `empty_stem_reason`).
- `src/rename/validate.rs`: `has_separator` is public so the title check uses the
  same character set as the validator (`/`, `\\`, `\0`, `:`).
- `src/main.rs`: a held report reads `perception` and `date_source` from the
  hold facts, so the JSON carries the perception it obtained and the date
  source it resolved.
- `tests/v116_label_proposal_reason_test.rs` (new, 10 tests).

## Verification (2026-11-08)

- `tests/v116_label_proposal_reason_test.rs`: 10 passed (title separator, empty
  title, image perception unavailable, normal text/image/folder proposals
  unchanged).
- `bash scripts/quality_check.sh` at `96b946b`: OVERALL PASS (1044 tests,
  rustfmt 0, clippy 0, CC >= 16 = 0, coverage 84.81%).
- Isolated HOME run of the reported command shape: `date_source` reads
  `modified`, `perception` reads `{"status":"unavailable","reason":"failed:
  image-undecodable: ..."}` instead of `null`.
- `--format json` keeps every key; only the null/empty values become real.
