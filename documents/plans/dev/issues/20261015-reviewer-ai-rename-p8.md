# reviewer 検収: AIリネーム P8（`20261015d-ai-rename-p8.md` の実装）

レビュー方式: 本ラウンドは親スレッドが実装したため、実装者と別の観点を強制するため次の順で行った: 受入条件の確定 -> 差分の読解 -> 仕様 -> 実装のトレーサビリティ -> 敵対的試行 -> 自動検証 -> 判定。根拠は `read_file` / `grep` / `terminal` の実測と、実機 CLI（隔離 HOME・合成ツリーのみ）の実行結果。

## 判定: 承認（REQUEST CHANGES なし。実装中に自分の不具合を2件検出して修正済み）

## 受入条件 -> 実装のトレーサビリティ

| 受入条件（原典） | 実装 | 実測の証跡 |
|---|---|---|
| §8.5 `--recursive` | `batch::walk` + `main::rename_batch` | R1、C1、`v097_cli_recursive_needs_a_folder` |
| §8.5 変更一覧（番号・件数） | `batch_item_lines` / `batch_summary_lines` | R1、C3、`v097_cli_an_empty_folder_is_an_empty_batch` |
| §8.5 衝突件数 | `hold_conflicts` | R2（2件保留）、単体 B4 |
| §8.5 `Apply changes? [y/N]` | `confirmed_by_prompt` | R6（PTY で出る / `--yes` で出ない / 非 TTY で出ない） |
| §8.5 一括 Undo | `undo --last N` + `undo:` 行 | R3、R4 |
| §9.1 上書き禁止 | 集合衝突 + 既存 `check_destination_free` + 適用時再確認 | R2、`v097_one_failing_item_does_not_stop_the_batch` |
| §9.2 TOCTOU | 既存の file / folder 再確認を再利用 | `v097_one_failing_item_does_not_stop_the_batch`（計画後にソースを消す） |
| §9.3 / SAFE10 | 1行ずつ `pending` -> `success`、`undo` は `pending` を無視 | コード読解 + 既存 `print_pending_warning`（変更なし） |
| §11.2 Batch / Bounded / Early Exit | 逐次、上限50、深さ3、非対応は着手前 held | R5、単体 B1-B3、`v097_an_unsupported_type_is_held_without_spending_a_model_call` |
| §12 失敗した単位はスキップ | `Applied::Failed` で続行 | `v097_one_failing_item_does_not_stop_the_batch` |
| §13.6 SAFE06-SAFE09 | dry-run 既定 / `--execute` / `undo` / 二重 undo 防止 | R1、R3、R4、C5 |
| §15 モデル不可の安全終了 | `require_model` | C5 の分岐、v097（stub ビルドで分岐を通る） |

## 敵対的レビューで試したこと

| 試行 | 実測結果 |
|---|---|
| 同じ名前を提案する2件を同時に入れる | 両方 `conflict`、ファイルは不変（R2） |
| 大文字小文字だけ違う2件 | 単体 B4 で同一視して保留 |
| 別ディレクトリの同名 | 衝突にしない（単体 B5） |
| 親フォルダと子を同じバッチで改名 | 子が先に適用され両方成功、`undo --last 2` で両方戻る（R3/R4、統合テスト） |
| 計画後にソースを消す | その1件だけ `Failed`、他は改名（統合テスト） |
| 上限を超えるツリー | 50件で打ち切り `bounded:`、0.024 秒（R5） |
| 非対応拡張子を混ぜる | モデル呼び出し0回で `held`（統合テストで request 数を固定） |
| 非 TTY で `--execute` | 問い合わせず完走（`/dev/null` から、C5） |
| PTY で `--execute` | `Apply changes? [y/N]` を出して入力待ち（確認として正しい）。`--yes` で出ない |
| JSON で `--execute` | 問い合わせず 1 文書（C3 のキー集合） |
| 空フォルダ | `proposals: 0` で exit 0、モデル要求なし（C3） |
| `--recursive` をファイルに | 明示エラー（C1） |
| `--yes` を単独で | clap エラー（C2） |
| 文書 / 画像 / 動画の単一経路 | `--recursive` 無しの挙動は不変。v092-v096 が無変更で通過 |

## 確認済み項目

- **journal 互換**: 一括の行も `operation = 'move'` のまま（既存 `undo` が拾える）。スキーマ・マイグレーションなし。`Cargo.toml` 無変更。
- **順序の保証**: `execution_order` が深い順を返し、包含関係で子の plan が無効化されない。表示順は歩いた順のままだが `order:` 行で明示している。
- **非ブロッキング**: 質問の条件は `--execute` + 非 JSON + 端末あり + `--yes` なし。パイプでは必ず省略される。
- **出力の規約**: 新規ソースの追加行に em dash なし（`git diff` を `grep`）。新規出力は ASCII（`->`、`bounded:`、`order:`、`undo:`）。
- **help 走査**: `--recursive` / `--yes` の説明に内部 Phase 語なし（`v070` の全 help 走査が PASS）。
- **複雑度**: 新規関数の最大 CC は 6（`rename_batch`）。CC>=16 = 0、最大 CC 15（ベースライン同）。
- **テストの隔離**: v097 の書き込み先は `std::env::temp_dir()` 配下と隔離 HOME のみ。実 Keychain・実補完ディレクトリ・実ユーザーフォルダに触れない。

## 実装中に自分で見つけて直した点（改善であり差し戻しではない）

- **確認待ちの表示が嘘だった**: `--execute` で `Apply changes?` を出す直前のサマリが `mode: dry-run (pass --execute to rename)` と出ていた（実機で検出）。`BatchPhase::{Preview, Awaiting, Result}` に分けた。
- **衝突理由の重複**: 同じ提案名が2件並ぶと `(reports.txt, reports.txt)` と出ていた。ソート + 重複除去して1回にした。

## 残る注意（QA・次ラウンドへ）

- Q1（QA へ）: `KASEIFU_FM_FORCE_STUB=no-sdk` のスイートと、`/dev/null` からの `--execute` が完走することを確認する。
- Q2（QA へ）: `git status` に無関係な変更が混ざっていないこと、実ユーザーのパスを読み書きしていないことを確認する。
- R1（次ラウンド）: 深さの上限は単体テストのみ（実機 CLI では55件の件数上限だけを実測）。
- R2（次ラウンド / Phase 9）: 逐次実行のコスト（1件あたり数秒〜十数秒）と、セッション再利用・並列度の評価。原典 §11.5 のベンチマーク対象。
- R3（既知の限界）: 計画時の衝突検出は「同一ディレクトリ + 小文字化した名前」で、正規化差は適用時の再確認に委ねる（上書きはしないが、プレビューでは提案として見える）。
- R4（既知の限界）: `undo --last N` は「バッチの行だけ」を選ばない。新しい順に戻るため状態は常に一貫するが、バッチ直後に単発改名した場合は N の数を利用者が合わせる必要がある。
