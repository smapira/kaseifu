# 残タスク: 一括 Undo がバッチ単位で固定されない（`undo --last N`）

- status: done (moved from `open/` 2026-10-15)
- milestone: M3 - Workflow semantics
- next owner: none (verify at merge time)
- blocker: needs design decision inside implementation
- 出典: `documents/plans/dev/20261015d-ai-rename-p8.md` D6、`20261015-reviewer-ai-rename-p8.md` R4
- 優先度: 中（状態は壊れないが、利用者が N を自分で数える必要がある）

## 現状

- 一括の実行結果に `undo: kaseifu undo --last N` を出すので、直前のバッチを戻すことはできる。
- しかし `undo --last N` は**バッチを選ばない**: 新しい順に N 行を戻すだけ。バッチ直後に単発で改名すると、N を自分で合わせないと単発分まで戻る。
- journal 表にバッチ/グループ列はなく、`operation` を変えると**既存の undo が拾わなくなる**（`operation = 'move'` で選択しているため。実測確認済み）。

## やること（どちらかを選ぶ）

- (1) `operations` に nullable な `batch_id` 列を足し、既存 DB を壊さない移行手順を入れる（`CREATE TABLE IF NOT EXISTS` の後に `ALTER TABLE` を試し、既存行は NULL のまま）。`undo --batch <id>` を追加する。
- (2) 現状のままにし、`undo --last N` が「新しい順に N 行」であることを README に明記する（バッチ単位の選択は提供しない）。
- 判断材料: 「新しい設定ファイル・別 DB なし」の制約は満たせるが、**既存ユーザーの DB を触る**変更になるため、優先度は利用者の判断。

## 受け入れ条件

- 選んだ方針が計画書に理由付きで残っている。
- (1) を選ぶなら、既存 DB（列なし）を開いても壊れないテストと、`--batch` の一巡テストがある。
- (2) を選ぶなら README に限界として書かれている。

## 制約

- 既存の行の意味を変えない。`operation` の値を変えない。
- CC 16 以上を作らない。テストは隔離 HOME のみ。

## 検証

- (1): 旧スキーマの DB を作ってから新バイナリで開き、`undo --batch` と `undo --last` の両方を実測。

## Verification (2026-10-15, worktree `kaseifu`)

Chose option (1) per the BDD plan's PM decision: nullable `batch_id` + `undo --batch <id>`. Rationale recorded in `documents/plans/dev/20261015j-backlog-bdd-plan.md` (additive migration, old DBs keep working, `--last` semantics byte-identical).

- Journal: `batch_id TEXT` via checked migration (`pragma_table_info`, genuine
  errors still surface); `PendingRecord` struct (keeps the 7-arg cap for
  clippy); `record_pending_in_batch`, `successful_moves_in_batch` (newest
  first). Read-only `inspect_journal` keeps the legacy column list so doctor
  reads old journals without migrating them.
- `execute_plan` keeps its signature (delegates with `None`); batch rows go
  through `execute_plan_in_batch`. `BatchReport.batch_id` is stamped by
  `apply` (`None` in previews). Batch output gains `batch: kaseifu undo
  --batch <id>` (text) and additive `undo_batch` (JSON).
- `undo::undo_entry` splits the per-row work out of `undo_last_move`; CLI
  `undo --batch <id>` (`conflicts_with last`; `last` is now `Option` with the
  same default 1) reuses the `UndoStep` text/JSON machinery and stops at the
  first refusal like `--last`.
- Tests: `tests/v104_batch_undo_scope_test.rs` (6 pass: legacy schema undo +
  batch-free rows, batch-newest-first with interleaved single untouched,
  `--last` after `--batch`, unknown id empty in text+JSON, batch JSON,
  flag conflict).
- Real-CLI spot check (isolated HOME): 2-file `--recursive --execute` batch
  applies with `undo_batch: batch-31714-...`, and `undo --batch` restores both
  newest-first.
- Regression: lib 227 pass; v097 (29), v093, phase5, v051 x2, v091 x2 green;
  v099 (10) green after the undo refactor. v092 shows only the known
  parent-owned `..._noop_for_current_names` conflict. clippy `-D warnings`
  clean, `cargo fmt --check` clean.
