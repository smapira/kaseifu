# Reviewer 検収レビュー: organize 出力改善 4件

## 対象
- 実装差分（`git diff`）: `src/main.rs`、`src/presentation/renderers/{generic,classify,mod}.rs`、新規 `src/presentation/renderers/organize.rs`、新規 `tests/organize_output_test.rs`、`CHANGELOG.md`
- 計画書: `documents/plans/dev/20261013d-organize-output-plan.md`
- 受入条件: 親スレッドの 5 項目 + 「報告のみ」項目

## トレーサビリティ（受入条件 → 実装/テスト）

| 受入/要件 | 実装 | 検証 |
|---|---|---|
| 1. 選択先パス表示（NONE は no destination） | `organize::decision_lines` + `shown_path`（`relative_display`/`paint_path`） | `decision_shows_the_selected_directory_before_policy_and_result`, `none_selection_shows_no_destination_without_a_path`, `unresolved_selection_is_labelled_and_never_invents_a_path`, `rich_emphasizes_...`, `rich_truncates_...`, `home_relative_paths_...` |
| 2. 「executed」是正 + 全コマンド一貫 | `generic::dry_run_banner` → `execute mode` / `dry run (no files moved)` | `execute_mode_is_a_mode_not_a_moved_claim`, `dry_run_banner_wording_is_shared_with_watch` |
| 3. 内部 enum 名排除 | `organize::plan_error_message`（6 変種を人間向け文に）+ `generic::policy_view` 再利用 | `plan_errors_are_human_readable_sentences`, `refused_and_unplanable_share_the_policy_refused_wording`, `policy_section_reuses_the_classify_wording` |
| 3b. Collision は REVIEW 維持 | `result_heading` → (`REVIEW`, `REVIEW`)、文言 `destination already exists: ...` | `collision_keeps_the_review_result`（T37 の「REVIEW: destination already exists」と整合） |
| 4. 順序とパネル統一 | `render_panel`（`frame_with_sections`）+ `render_text`、mode → DECISION → POLICY → result | `decision_shows_...`（順序）, `plain_text_is_ordered_ascii_and_ansi_free`, `panels_are_ascii_only_and_share_one_visible_width` |
| 5. 文言・順序のテスト | `tests/organize_output_test.rs`（17 件） | 全緑 |
| 非回帰: 移動時表示 | `Moved` は Destination + Operation、`WouldMove` は From + Destination | `moved_reports_destination_and_operation_id`, `would_move_shows_source_and_destination_paths` |
| 非回帰: `--format json` 分岐不変 | 分岐構造に未接触（`resolve_mode` も不変） | 実機: json は従来どおりプレーン行（親報告参照） |

## 敵対的検証（実行したもの）

- `cargo test --all-targets --all-features` → 296 passed / 0 failed（新規 17 件含む。`policy_view` 移動後も classify テスト緑）
- `cargo fmt --all -- --check` / `cargo clippy --all-targets --all-features -- -D warnings` → クリーン
- `bash scripts/quality_check.sh` → **OVERALL PASS**
- 実バイナリで確認: `--format rich`（パネル）、TTY auto（パネル）、パイプ（プレーン）、`--format json`（プレーン・JSON ではない）。旧文言 `executed` / `NotAutoMove` / `Some("D1")` / `Leave {` は出力に現れない
- 不変条件: `git diff --stat` で判断・実行・journal・policy のコードに差分なし。`OrganizeOutcome` の構造は不変（機械可読な原因は保持）
- 境界: NONE/None/unresolved/長パス/HOME 相対/色ありなし/execute モード — テストで固定

## 指摘事項

### 🟡 MAJOR（対応済・記録）: `--format json` の文字列はプレーン改善の対象

受入基準2を「文字列バイト不変」と解釈すると成立しない（`--format json` は通常プレーンと同一経路で、項目3はまさにその文字列を変える）。判断: 分岐構造を変えない（JSON 化しない）ことを優先し、最終報告で修正前後の全文を提示して親の判断に委ねる。

### 🟢 MINOR（残課題・本タスク対象外）: `MoveFailed` の文字列は依然 Debug 風

`operations/organize.rs:123` の `format!("{error:?}")` は watch の `ProcessOutcome::Failed` にも流れる共有データで、lib 側の変更になるため表示タスクでは触らない（計画に記録済）。

### 🟢 MINOR（残課題）: watch の一部文言は helper 非経由

`print_watch_start` の非 rich 文言 `watching ... (dry-run: true)` と `watch/runner.rs` の `dry-run {path} ...` は矛盾する「executed」を含まない別文言。統一するなら別タスク（計画に記録済）。

### 🟢 MINOR（親の判断待ち）: ユーザーテスト計画の期待文言

`tests/user-test-plan.csv` の T34（「Dry run」）はモード行 `dry run (no files moved)`（小文字）が、T42/T74 の括弧内 enum 名は人間向け文が対応する。手動計画の文言更新は親（ユーザー判断）に報告する。

## 判定

**APPROVE**（Blocking なし。MAJOR は仕様解釈の記録で、実装は「分岐不変・文字列改善」の解釈を明確に選択。MINOR はすべて残課題として記録済）
