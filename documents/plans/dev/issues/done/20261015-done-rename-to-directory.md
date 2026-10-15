# 残タスク: ZC-14 `--to <dir>`（明示した移動先を指定する）

- status: done (moved from `open/` 2026-10-15)
- milestone: M3 - Workflow semantics
- next owner: none (verify at merge time)
- blocker: actionable
- 出典: `documents/plans/user/20261013d.md`（ゼロコンフィグ化 改善計画 v1.0）ZC-14
- 優先度: 中（非 TTY の案内文が「将来の `--to <dir>`」を名指ししており、未実装のまま利用者に見えている）

## 現状（実測、`f519188`）

- `src/main.rs` に `--to` は 0 件。`kaseifu rename --help` にも出ない。
- `organize --select` の非 TTY 案内が代替手段として `--format json` と将来の `--to <dir>` を挙げている（原典 §5）。

## やること

- `organize --to <dir>`（または `--select --to`）を実装し、対話なしで移動先を指定できるようにする。
- 移動先の検証は既存の除外規則（`discovery::destinations::is_excluded_destination`）と「存在するディレクトリであること」を再利用する。
- 自己の親ディレクトリなど no-op になる指定を拒否する。

## 受け入れ条件

- 非 TTY でも対話なしで移動できる。`--format json` と併用可能。
- 除外規則に落ちる移動先は理由付きで拒否される（上書きしない）。
- journal / undo は既存経路のまま動く。
- help に内部 Phase 語を出さない（`tests/v070_zero_config_p1_help_test.rs` が全 help を走査）。

## 制約

- 新しい設定ファイル・別 DB・独自キャッシュ・外部プロセスなし。CC 16 以上を作らない。
- テストは隔離 HOME と一時ディレクトリのみ。実ファイルを動かさない。

## 検証

- 実機: 隔離 HOME の一時ツリーで `--to` による移動 → `undo` 一巡。
- 非 TTY の実行でハングしないこと（`</dev/null`）。

## Verification (2026-10-15, worktree `kaseifu`)

- `organize --to <dir>` standalone (`conflicts_with_all = ["select",
  "explain"]`): validation reuses `plan::build_manual_move_plan` (exclusion,
  missing source, non-dir, collision order) plus an explicit canonical-path
  no-op refusal (`already in <dir>; nothing to move`). Execution reuses
  `mover::execute_manual_move` (manual journal path, NULL Jev columns), so
  `undo` reverses it. No confirmation prompt (non-interactive is the point;
  `--execute` is the explicit request).
- Text: new `select_view::render_explicit_plan` / `render_explicit_moved`
  (`KASEIFU / TO [MANUAL]` frames; basis names the flag instead of the
  wrong "you selected it"). JSON: additive `{"organize_to": {...}}` doc
  (dry run `executed: false`, execute carries `operation_id`).
- `SELECT_NON_TTY_GUIDANCE` now names the real `--to <dir>` (keeps the
  `--to <dir>` substring the v070 select test pins).
- Tests: `tests/v102_rename_to_directory_test.rs` (7 pass: dry-run plan +
  nothing moved/journaled, execute+undo round trip, JSON dry-run+execute,
  excluded `.app` refusal, missing/file-dest refusal, own-parent no-op,
  `--select` conflict). Destinations are relative in tests because the temp
  tree sits under `/var/folders`, which the reused system-root rule refuses
  (correct for real use; documented in the test).
- Real-CLI spot check (absolute paths under `~/kaseifu-to-spot`, isolated
  HOME, `</dev/null`): dry-run plan, `--execute` move (`op-14057-...`), and
  `undo` restore all succeed; scratch dirs removed afterwards.
- README gained a `--to` paragraph. v070 select/help suites green (8+10).
  clippy `-D warnings` clean, `cargo fmt --check` clean.
