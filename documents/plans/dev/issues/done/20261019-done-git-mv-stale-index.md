# `organize` after `git mv`: index lag vs. a real collision - done

- status: done
- completed: 2026-10-19 (GitHub issue #11, user report)
- milestone: M1 - reliability and CLI clarity
- next owner: none (verify at merge time)
- blocker: none

## What the issue asked

After `git mv ai-generated-text.md scripts/ai-tools/ai-generated-text.md`,
`kaseifu organize scripts/ai-tools/ai-generated-text.md` reported
`REVIEW destination already exists: <root>/ai-generated-text.md` for a path the
reporter's `ls` said was gone, and the classifier proposed the pre-move
directory with confidence 0.950. Acceptance: (1) existence must be `stat`, not
the index; (2) when the classifier's input may be stale, say so or fall back to
a filesystem walk.

## What was found

- Acceptance 1 was **already true**. `destination already exists` is
  `PlanError::Collision`, built only from `Path::exists()` in
  `operations/plan.rs` (repeated by `mover::verify_preconditions`). Discovery
  also drops every hit whose path is not on disk, so an index entry can never
  become a phantom destination.
- The collision was **true**. On the issue's own path, discovery returned
  `D1 = <seo-operations>` score=1.00, reason `exact normalized filename`, and
  `<root>/ai-generated-text.md` is a real tracked file there (683 bytes,
  `mtime == ctime 2026-10-06`; `git status` clean; `scripts/ai-tools/` holds
  only `ai_tool_check.py`).
- Both of the reporter's observations were true of **different checkouts**: the
  verdict named the primary checkout's root, the `ls` checked the root of the
  worktree the file had been moved in. Four of the five Delta worktrees of
  `seo-operations` hold the file at `scripts/ai-tools/` (mtime 2026-10-19); the
  primary and one older worktree hold it at the root, and a read-only
  `kaseifu discover` from a moved worktree selects the primary (`D1`).
- The index lag is real but in the other direction: `mdfind` returned **no**
  path for the just-moved file (0s) and only the new path at 5s; the old path
  never came back in 30s. So the new location cannot compete, and the pre-move
  directory (real files with the same words) wins legitimately.
- The worktree's new location is not a candidate by design: a file's own
  directory is never a destination, so the reporter's expectation ("propose
  `scripts/ai-tools/`") cannot be met by a ranking change.
- The verdict gave a reader no way to know the index had no entry for the file.
  That is what was fixed.

## What changed

- Discovery records whether the target itself appeared among the hits
  (`SimilarFiles::target_indexed`, set at the canonical comparison it already
  performs) and hands it out as `DiscoveryOutcome`.
- `OrganizeReport::target_indexed` carries it; `organize` renders one row under
  DECISION - `Index  not indexed yet; candidates may be incomplete` - in plain
text and in the Rich panel, and `--format json` gains the add-only key
  `target_indexed` (`true` / `false` / `null`). Nothing else changes: policy,
  plan gate, `--execute`, journal and undo are untouched.
- `organize_file_with_search` is the new injectable-search seam, so the whole
  verdict (discovery -> Jev -> policy -> plan -> report) is testable without the
  machine's index state.
- `tests/v110_stale_index_notice_test.rs` (7 tests) covers: a stale hit for a
  missing path never becomes a candidate; the index state is reported and the
  target stays out of its own evidence; no terms claim nothing; a plan collides
  only with a real file; the reported scenario renders the collision with the
  notice; the same run without the blocking file plans the move; and no notice
  appears for `Some(true)` / `None`.
- `README.md` (Spotlight section) and `CHANGELOG.md` (`[Unreleased]`) record the
  user-visible change. Dev brief:
  `../20261019b-git-mv-stale-index.md`.

## Rejected

Querying Spotlight for the destination (would create the bug), an unbounded
filesystem-walk fallback (discovery is machine-wide: 744 hits, 459 directories
measured), suppressing the pre-move directory (silently changes decisions, and
`git mv` leaves no trace kaseifu can read), refusing auto-move when unindexed
(a new file legitimately has no entry), and a `ctime` freshness window (fires on
every fresh download and every fixture).

## Verification (2026-10-19, worktree `kaseifu`)

- `cargo test --all-targets --all-features`: 907 passed / 0 failed / 5 ignored
  (baseline 900 passed; +7).
- `bash scripts/quality_check.sh`: `OVERALL: PASS` (rustfmt 0, clippy 0,
  cargo-test 907/0, CC >= 16 = 0, max CC 15, coverage 83.96% report-only).
- Timing measurements ran in a throwaway `git init` repository under `$HOME`
  and it was deleted; the `seo-operations` checkout was only read
  (`stat`, `git ls-files`, `git status`). No `--execute` run, no write to a user
  file, journal, Keychain entry or completion directory.

## Explained (2026-10-19, after the first report)

The reporter's `ls` and the `git mv` state both hold: the verdict's destination
was another checkout of the same repository (the primary), whose root really
contains `ai-generated-text.md`. `kaseifu discover` from a moved worktree
selects that primary checkout (score 1.00, reason `exact normalized filename`).
Nothing in the collision path or the ranking was wrong; the output was easy to
misread because both paths are "the repo root" of some checkout.

Follow-up (needs a product decision, not a bug fix): destinations are
machine-wide, so the selected directory can be a different working copy of the
same repository; without the collision, `--execute` would move the file into
that checkout. A REVIEW row for "destination is a different working copy" (or
excluding such destinations) is the candidate fix; the index notice added here
does not disclose it.
