# Should a rule match what a file is *about*, not only its path? - waiting for the user

- status: waiting-user
- opened: 2026-10-21 (raised by issue [#13](https://github.com/smapira/kaseifu/issues/13))
- milestone: M5 - user/external blockers (the decision is the user's)
- next owner: user
- blocker: a product decision, not a technical one

## The question

Issue #13 reported that the example rule could not express the real case from
#12: `content/cannabis-rescheduling-hearing-2026/brief.md` belongs to site
`thch-vape`, but the site appears nowhere in the path. Today that case is
expressed by naming the project in the rule
(`match_glob = "content/cannabis-rescheduling-hearing-2026/**"` with
`destination = "sites/thch-vape/content/"`), one rule per project/site pair.

Should a rule also be able to match the file's *content* (so a document that
names `thch-vape` is routed to that site without a per-project rule)?

## What that would cost (assessed, not built)

- `match_contains`/`match_glob` currently see only the relative path: cheap,
  deterministic, readable before a run, and testable without reading a file.
  Body matching would extract text (PDF decode, OCR for images) for every
  `organize` call in a governed repository, so a rule hit would stop being free
  of I/O and of the document pipeline.
- It would also need a decision on what "about" means (a substring? a whole
  word? NFC-folded? which languages?), on caching, and on how the report
  explains a hit that the path alone does not justify.
- Nothing in the current design blocks it: the seam is
  `Rule::matches(&self, relative: &str)`, and a second matcher could take a
  document excerpt without touching the plan layer.

## Recommendation

Keep path-only matching and the per-project rule. It is explicit, reviewable in
the committed file, and needs no extraction; a content matcher can be added
later as an opt-in rule field without breaking any existing policy. Reopen this
record if the answer is yes, and it becomes an implementation issue.
