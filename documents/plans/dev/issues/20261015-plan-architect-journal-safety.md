# Plan Architect レビュー: ジャーナル安全性と設定の正直さ（20261015b）

## 優先度
🟡 中（実装前に反映すべき指摘 2 件、実装時に注意すべき点 3 件）

## 対象
- 計画書: `documents/plans/dev/20261015b-journal-safety-config-honesty.md`
- 関連ファイル: `src/doctor.rs`, `src/storage/history.rs`, `src/discovery/{similarity,destinations}.rs`,
  `tests/v33_config_auth_test.rs`, `tests/key_source_test.rs`, `src/main.rs`

## 検証した項目（コードベース実測）

| 観点 | 結果 |
|---|---|
| 既存ユーティリティの再利用 | `config::storage::app_dir()` / `dirs::data_dir()` を再利用する計画で重複なし。`OperationHistory` に read-only 検査を追加するのは既存 `Connection` 抽象の延長で妥当 |
| 依存方向 | `storage::history` → `config::storage` は葉モジュールへの参照で循環なし（`config` は `storage` を参照していない。grep 実測） |
| 重複実装 | R8 は `destinations.rs` に一本化する計画で、`similarity.rs` の二重判定を除去。既存 `is_excluded_destination` の権威コメント（`src/discovery/candidates.rs:31-33`）とも整合 |
| テスト計画 | R8/R5/R1/R2/F-05/F-12 それぞれに正常系・異常系・境界がある（計画書 §2）。ただし下記 F1 の修正が必要 |
| JSON 互換 | `config show` は既存キー `operations.dry_run` / `operations.enable_undo` を維持し `inactive_settings` を追加（既存キー削除なし）。`doctor --format json` は `checks[]` へ追加のみで互換 |
| 実ユーザーデータ | 新規テストは隔離 HOME / 一時ディレクトリで計画済み |

## 指摘事項

### F1（🔴 高）: 既存の `doctor::diagnose` テストが実ジャーナルを読むようになる

`tests/v33_config_auth_test.rs:497,519,538` と `tests/key_source_test.rs:380,407,418,433,448,467` は
`kaseifu::doctor::diagnose(..)` を直接呼ぶ。doctor にジャーナル検査を追加すると、これらのテストが
実ユーザーの `~/Library/Application Support/kaseifu/kaseifu.db` を**読み取り**（書き込みはしないが、受け入れ基準 C5-6「テストは注入経路のみ」に違反）、
開発機の状態で診断結果が変わる。

**改善案**: 検査を注入可能な `diagnose_with(config, check_connection, journal_path, legacy_path)` に分離し、
既存テストは一時パス（存在しないファイル）を渡すよう更新する。既定パスの解決は CLI 統合テスト（隔離 HOME）で担保する。

### F2（🟡 中）: `journal_path()` の置き場所

`journal_path()` を `storage::history` に置き `config::storage::app_dir()` を参照する方針は妥当（F1 の依存方向実測）。
`config` 側にジャーナルの知識を持たせると「設定」と「実データ」の責務が混ざるため、計画どおり history.rs に置く。
`LEGACY_DATABASE_PATH` は history.rs に「検出専用・移行しない」を明記して置く。

### F3（🟡 中）: `undo` 警告の出力先と不変条件

既存の `undo` の結果出力（`restored:` / `nothing left to undo`）は stdout（`src/main.rs:427-446`）。
警告も同じ stdout に置く（F-07 の「stdout=人間向けテキスト」に整合）。Stre に出すと非TTY パイプで消える。
また `pending` 警告は SELECT のみで、Undo 対象条件（`result='success' AND undone_at IS NULL`）とジャーナルを**変更しない**ことをテストで固定する。

### F4（🟢 低）: doctor はジャーナルを作らない

`OperationHistory::open` は親ディレクトリ作成 + スキーマ初期化をするため、doctor から使わない。
`SQLITE_OPEN_READ_ONLY` + 存在チェックで開口し、未作成なら `Ok(None)`。テストで「doctor 実行後にファイルが存在しない」を固定する。

### F5（🟢 低）: ドキュメントの残存記述

`documents/development.md:132` の「application defaults (config/default.toml values)」は
default.toml を実行時未読とする本変更と矛盾するため、`src/config/defaults.rs` を指すよう更新する。

## 改善案まとめ（計画への反映）

1. T5 に「既存 `diagnose` 呼び出しテスト 9 箇所を `diagnose_with`（一時パス）へ更新」を追加。
2. T8 に `documents/development.md` の 1 行修正を追加。
3. 計画書 §7（検収記録）に本レビューの反映を追記。

## 備考

上記 3 点を計画・タスクへ反映済み（本ファイルはその記録）。他は計画を承認する。
新規 CC 16+ を作らないための関数分割（検査・整形）は実装時に意識する（計画 §6 に記載済み）。
