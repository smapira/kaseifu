# Rename subcommand renamed to `label` — done

- status: done
- completed: 2026-10-16 (user request)
- milestone: n/a - user-requested UX change (follow-up scope: none)
- next owner: none (verify at merge time)
- blocker: none

## Objective (as requested)

Change the `kaseifu rename` subcommand to `kaseifu label`.

## What changed

- `Commands::Rename` keeps its internal name and gains
  `#[command(name = "label", alias = "rename")]`: `label` is canonical,
  `rename` stays as a hidden transitional alias so existing scripts keep
  working.
- User-facing text now names `label` (guidance usage lines, `--patterns`
  pointer, `Label` / `Label patterns` / `Label preset` / `Label default` /
  `Label (batch)` titles, README usage, batch module docs).
- JSON keys (`"rename"`) and the journal shape are unchanged
  (additive-only contract); internal module and type names are unchanged.
- `tests/v108_label_subcommand_test.rs` (new, 5 tests): canonical guidance,
  alias resolves to the same help and works, top-level help lists `label`,
  JSON guidance keeps the stable key, `--patterns` lists the rules.
- `tests/v092_ai_rename_p34_test.rs`: completion test now pins the `label`
  arm instead of `rename`.
- CHANGELOG `[Unreleased]` records the change.

## Verification (2026-10-16, worktree `kaseifu`)

- `cargo test --test v108_label_subcommand_test`: 5 passed.
- `cargo test --all-targets --all-features`: 886 passed / 0 failed.
- `bash scripts/quality_check.sh`: OVERALL PASS (fmt/clippy/test/
  complexity/baseline-gate/report-tests green; CC>=16 = 0, max CC 15).
- Fixed en route: v092 completion pin (`rename` arm to `label` arm) and one
  clippy `-D warnings` unused import in the new test.
