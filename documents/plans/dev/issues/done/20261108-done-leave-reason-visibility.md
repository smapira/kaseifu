# `organize` says why it left a file alone - done

- status: done
- completed: 2026-11-08 (GitHub issue [#20](https://github.com/smapira/kaseifu/issues/20); commit `b58e26a`)
- milestone: M1 - Reliability and CLI clarity
- next owner: none (verify at merge time)
- blocker: none

## The finding

The rich output printed `[LEAVE] LEAVE` and `policy refused: automatic move not
allowed` for two different states: a file with no candidate at all (probably
not indexed yet) and a file whose candidates missed `review_confidence = 0.60`.
The index state was only visible in `--format json` (`target_indexed`), so the
reader could not tell "wait for the index" from "pick a folder yourself".

## Decision

- Three reasons, not two: no candidate, below the review threshold, and a
  classifier answer of `NONE`. `NONE` is a decision, not a threshold miss, and
  saying so is the difference between "the tool could not see" and "the model
  declined".
- The zero-candidate message names the likely cause (the index may not know the
  file yet) and offers `--to <dir>`; the below-threshold message lists the first
  three candidates by ID and path and points at `--select`, because the
  candidates exist and the reader needs to see them.
- The renderer reads the candidate count and index state from the report instead
  of re-deriving them, so the text and `--explain` cannot disagree.

## What changed

- `src/operations/organize.rs`: the report keeps the discovered candidate list
  (needed by both the renderer and the explanation).
- `src/presentation/renderers/organize.rs`: `leave_reason` branches on
  candidates-present / candidate-count / `target_indexed`, and the POLICY
  section lists the offered candidates and the next step.
- `src/presentation/renderers/explain.rs`: `Candidates` and `Index` rows, and
  `candidate_count` / `target_indexed` in the explanation JSON (additive; a
  decided verdict keeps its exact rows).
- `tests/user-test-plan.csv`: T92 (zero candidates), T93 (below threshold),
  T94 (`--explain`).
- `tests/v117_leave_reason_test.rs` (new, 10 tests).

## Verification (2026-11-08)

- `tests/v117_leave_reason_test.rs`: 10 passed, including
  `v117_the_two_leave_reasons_differ` and `v117_json_stays_additive`.
- CLI run with no network (isolated HOME): zero-candidate output carries the
  index hint and the `--to` line, `Candidates 0` and `Index` rows appear, and
  the JSON gains `candidate_count: 0` / `target_indexed: false`.
- `bash scripts/quality_check.sh` at `96b946b`: OVERALL PASS (1044 tests).
