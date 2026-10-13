# QA: organize 出力改善 4件

## スコープ

- `src/presentation/renderers/organize.rs`（新規）、`generic.rs`、`classify.rs`、`mod.rs`、`src/main.rs`（表示委譲のみ）、`tests/organize_output_test.rs`（新規）、`CHANGELOG.md`、計画・issue ドキュメント

## 静的チェック

| チェック | 結果 |
|---|---|
| ユーザー向け文字列への内部 enum 名混入（`NotAutoMove`/`UnknownCandidate`/`SourceMissing`/`SourceNotAFile`/`DestinationNotADirectory`/`Collision`） | ✅ なし（すべて `plan_error_message` の写像。テストで不在を固定） |
| `executed` の残存 | ✅ なし（`dry_run_banner` の新文言のみ。テストで不在を固定） |
| 非 ASCII の画面文字列 | ✅ なし（パネル・プレーンとも ASCII。`panels_are_ascii_only_...` / `plain_text_is_ordered_ascii_and_ansi_free`） |
| 秘密情報のハードコード | ✅ なし（新規ファイルに key/secret/token なし） |
| 死コード | ✅ なし（`print_organize_outcome` 削除、clippy `-D warnings` クリーン） |
| 既存アーキテクチャ | ✅ ライブラリ renderer パターン（classify/inspect/discover と同一）。`main.rs` の変更は表示委譲と import のみ、`Commands` enum / ディスパッチ未接触（並行 `completions` タスクと非衝突） |

## 実行チェック

| 検証 | コマンド | 結果 |
|---|---|---|
| テスト | `cargo test --all-targets --all-features` | ✅ 296 passed / 0 failed |
| 品質ゲート | `bash scripts/quality_check.sh` | ✅ **OVERALL PASS**（rustfmt / clippy `-D warnings` / cargo-test / complexity / baseline-gate / report-tests / coverage 74.08%） |
| 複雑度 | ゲート出力 | ✅ CC>=16: 0（Max 15 は既存）、新規関数は 4。`policy_view` 移動は moved として許容、`print_organize_outcome` 削除は note |
| 依存 | `git diff -- Cargo.toml Cargo.lock` | ✅ 空（新規依存なし） |
| 管理ファイル | `git diff -- .gitignore reports/quality/baseline.json` | ✅ 空（親管理のため未変更） |
| 新規ファイルの tracked 化 | `git status --short` | ✅ `A` として追加済（organize.rs / organize_output_test.rs / 計画書 / plan-architect ・ reviewer issue） |
| パス・リンク | 計画書・issue の参照先 | ✅ 存在（renderer・テスト・ソース行番号は現行と一致） |

## 実機確認（スクリーンション）

- `--format rich`（TTY）: ASCII パネル、モード行が先頭、`Selected ... - <候補ディレクトリ>`、`[LEAVE] LEAVE`、`policy refused: automatic move not allowed`
- パイプ（非 TTY）: プレーン（`== Mode ==` 〜）。ANSI なし
- `--format json`: 従来どおりプレーン行（JSON ではない）。分岐挙動不変
- `--execute` の実機再現は Keychain 許可ダイアログ未応答のため不可（環境ブロッカー。テストで表示契約を固定）

## 発見事項

- 🟡 注意（親の判断）: `--format json` の「出力不変」は文字列レベルでは成立しない（プレーン改善が同一経路に適用されるため）。分岐構造は不変で JSON 実装もなし。選択肢は計画書 §7 に整理
- 🟢 残課題（記録）: `MoveFailed` の文字列（lib 生成）と watch の一部文言（helper 非経由）は本タスク対象外

## 判定

**PASS**（Blocking なし。既存 279 + 新規 17 = 296 全緑、ゲート OVERALL PASS、新規 CC16+ なし、新規依存なし）
