# `organize` after `git mv`: what the index claim actually was

Last updated: 2026-10-19. Status: implemented and verified.

Source: GitHub issue #11 (2026-10-19, kaseifu 0.12.4). Reported after
`git mv ai-generated-text.md scripts/ai-tools/ai-generated-text.md`:

1. `organize` reported `destination already exists: <root>/ai-generated-text.md`
   while `ls` said the path was gone.
2. The classifier proposed the pre-move directory with confidence 0.950 and
   never proposed the new location.

## 1. Objective

Answer two questions with evidence rather than with the issue's hypothesis,
then make the smallest coherent change that leaves the user able to read the
verdict correctly:

- Is destination existence ever decided by the Spotlight index? (Acceptance 1
  of the issue: it must be `stat`.)
- When the classifier's input may be stale, does the verdict say so?
  (Acceptance 2.)

Constraints kept: dry run stays the default, `--execute` stays the only
mutation path, journal/undo semantics are untouched, no new crate, no new
configuration key, no new external process.

## 2. Root cause, with evidence

### 2.1 Confirmed by reading the code

- `destination already exists` exists in exactly one place:
  `plan_error_message` (src/presentation/renderers/organize.rs) for
  `PlanError::Collision`.
- `PlanError::Collision` is built only by
  `checked_destination_path` (src/operations/plan.rs), from
  `destination_path.exists()`. `mover::verify_preconditions` repeats the same
  `exists()` immediately before a real move. That is a filesystem `stat`, not a
  query. `Path::exists()` is also true for a directory and for a broken case
  the filesystem reports, but it never consults the index.
- The index cannot inject a phantom path either: discovery drops every hit
  whose path is not on disk (`similarity.rs`, `if !path_obj.exists() {
  continue; }`), before scoring. A stale hit is therefore not evidence, and
  the pre-move directory can only be proposed through **other files that
  really exist there**.

### 2.2 Measured on this machine (2026-10-19)

Reproduction used a throwaway `git init` repository under `$HOME`
(`$HOME/kaseifu-issue11-repro`, deleted afterwards), never the real checkout:

| Step | Result |
|---|---|
| `mdfind -onlyin <repo> ai-generated-text` right after `git mv` | no output at all: the whole new tree was still unindexed |
| `[ -e <root>/ai-generated-text.md ]` right after `git mv` | MISSING |
| same `mdfind` at t+5s .. t+30s | only the **new** path; the old path never came back |

The staleness is real, but it is "missing new evidence", not "invented old
evidence". Measured run of the shipped binary on the issue's own path
(`KASEIFU_DEBUG=1 kaseifu discover scripts/ai-tools/ai-generated-text.md`):

```text
raw terms: ["Markdown", "ai", "text", "generated"]
mdfind query: 'Markdown ai text generated'
Spotlight raw results: 744
SimilarFile candidates (after dedup + filter): 576
Candidate destinations: 459 (returned 50)
D1 /Users/bookair18/OS/home/Codes/github.com/routeflags/seo-operations
   score=1.00  reasons: found 1 items / established: ~14 days /
   exact normalized filename / extension: md / match: ai, generated, text
```

The reported state was then identified: this machine holds the same repository
in several checkouts. Four of the five Delta worktrees of `seo-operations` have
the file at `scripts/ai-tools/ai-generated-text.md` and no file at the root
(mtime 2026-10-19 21:42 to 22:30), while the primary checkout and one older
worktree still have it at the root. Running the read-only discovery from a
moved worktree selects the primary checkout (`D1` above), and the primary's root
file is the `exact normalized filename` evidence.

`D1` scored 1.00 on **one** item, and the reason is `exact normalized
filename`, which means a file literally named `ai-generated-text.md` was on
disk in that directory. The checkout the issue names was inspected read-only:

```text
<root>/ai-generated-text.md          683 bytes, mtime == ctime 2026-10-06 02:33
git ls-files -- ai-generated-text.md ai-generated-text.md  (tracked)
git status --short                   (clean)
<root>/scripts/ai-tools/             ai_tool_check.py only
```

### 2.3 Conclusion

- The collision was **true**: a file with that name really was in the proposed
  directory, and it was found by `stat`.
- The classifier's proposal was **legitimate** (that file is the exact-name
  evidence), not a stale-index ghost. The index lagged in the other direction:
  the new path was not in the index yet (measured 0s..5s window), so the new
  directory could not compete.
- The two observations were both true, of **different checkouts**. The verdict
  named the primary checkout's root (`<primary>/ai-generated-text.md`, tracked
  there since 2026-10-06); the reporter `ls`-ed the root of the worktree they
  had moved the file in. "The repo root" was ambiguous: the tool meant the
  primary, the reporter meant their worktree.
- The worktree's new location is not a candidate by design: a file's own
  directory is never a destination (moving a file to where it already is
  changes nothing), so that expectation cannot be met by any ranking change.
- What *was* missing is disclosure: the verdict never said that the index had
  no entry for the file, so a reader could only explain the output by guessing.

## 3. Behavior units (as implemented and verified)

| ID | Behavior |
|---|---|
| B1 | A hit whose path is gone is dropped before scoring, so it never becomes a candidate. A stale entry cannot make a directory look like a destination. |
| B2 | Destination existence stays a `stat`: with no file at the destination a plan builds and reports `Would move`; with a real file it reports `REVIEW / destination already exists: <that file>`. The reported path is the path that was `stat`-ed. |
| B3 | Discovery reports whether Spotlight knew the target itself (`SimilarFiles::target_indexed`), recorded at the canonical comparison the scan already performs (no extra `canonicalize`, no extra process). |
| B4 | `organize` renders one `Index  not indexed yet; candidates may be incomplete` row under DECISION when that flag is `Some(false)`, in plain text and in the Rich panel (amber), and never otherwise (`Some(true)` / `None`). |
| B5 | `--format json` gains one add-only key, `target_indexed` (`false` / `true` / `null`). No existing key is removed or retyped, and `outcome.kind` is unchanged. |
| B6 | `None` means "no claim": with no search terms no search runs and no row appears. |
| B7 | The notice changes nothing else: policy, plan gate, `--execute`, journal and undo are untouched. With the notice present the same run still reports `AUTO MOVE` and `Would move`. |
| B8 | `watch` shows the same row, because it renders through the same organize report. |

## 4. Decisions

- **Notice, not a fallback walk.** Acceptance 2 allowed either. Discovery here is
  machine-wide (744 raw hits and 459 candidate directories measured for one
  markdown file), so an unbounded filesystem walk would change the candidate
  set and the cost of every run, on every file, to fix a case where a file had
  been moved seconds earlier. The notice is additive, exact, and leaves the
  ranking untouched.
- **The signal is the hit list, not a clock.** "The target itself was not among
  the hits" is a fact from the search that already ran. A freshness window on
  `ctime`/`mtime` was rejected: every freshly downloaded file has a new
  `ctime`, which is the primary use case, and every test fixture would trip it.
- **Absence is not an error.** With `None` (no terms) nothing is claimed; with
  `Some(false)` the verdict is shown unchanged and only one row is added. A
  brand-new file legitimately has no index entry, so refusing to move it would
  be wrong.
- **`stat` stays the only existence check.** Switching the collision check to a
  Spotlight query would create exactly the bug the issue describes.

### Rejected alternatives

1. **Query Spotlight for the destination instead of `stat`** - the current code
   already uses `stat`; this would introduce index dependence into the one place
   that must not have it.
2. **Filesystem-walk fallback when the target is unindexed** - unbounded scope
   and cost (see above), and it would silently change candidates rather than
   tell the user what is uncertain.
3. **Suppress or re-rank the pre-move directory when the target is unindexed** -
   silently changes decisions, and kaseifu cannot identify "the pre-move
   directory": `git mv` leaves no trace it can read, and a manual move is not in
   the journal.
4. **Refuse auto-move when the target is unindexed** - turns a display fact into
   a policy change and would refuse legitimately new files.
5. **A `ctime` freshness window** - fires on every fresh download and every test
   fixture; the hit-list signal needs no constant.
6. **A different message for a collision with the file itself** (`--to <own
   dir>`) - the sentence is accurate there (the file is already in that
   directory); left unchanged to keep the wording stable.

## 5. Files changed

| File | Change |
|---|---|
| `src/discovery/similarity.rs` | `SimilarFiles` (files + `target_indexed`) and `scan_similar_files_with`; the flag is set at the canonical comparison the scan already does. `find_similar_files` / `find_similar_files_with` keep their signatures. |
| `src/discovery/mod.rs` | `DiscoveryOutcome` and `discover_with_outcome`; `discover` / `discover_with` unchanged wrappers. |
| `src/classifier/pipeline.rs` | `classify_with_candidates_and_search` (injectable search, returns the discovery outcome); `discover_candidates_with_options` now returns the outcome; existing entry points keep their signatures. |
| `src/operations/organize.rs` | `OrganizeReport::target_indexed`; `organize_file_with_search` seam so the whole verdict is testable without the machine's index. |
| `src/presentation/renderers/organize.rs` | `Index` notice row (amber in Rich) and the add-only `target_indexed` JSON key. |
| `tests/organize_output_test.rs`, `tests/v070_zero_config_p1_explain_test.rs` | Report literals gain `target_indexed: None`. |
| `tests/v110_stale_index_notice_test.rs` | New: 7 tests (see below). |
| `README.md`, `CHANGELOG.md` | Spotlight section states the row; `[Unreleased]` entry. |
| `documents/plans/dev/issues/done/20261019-done-git-mv-stale-index.md` | Issue record. |

## 6. Verification

| Command | Result |
|---|---|
| `cargo test --test v110_stale_index_notice_test` | 7 passed, 0 failed |
| `cargo test --all-targets --all-features` | 907 passed, 0 failed, 5 ignored (baseline 900 passed / 0 failed; +7) |
| `bash scripts/quality_check.sh` | `OVERALL: PASS` - rustfmt 0, clippy 0, cargo-test 907/0, CC >= 16 = 0, max CC 15, coverage 83.96% (report-only) |

Manual checks: the throwaway `$HOME` repository for the `git mv` timing
measurements (removed afterwards), `KASEIFU_DEBUG=1 kaseifu discover` against
the issue's file, and read-only `stat`/`git ls-files`/`git status` on the
`seo-operations` checkout. No `--execute` run was performed anywhere, and no
user file, journal, Keychain entry or completion directory was written.

Residual risk: the notice can also appear for a file that Spotlight will never
index (a hidden tree, a privacy-list path). The wording ("not indexed yet;
candidates may be incomplete") stays true there, and a run with no candidates
produces no plan at all.

**Follow-up decision (not part of this fix):** candidates are machine-wide, so
the selected directory can be another working copy of the same repository
(measured above: the primary checkout). With the blocking file absent,
`--execute` would have moved the file into that other checkout. Whether a
destination in a different working copy should be a REVIEW row (recommended) or
excluded is a product decision; the index notice does not disclose it.
