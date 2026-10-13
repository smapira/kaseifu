# QA レコード: デバッグシナリオ設計成果物（v1.1 + v1.2 統合）

## 判定

**PASS（品質ゲート相当のチェックはすべて緑。🔴/🟡 なし。🟢 NIT 3 件は記録のみ）**

## スコープ

- 対象: `documents/plans/dev/20261014d-*.md`（7 冊）+ `documents/plans/dev/issues/20261014-{plan-architect,reviewer,qa}-debug-scenario*.md`（3 冊）
- 方法: 静的チェック（git / grep / パス存在） → テスト実行 → 要求トレースの抜き取り確認
- 本作業は文書のみで `src/`・`tests/` を変更しないため、テストは「現状の緑を壊していないこと」の確認として実行

## 1. 変更範囲の検証

| 検証 | コマンド | 結果 |
|---|---|---|
| 変更ファイル | `git --no-optional-locks status --short` | ✅ 新規 10 件（設計 7 + 検収 3）。並行タスクの変更なし |
| 差分（追跡ファイル） | `git --no-optional-locks diff --stat` | ✅ 空（src / tests / Cargo / CHANGELOG 不変） |
| 原典 | `documents/plans/user/20261013b.md` | ✅ untracked のまま（編集なし） |
| 原典 v1.2 | `documents/plans/user/20261013c.md` | ⚠ 本 worktree には未複製（親 worktree `21d5jjhnz4wd` に存在）。参照は親側で解決する。**コピーしない**（ユーザー管理・untracked） |
| CHANGELOG | `git status` | ✅ 未変更（コード変更を伴わないため） |

## 2. テスト実行

| 検証 | コマンド | 結果 |
|---|---|---|
| 全テスト | `cargo test --all-targets --all-features` | ✅ **279 passed / 0 failed / 0 ignored**（2026-10-13 実測、2 回実行で同値） |
| lib | 同上 | ✅ 51 passed |
| 統合 | 同上 | ✅ 228 passed（20 実行バイナリのうち空のテストファイル 2 件は 0） |
| CLI 実測の再現 | `kaseifu {--version,--help,help undo,discover --help,foo,classify --version,--format xml scan /tmp,undo --last abc,undo --last -1,inspect,config set,auth,watch,watch --path <file>,inspect <missing>,undo --last 0,scan <missing>,scan /tmp --format json,inspect <dir> --format json,inspect <space 入り PDF> --format json}` | ✅ C06 の表の値と一致 |

## 3. 品質チェック

| 観点 | 結果 |
|---|---|
| 要求トレース（v1.1 §7 の 6 項 + v1.2 §12 の 7 項） | ✅ 13/13 が成果物のいずれかに対応し、設計書 §0 の索引から辿れる |
| シナリオ網羅 | ✅ S01–S28（28）+ C01–C12（12）+ D01–D16（16、検出基準つき） |
| 実装との接地 | ✅ 設計書 §7 に「どのツールで何を確認したか」を記録。全 expected に `ファイル:行` の根拠を付与 |
| 反証可能な期待 | ✅ 終了コード・ジャーナル状態語・ファイル存在・sha256・ストリーム分離のいずれかで記述（抽象語を排除） |
| 重複の統合 | ✅ S26↔S07/S17、S27↔S02/S14、S28↔S06/S18、C09/C10↔S17、C11↔S05/S28 をカバレッジ表 §4 で明示 |
| 未カバーの可視化 | ✅ 動画（MP4/MOV）は「画像判定の対象外」として R7/F-11/カバレッジ表に明記。`--format json` の organize/undo/scan/watch は F-07/R9/C07 に明記 |
| 実装済みと設計提案の区別 | ✅ 設計書 §8 で二分類。各シナリオの `status: DESIGNED` は「設計済みでテスト未実施」の意味と明記 |
| 秘密情報 | ✅ `grep -iE 'api[_ -]?key\s*=|bearer |sk-'` = 0 件。座標ペアのリテラル 0 件。実在ユーザーパス 0 件（`/Users/<user>` のプレースホルダのみ） |
| ASCII 方針 | ✅ 罫線文字（U+2500 台）0 件。UI 例は `+--` / `|` / `==` / `'` のみ |
| 新規ファイルの tracked 化 | ✅ 10 ファイル（設計 7 + 検収 3）を `git add` 済（`git commit` / `push` / `tag` は実施せず = 親が実施） |
| ディレクトリ規約 | ✅ 設計書は `documents/plans/dev/`、検収記録は同 `issues/`。既存の命名（`<日付>-<内容>.md`）と一致 |

## 4. 発見項目

### 🟢 NIT-1 `completions` 未実装の再評価が必要

- C08 は設計のみ。並行タスク完了後に「`--help` の 11 コマンド / サブコマンド / `--format` の値」と補完候補の一致を再評価する。本レコードでは未達として扱う（設計上も「未実装領域」と記載済み）。

### 🟢 NIT-2 `doctor` の Keychain ハングは実環境依存

- QA 環境で `doctor` が 120 秒で完了しなかった（キー確認の可能性）。S25 / C11 に「タイムアウトを入れてハングを検出とする」と記録済みだが、再現条件（キーが存在する場合のみか）は未確定。次の実機確認で切り分ける。

### 🟢 NIT-3 pending 滞留の検出手段が SQL 直接参照のみ

- S05 / R1・R2 の検出は `select ... from operations where result='pending'` に依存する。検出 CLI（例: `undo --status` / `doctor` のジャーナルチェック）は設計提案側（設計書 §8）。本設計の範囲では追加実装しない。

## 5. 改善提案（任意・ブロックなし）

- 設計書 §3 の Wave 表をそのまま実行計画として使う場合、S05 の中断再現は「テスト用の一時ディレクトリ + タイムアウト付きの観測」を標準手順として先に整備すると、W0 の再現性が上がる
- S19（線返し分類）は実 Jev のレートを消費し得るため、記録様式だけ先に確定させ、実施は低頻度の時間帯に置くのが安全

## 備考

- QA は修正を行わない方針（`.github/agents/qa.agent.md`）。スキル未導入環境のため、QA スキルの手順（スコープ確認 → 静的チェック → 実行確認 → レポート → 改善提案）を本会話内でエミュレートして作成した
- 本レコードの grep 結果はすべて文字列検査であり、画像・バイナリは対象外（本成果物に含まれない）
