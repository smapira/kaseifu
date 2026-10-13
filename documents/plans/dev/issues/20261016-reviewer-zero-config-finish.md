# Reviewer レビュー: ゼロコンフィグ仕上げバッチ（ZC-11 / ZC-9 / ZC-10）

- 日付: 2026-10-16
- 対象: `documents/plans/dev/20261016b-zero-config-finish.md` の実装（未コミット差分 + 新規 5 ファイル）
- 原典: `documents/plans/user/20261013d.md` §6 / §7 / §12
- 判定: **APPROVE**（blocking 0 / MINOR 4 / NIT 2。下記は記録と注意点で、差し戻し事項ではない）
- 確認手段: 実コード読解 + `git diff` + 実 CLI（隔離 HOME・一時 `KASEIFU_COMPLETION_DIR`・`env` モード）+ ベースライン `d436451` の実バイナリ実行比較 + `bash scripts/quality_check.sh`

## 受入条件→実装トレーサビリティ

| 受入条件（原典） | 実装 | 独立検証 |
|---|---|---|
| ZC-11(1) ドライランで削除ゼロ・対象とサイズ表示 | `uninstall::plan`（読み取りのみ・`SecretStore` を受け取らない）+ `report_lines` の summary | 実 CLI（a）: `1 target(s), 64 B total (dry run; nothing was deleted)`、全ファイル残存。library テストで plan 後の存在を固定 |
| ZC-11(2) `--execute` で補完のみ削除 | `execute` の completion 行のみ `WouldRemove` | 実 CLI（c）: `removed Completion: ...`、config/journal/旧ジャーナル残存。library テスト（config/journal/legacy の存在 + spy 0 呼び出し） |
| ZC-11(3) `--purge --execute` で設定・ジャーナル削除 + `--remove-key --yes` | plan の action=Remove（purge 時）+ `execute_keychain` | 実 CLI（d）: 4 target 削除（completion/config/journal/旧 CWD）。library テストで `MemoryStore` からの削除を固定（`get()==NotFound`） |
| ZC-11(4) `env` モードで Keychain 不接触 | plan 段の `keychain_allowed()` ゲート + execute 段の 2 重ゲート | spy ストアで get/delete 0 回を固定。`env` の `execute` 時に confirm クロージャがパニックしないことも固定。実 CLI（d）で `not touched in env mode` 表示 |
| ZC-11(5) 2 回実行の冪等 | 存在しない対象は `Absent`、summary が `nothing to remove` | 実 CLI（e）と library テストで固定（exit 0） |
| ZC-11(6) 移動済みファイル・他ディレクトリ旧ジャーナル不可侵 | 対象は CWD の `data/kaseifu.db` のみ・読めた場合のみ削除 | 実 CLI（d）: `other/data/kaseifu.db` と `MovedPhotos/photo.jpg` が残存 |
| ZC-11(7) テストは一時ディレクトリと注入経路のみ | `tests/v080_zero_config_uninstall_test.rs`（library 17 ケース中 9 ケース + CLI 8 ケース） | 全テストが隔離 HOME + 一時 CWD + `KASEIFU_COMPLETION_DIR` + `env` モード。実 Keychain 不使用をテストコメントとコードで確認 |
| ZC-9 語彙統一 | `generic::watch_line`（常時モード表示）+ `print_watch_start` 一本化 + `outcome_text` | ベースライン実バイナリ: `watching ... (dry-run: true)` → 新: `watch ... start (dry run (no files moved))` / `(execute mode)`。WouldMove 行は `would move a -> b (dry run (no files moved))`。テスト 3 本固定 |
| ZC-10 README 短縮 | 冒頭 `## Quick Start` + 詳細は後段 | `## Quick Start` が `## How it works` より前、旧情報（ビルド・鍵・非TTY・setup/doctor/素の実行）を保持、`](#...)` リンク全件解決をテストで固定 |
| §10 非機能（秘密・安全・ASCII・互換・品質） | 下記の敵対的確認 | quality_check.sh OVERALL PASS / 450 passed / CC>=16 0 / 依存追加 0 / `discover` 非表示維持 |

## 敵対的確認（実施済み）

1. **秘密**: uninstall は鍵の値を一切表示しない（表示は service 名と成否のみ）。`env` モードは Keychain を読みも削除もしない（spy 0 回 + CLI 実測）。テストのダミー秘密が stdout/stderr/JSON に出ない。`.env` の中身を読む・出すテストは無い。
2. **壊しにいく入力**: `uninstall --execute --remove-key`（非TTY・keychain モード・`--yes` なし）は削除前に exit 1 で拒否（実 CLI（b））。`uninstall --help` は ASCII のみ・禁止語 0。未知フラグは clap エラー。
3. **部分削除の可視化**: 補完の削除失敗（読み取り専用ディレクトリ）でも config/journal の削除は完了し、`FAIL Completion: ... (not deleted: ...)` と summary の `failed (partial result; nothing is rolled back)` が出る（library テスト）。
4. **旧ジャーナル**: CWD の `data/kaseifu.db` は `--execute --purge` のみ。他ディレクトリは構造的に探索されない（`UninstallPaths.legacy_journal` は 1 パスのみ）。読めないファイルは削除せず `keep ... not deleted, check it yourself`。
5. **冪等**: 2 回目は全行 `none`/`keep` + `nothing to remove (already clean)` で exit 0。
6. **回帰**: ベース 428 が全て緑のまま（`v052_help_still_lists_every_command` は uninstall 追加を意図的に反映）。`KASEIFU_KEY_SOURCE`・ドライラン既定・`discover` 非表示・既存 JSON キー不変は維持。
7. **補完の再生成との関係**: main の自動更新除外に `Uninstall` を追加（実行直前に再生成されない）。案内文に「バイナリが残る限り次回実行で再生成」「`cargo uninstall kaseifu`」を明示。
8. **watch の他出力**: `detected` / `ignored` / `moved` / `review` / `no move` / `error` 行は不変（`outcome_text` の差分を目視 + 既存 phase6 テスト緑）。

## 指摘事項

### 🟡 MINOR-1. uninstall は削除失敗でも exit 0

原典は exit code を規定せず、`doctor` の「FAIL 行があっても 0」の前例に合わせた。失敗は `FAIL` 行 + summary の `failed (partial result...)` で見える。スクリプトが失敗を検知したい場合は JSON の `status: "failed"` を読む（計画 §7-1 に記録）。

### 🟡 MINOR-2. 非TTY の確認要求は「鍵が無くても」拒否する

`--execute --remove-key`（`--yes` なし）は非TTYで常に拒否する。鍵の有無を先に調べるには Keychain 読みが必要となり、非TTYで誘導確認が出る可能性を排除できないため（`doctor` も Keychain を読まない同じ原則）。`--yes` を案内する。

### 🟡 MINOR-3. uninstall JSON はコンパクト出力

doctor / auth / config / setup と同じ `to_string()`（1 行）。classify / organize の pretty とは異なるが、レポート系 JSON の既存慣行に合わせた。

### 🟡 MINOR-4. 読めない旧ジャーナルは保持

`inspect_journal` が失敗するファイルは `--execute --purge` でも削除しない（内容不明の履歴を壊さない。`keep ... not deleted, check it yourself` と表示）。原典の「列挙・案内のみ」の範囲内。

### 🟢 NIT-1. `--yes` は単独で受け付ける（no-op）

`--remove-key` / `--purge` なしの `--yes` は何も変えない。help に「used with --remove-key or --purge」と明記。clap の `requires` では「`--purge` との併用」を表現できないため許容。

### 🟢 NIT-2. 補完ディレクトリの `_kaseifu` がシンボリックリンクの場合は保持

自動更新がシンボリックリンクを置換しない既存規則に合わせた（`symbolic link; not written by kaseifu, left alone`）。

## 備考

- 上記 MINOR/NIT はいずれも差し戻し不要。APPROVE として QA へ引き継ぐ。
