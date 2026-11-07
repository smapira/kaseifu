# Spotlight scope and path shape - done

- status: done
- completed: 2026-11-08 (GitHub issue [#19](https://github.com/smapira/kaseifu/issues/19); commit `aa3c0ff`)
- milestone: M1 - Reliability and CLI clarity (a repository checkout winning
  against a user folder is a wrong answer in ordinary use)
- next owner: none (verify at merge time)
- blocker: none (coefficient calibration against a real index needs the release;
  see Follow-ups)

## The finding

`mdfind` ran without `-onlyin` (`src/platform/macos/spotlight.rs`), so the query
covered the whole index. A picture on the Desktop ranked
`.../agent-bank/public/system/images/11/email` first (confidence 0.34), and
`MAX_SPOTLIGHT_HITS = 10_000` was spent on unrelated areas. The ranking had no
term for depth, "inside a working tree", or "this is the standard folder": only
filename, token and extension, then directory aggregation.

## Decision

- The search is scoped, not filtered after the fact: `mdfind -onlyin <root>`,
  home by default, one invocation per root, results merged up to the limit. An
  empty root list falls back to home; only when home cannot be resolved does the
  query run unscoped, exactly as before.
- `--roots DIR` (repeatable) names the scope on `organize`, `classify` and the
  diagnostic `discover`. Another volume is searched only when it is named.
- Path shape is a **subtraction, never an exclusion** (the `destinations.rs`
  style): `-0.015` per level beyond three below home (capped at eight levels),
  `-0.18` inside a git working tree, `+0.10` for the standard folder of the
  target's type. The score is clamped to 0.0, so a penalised directory stays a
  candidate, and each adjustment is reported in the candidate's reasons.
- The adjustment sits next to the age adjustment on the directory score, not on
  the per-file score: it is a fact about the directory, and the same fact must
  not be counted once per matched file.
- A default `DirectoryShape` changes no number, so historical callers keep exact
  rankings.

## What changed

- `src/platform/macos/spotlight.rs`: `search_in`, `search_in_with` (injectable
  runner), `search_arguments` (pure, asserted without starting a process),
  `default_roots`.
- `src/main.rs`, `src/discovery/mod.rs`: `--roots` wired through one
  `scoped_search` closure, so `organize`, `classify` and `discover` share the
  scope instead of growing a second discovery path.
- `src/discovery/scoring.rs`: `DirectoryShape`, `adjust_for_shape`,
  `depth_below_home`, `is_inside_git_worktree`.
- `src/discovery/candidates.rs`: `aggregate_by_directory_with_shape`, called by
  `aggregate_by_directory_with_exclusions` with a default shape.
- `tests/v119_spotlight_scope_test.rs` (new, 10 tests). The two legacy exact-match
  tests (`v042`, `v042c`) now assert the ranking and the 0..1 range instead of
  `score == 1.0`, because the fixture lives inside a git working tree and is
  penalised by design.

## Verification (2026-11-08)

- `tests/v119_spotlight_scope_test.rs`: 10 passed (`-onlyin` present per root,
  repeated roots, no scope when home is unresolved, a git-worktree path does not
  outrank an equally similar user folder, reasons reported, score clamped).
- Isolated `mdfind` stub: the invocation log reads
  `ARGS: -onlyin <root> <query>`.
- Plan §6 records the before/after indicator (a repository path tied with
  `~/Pictures/2024` before; the user folder wins after).
- `bash scripts/quality_check.sh` at `96b946b`: OVERALL PASS (1044 tests,
  max CC 15, coverage 84.81%).

## Follow-ups

- [`#23`](https://github.com/smapira/kaseifu/issues/23): the coefficients were
  calibrated with fixtures, not a real index (the issue forbids measuring the
  developer's home in tests), and `watch` / `config` cannot carry a scope yet.
  A real `~/Desktop` run is the calibration data for the next adjustment.
