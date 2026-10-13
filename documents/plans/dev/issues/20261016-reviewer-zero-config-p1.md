# Reviewer レビュー: ゼロコンフィグ P1（ZC-5 / ZC-6 / ZC-7 / ZC-8 / ZC-12 / ZC-13）

- 日付: 2026-10-16
- 対象: `documents/plans/dev/20261016a-zero-config-p1.md` の実装（未コミット差分 + 新規 6 ファイル）
- 原典: `documents/plans/user/20261013d.md` §5 / §8 / §9 / §12
- 判定: **APPROVE**（blocking 0 / MINOR 4 / NIT 2。下記は記録と注意点で、差し戻し事項ではない）
- 確認手段: 実コード読解 + `git diff` + 実 CLI（モック Jev・PATH 注入 mdfind・隔離 HOME・実 PTY）+ `bash scripts/quality_check.sh`

## 受入条件→実装トレーサビリティ

| 受入条件（原典） | 実装 | 独立検証 |
|---|---|---|
| ZC-5 近接表示と `--explain`、既存表示・JSON 不変 | `explain.rs`（`NEAR_MOVE_WINDOW=0.05`・実効 `Thresholds`） / `classify::render_panel_with` / `organize::render_text_with` / `json_report(explanation)` | レンダラ 9 テスト + CLI 実測（0.840 → `near auto move: 0.010 more confidence...`）。`render_panel` 旧シグネチャは委譲で完全一致 |
| ZC-6 3 値の意味・閾値・`--execute` 必須 | `Cli` の `after_help` | `-h`/`--help` 両方を実測 + テスト固定 |
| ZC-7 decision / policy / outcome の JSON | `organize_view::json_report`（`decision` / `policy` / `candidate_path` / `mode` / `outcome.type`・`kind`・`reason`） | 実 CLI 3 形態（would_move / moved / unplanable）+ ジャーナル行の独立検査 |
| ZC-8 対象/対象外の明記 | README スコープ節 + help `Scope:` + R6 3 行 | README の拡張子を機械抽出して `is_image_file` と照合（9/9）。`doctor` の 9 チェックは不変 |
| ZC-12 禁止語 0 / フラグ実在 / 安全性 / ASCII | main.rs の doc コメント書き換え + `--format` の対応範囲明記 | 18 対象×2 フラグ=36 出力を実バイナリで走査（禁止語 0・ASCII）。`v052`・`completion_test` 緑（`discover` 非漏洩 0 件） |
| ZC-13 PTY 選択・中断・非TTY・除外・undo 整合・鍵不要 | `operations/select.rs` + `renderers/select.rs` + `build_manual_move_plan` + `execute_manual_move` + `handle_organize_select` | PTY 4 本（選択実行 / 確認辞退 / q + 不正入力 / SIGINT）+ 非TTY + JSON。実 PTY 手動ログも取得 |
| §10 非機能（秘密・安全・ASCII・互換・品質） | 下記の敵対的確認 | quality_check.sh OVERALL PASS / 428 passed / CC>=16 0 / 依存追加 0 |

## 敵対的確認（実施済み）

1. **秘密**: TTY select は鍵プロンプトを出さず（`resolve_for` のみ）、`.env` を書かず、env モードで Keychain に触れない。テスト用ダミー鍵が stdout / stderr / JSON に出ないことをテストで固定。`.env` の中身を読む・出すテストは無い。
2. **壊しにいく入力**: `--select --explain` → clap が排他エラー。`--yes` 単独 → `--select` 必須エラー。`--select --format json --execute` → 明示エラー（沈黙無視なし）。存在しないファイル → `cannot resolve ...` で exit 1（パニックなし）。メニューへの `9` / 空入力 → 再入力（範囲エラー表示）、`q` / `Q` → 変更 0。
3. **no-op 防止**: 選択肢は Discovery が対象の親ディレクトリを除外して生成（既存規則）。仮に親が候補になっても計画層の衝突検査で拒否される（同一パスは `exists()` に当たる）。テストで親が候補に出ないことを固定。
4. **除外規則**: `~/Library/**` / `/var/**` / `*.library/**` / 生成名フォルダは候補に出ない（偽 mdfind で注入して確認）。`build_manual_move_plan` は `is_excluded_destination` を再検査し `DestinationExcluded` で拒否。
5. **ジャーナル整合**: 人間移動は `candidate_id` / `confidence` / `margin` が NULL で記録され、`undo` はそれを引き継いで復元、`doctor` は `ok Journal: ... 2 operation(s); no pending` と表示（PTY テストで実測）。
6. **回帰**: P0（v052 / key_source_cli / completion / organize_output / v31_render / phase5）を含む既存 387 が全て緑。`KASEIFU_KEY_SOURCE`・`discover` 非表示・ドライラン既定は不変。
7. **ネットワーク**: select は Jev を構築しない（`JEV_API_URL` を閉ポートにしても完走）。`--select` のテストはネットワークなしで決定論的。

## 指摘事項

### 🟡 MINOR-1. `--explain` の JSON キー順がアルファベット順になる

`serde_json::to_value` 経由で `explanation` を挿入するため、`--explain` 指定時のみキー順が変わる（値・キーは完全保持。既定出力は従来の構造体順で不変）。JSON オブジェクトのキー順は意味を持たないため許容。計画書 §9 T3 に注記済み。

### 🟡 MINOR-2. ヘルプの閾値は既定値

`--help` は user TOML を読まないため既定値（0.85 / 0.40 / 0.60）を表示し、実効値は `kaseifu config show` に誘導する。`--help` を設定ファイル依存にしないための意図的な設計（計画 §3.2）。

### 🟡 MINOR-3. `--select` に「Jev の提案」行を出さない

原典の例示（「Jevの提案: D1」）は、同じ項の「**Jev は呼ばない**（鍵不要で動作する）」と両立しないため、Jev の選択を一切表示しない形で決定権を人間に残した（計画 §7.1 に理由を記録）。受入基準 1〜7 は全て満たす。将来提案を出す場合も決定には使わない前提を明記。

### 🟡 MINOR-4. `dispatch` の CC 14（warn-only）

`main` を 16 → 2 に下げるために抽出した `dispatch` が 11-15 帯（警告のみ、ゲート非阻害）。CC>=16 は 0、最大 CC 15、CC<=10 比率 99.4% を維持。将来サブコマンドが増える場合は分割が必要。

### 🟢 NIT-1. `--yes` は `--select` を要求

`organize --yes` 単独は clap エラー（未使用フラグの沈黙無視をしない）。仕様にない組合せだが安全側。

### 🟢 NIT-2. メニューの score 表示は小数 2 桁

Discovery のスコアは相対値であり、既存 `discover` 表示（`{:.2}`）と揃えた。

## 備考

- 上記 MINOR/NIT はいずれも差し戻し不要。APPROVE として QA へ引き継ぐ。
