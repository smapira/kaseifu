# Reviewer 検収レビュー: デバッグシナリオ設計成果物（v1.1 + v1.2 統合）

## 判定

**APPROVE WITH COMMENTS（🟡 2 件・🟢 1 件はいずれも修正済み。納品可）**

## スコープ

- 対象: `documents/plans/dev/20261014d-debug-scenario-design.md`、`20261014d-scenarios-S01-S10.md`、`20261014d-scenarios-S11-S20.md`、`20261014d-cli-scenarios-S21-S28.md`、`20261014d-cli-scenarios-C01-C12.md`、`20261014d-cli-coverage-map.md`、`20261014d-existing-test-coverage.md`
- 原典: `documents/plans/user/20261013b.md`（v1.1 §7）、`documents/plans/user/20261013c.md`（v1.2 §12/§13）
- 方法: 要求事項のトレース、実装との突き合わせ（サンプル 24 箇所の `ファイル:行` を再読）、実測値の再現、差分検査

## 1. 要求事項のトレース

| 要求 | 成果物 | 結果 |
|---|---|---|
| v1.1 §7-1 D01–D16 一覧 | 設計書 §2（検出基準列つき） | ✅ 16 件すべてに「何をもって検出とするか」と対応シナリオあり |
| v1.1 §7-2/3 S01–S20 の完全仕様 | S01–S10 冊 + S11–S20 冊 | ✅ 20 件（各に preconditions / inputs / expected / failure_conditions / observation_points / evidence / existing_test / status） |
| v1.1 §7-4 既存テストとの対応表 | `existing-test-coverage` | ✅ 実測 279 件の内訳と関数名レベル対応 |
| v1.1 §7-5 優先度別の実施順序 | 設計書 §3 | ✅ W0 = S05 → S01 → S04 → S02 → S06（原典指定）を先頭に固定 |
| v1.1 §7-6 未検証領域・リスク | 設計書 §4（R1–R12） | ✅ リスク降順、各に実装根拠と未確認事項 |
| v1.2 §12-1 S21–S28 詳細 | `cli-scenarios-S21-S28` | ✅ 8 件（ケース表も併記） |
| v1.2 §12-2 C01–C12 横断 | `cli-scenarios-C01-C12` | ✅ 12 件 + 実測終了コード表 |
| v1.2 §12-3/4/5/6 | `cli-coverage-map` | ✅ コマンド対応表 / サブコマンド・オプション表 / 重複・依存 / P0–P2 |
| v1.2 §13 完了条件 | 設計書 §9 | ✅ 11 コマンド（+ discover）、サブコマンド、正常系/異常系/境界、反証可能な期待、重複、未カバー、実装済/提案の区別 |

## 2. 実装との突き合わせ（サンプル検査）

再読して一致を確認した主要な根拠（抜粋）:

| クレーム | 実装 | 一致 |
|---|---|---|
| Policy の比較は `>=`、0.85/0.40/0.60 | `src/policy/decision.rs:14-18,76-79` | ✅ |
| `NONE` / 未選択は confidence に関係なく Leave | `src/policy/decision.rs:52-56,69-74` | ✅ |
| 候補外 ID は pipeline で Err | `src/classifier/pipeline.rs:238-245`、`src/models/decision.rs:115-132` | ✅ |
| 移動は verify -> record -> rename -> mark | `src/operations/mover.rs:43-59,80-99` | ✅ |
| Undo 対象は success かつ undone_at IS NULL | `src/storage/history.rs:140-154` | ✅ |
| Undo は上書きしない | `src/operations/undo.rs:76-78` | ✅ |
| `--last N` は反復し失敗で break | `src/main.rs:427-431` | ✅ |
| debounce 500ms / stability 1s / self 10s | `src/watch/coordinator.rs:32-43` | ✅ |
| 自己イベントは source と destination を登録 | `src/watch/runner.rs:112-115` | ✅ |
| GPS は既定 OFF、マーカーも出さない | `src/config/defaults.rs:34`、`src/classifier/jev.rs:476-490,543-562` | ✅ |
| 欠損は推測しない（mtime を使わない） | `src/context/image_metadata.rs:75-98`、`src/normalize/image.rs:12-70` | ✅ |
| 除外規則は path 形状ベース | `src/discovery/destinations.rs:66-170` | ✅ |
| 上限 10,000 / 50 はランキング後 | `src/discovery/mod.rs:31,39,110-117` | ✅ |
| scan はバナー出力のみ | `src/main.rs:268-280` | ✅ |
| JSON 非対応コマンド（organize/undo/scan/watch） | `src/main.rs:268-280,307-361,402-433` + 実測 | ✅ |
| doctor の副作用は app ディレクトリ作成のみ | `src/doctor.rs:128-134` | ✅ |
| 実測終了コード 0/1/2 | 再実行で再現 | ✅ |

## 3. 発見項目と修正

### 🟡 REV-1 設計書 R9 行に編集残骸

- 対象: `20261014d-debug-scenario-design.md` §4 リスク表 R9 の「対応シナリオ」列
- 指摘: 「S24? ではなく C07、S09、S28」という検討途中の文字列が残っていた（読者に意図が伝わらない）
- 修正: 「C07、S09、S28」に統一（修正済）

### 🟡 REV-2 カバレッジ表の件数と集計の不一致

- 対象: `20261014d-cli-coverage-map.md` §3 末尾
- 指摘: 「`no` 4 件」と書いていたが、表の実数は 6 件（inspect PATH / scan PATH / discover FILE / setup flag / doctor flag / help）
- 修正: 6 件に訂正（修正済）

### 🟢 REV-3 行参照が曖昧

- 対象: S12 の evidence
- 指摘: 「`src/operations/undo.rs:101` の前に検証」は範囲が読みにくい
- 修正: 「`:57-83`（前検証）、`:92-101`（record_pending）、`:103`（rename）」に分解（修正済）

## 4. 制約の確認

| 制約 | 確認 | 結果 |
|---|---|---|
| `src/` を変更しない | `git diff --stat` | ✅ 差分 0（並行タスクの編集も本worktreeでは未検出） |
| `tests/` を変更しない | 同上 | ✅ |
| 原典 2 ファイルを編集しない | git status では既存の untracked のみ | ✅ |
| テストコード・Mock・Fixture・CI を作らない | 新規ファイルは `.md` のみ | ✅ |
| 実ユーザーパス・鍵・位置情報を書かない | grep（ユーザー名 / 実在絶対パス / API キー形 / 座標ペア） | ✅ 実在パス 0 件。`/Users/<user>/...` のプレースホルダ 1 件のみ（S02 の入力例） |
| ASCII 表示方針 | 罫線文字 0 件（UI 例は `+--` / `|` / `'` / `==`） | ✅ |

## 5. 情報（プロックしない）

- `completions` は未実装。C08 は「未実装領域」として設計のみ。並行タスク完了後に、`--help` の一覧と補完候補の一致を再評価する必要がある
- `doctor` が実環境で 120 秒完了しなかった事象（Keychain アクセス確認の可能性）は S25 / C11 の未確認事項として記録済。**再現手順には必ずタイムアウトを入れる**
- v0.4.2c の計画書（`20261014c-...md`）の実測記録を S20 の回帰基準の前例として引用した。形式は本成果物と整合的なため、追加のフォーマット変更は不要

## 6. 検証実行記録

| 検証 | コマンド | 結果 |
|---|---|---|
| 全テスト | `cargo test --all-targets --all-features` | ✅ 279 passed / 0 failed（本成果物はコードを変更しないため実測値は同一） |
| CLI 実測の再現 | `kaseifu {--version,--help,foo,--format xml,undo --last abc,inspect <missing>,watch --path <file>}` | ✅ C06 の表と一致 |
| 差分検査 | `git --no-optional-locks status --short` / `git diff` | ✅ 新規 `.md` のみ |
