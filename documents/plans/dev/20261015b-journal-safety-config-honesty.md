# ジャーナル安全性と設定の正直さ（R1/R2/R5/R8/F-05/F-12）実装計画

- 起票: 2026-10-15
- 原典: `documents/plans/dev/20261014d-debug-scenario-design.md`（R1/R2/R5/R8, F-05/F-12, §8 設計上の提案）
- 対象: `src/discovery/{similarity,destinations}.rs`, `src/storage/history.rs`, `src/doctor.rs`,
  `src/main.rs`, `src/watch/coordinator.rs`, `src/config/{defaults,loader}.rs`,
  `config/default.toml`, `README.md`, `CHANGELOG.md`, 新規テスト 4 ファイル
- 制約: 実ユーザーデータを壊さない（`~/Library/Application Support/kaseifu/`・`~/Desktop/`・旧 `./data/` をテストで読み書きしない）/
  既存ジャーナルの移行・削除・上書き禁止 / 新規依存なし / `--format json` の既存キーを消さない /
  `discover` はデバッグ専用のまま / `KASEIFU_KEY_SOURCE` の挙動不変 / commit・tag・push は親が実施
- ベースライン実測（2026-10-15、HEAD `5511c72`）: `cargo test --all-targets --all-features` = **346 passed / 0 failed**（lib 51 + 統合 295）

## 1. 背景と現状実測（変更前）

| ID | 現状（実測・コード根拠） | 影響 |
|---|---|---|
| R8 | `is_system_directory` が `contains` 部分一致（`src/discovery/similarity.rs:200-207`）: `/system`, `/private`, `/usr`, `/bin`, `/sbin`, `/var`, `/tmp` | `~/var/...`, `~/tmp/...`, `~/system-backup`, `~/private-docs` が候補から消える。既存テストのフィクスチャが `std::env::temp_dir()` を避けている理由（`tests/v042c_destination_exclusion_test.rs:27-29`）もこの副作用 |
| R5 | ジャーナルが CWD 相対 `data/kaseifu.db`（`src/storage/history.rs:16`、使用: `src/main.rs:329,373,416`、`src/watch/coordinator.rs:38-40`） | 実行ディレクトリを変えると別ジャーナルになり Undo 対象が消える。`doctor` はパスを出さない |
| R1 | `rename` 成功後に `mark_success` が失敗すると pending 残留（`src/operations/mover.rs:93-99`）。Undo 対象は `result='success' AND undone_at IS NULL` のみ（`src/storage/history.rs:140-154`） | この移動は戻せず、検出手段も無い（`doctor` はジャーナルを見ない） |
| R2 | シグナルハンドラ無し（`grep -rn "signal\|SIGINT\|ctrl_c" src/*.rs` = 0 件）。`record_pending`→`rename`→`mark_success` は非トランザクション | 中断点により pending が残留し得る |
| F-05 | `config/default.toml:1-10,36-37` に `[scanner]` `[context]` `[storage] database = "data/semantic-file-organizer.db"`。実装は定数 `data/kaseifu.db`。**default.toml は実行時に誰も読まない**（`grep` で読取コード 0 件、`include_str!` 0 件） | 死んだ記述（幻のパス・幻の設定） |
| F-12 | `operations.dry_run` / `enable_undo` は解決される（`src/config/resolver.rs:201-205`）が挙動に未接続（`src/main.rs` は CLI フラグのみ使用）。README:301-302 は「Default to dry run unless `--execute`」と実装と食い違う説明 | 設定したのに効かない（静かな無効化） |

### ユーザー TOML スキーマの制約（重要）

`UserConfig` は `#[serde(default, deny_unknown_fields)]`（`src/config/loader.rs:12-23`）で 4 セクションのみ。
`operations.dry_run` / `enable_undo` を**削除すると既存ユーザー TOML がエラーになる**ため、受理は継続する（互換）。
`[storage]` / `[scanner]` / `[context]` はそもそもユーザー TOML に存在しない（書けば未知キーでエラー）。

## 2. 振る舞い（BDD シナリオ / 具体例）

### C1. R8: システム領域判定（規則ベース）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C1-1 | 正常系: ユーザーフォルダ | `/Users/test/var/photos`, `/Users/test/tmp/mail`, `/Users/test/system-backup`, `/Users/test/private-docs` は**除外されない**（旧実装は contains で除外 = 回帰テストになる） |
| C1-2 | 正常系: システム領域 | `/System/Library`, `/usr/share`, `/private/var/folders/x`, `/var/log`, `/tmp/x`, `/bin`, `/sbin` は除外 |
| C1-3 | 境界: 深い位置の同名 | `/Users/test/System Settings`（深い `System`）は除外しない。`/Volumes/Work/usr/manuals` も除外しない |
| C1-4 | 境界: 相対パス | 先頭が `/` でない文字列（`var/photos`）はシステム領域扱いしない |
| C1-5 | 統合: 候補到達 | 実 FS 上に `root/var/photos` 等を作り `discover_with` に渡すと候補として残る。`aggregate_by_directory` に `/usr/share` 等の合成 `SimilarFile` を渡すと落ちる（権威ゲート） |
| C1-6 | 二重管理の排除 | `similarity.rs` の `contains` 判定を撤去し、規則は `destinations.rs` のみ（権威をコメントで明示） |

### C2. R5: ジャーナルの安定絶対パス

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C2-1 | 既定パス | `journal_path()` = `<app_dir>/kaseifu.db`（macOS: `~/Library/Application Support/kaseifu/kaseifu.db`）。絶対パスで CWD に依存しない |
| C2-2 | 実行経路 | `organize` / `undo` / `watch` / `WatchConfig::default().managed_paths` が同パスを使う |
| C2-3 | doctor 表示 | `ok Journal: <resolved path> (...)` を表示。**doctor はジャーナルを作らない**（無副作用の読み取り検査） |
| C2-4 | 旧ジャーナル検出 | CWD に `./data/kaseifu.db` がある場合 `FAIL Legacy Journal: found ./data/kaseifu.db (old CWD-relative journal; not read, not migrated, not deleted; current journal: <path>)` を表示。ファイルには一切触れない |
| C2-5 | 移行しない | 旧ファイルの自動移行・マージ・削除はしない（設計判断 §3.2 に記録） |

### C3. R1/R2: pending の「検出して報告」

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C3-1 | 合成ジャーナル（新規） | 一時ディレクトリの DB に `record_pending` した行を `inspect_journal` で検出（`Ok(Some(..))`）。行数・pending 数・対象ファイルの現況（source/destination の存在）を返す |
| C3-2 | 古い pending | `inspect_journal_at(path, now + 2h)` で `stale = true`。1h 以内は `stale = false`。timestamp が不正なら stale 扱い |
| C3-3 | ジャーナル無し | `inspect_journal(missing) == Ok(None)`。読み取り専用でファイルを作らない |
| C3-4 | doctor 報告 | pending 行で `Journal` チェックが件数・古い pending・対象ファイル状態を表示。stale があれば `FAIL`。**自動削除・自動復旧しない**（doctor は読み取りのみ） |
| C3-5 | undo 報告 | pending 行があると `undo` は先頭で `warning: ... pending journal row ... interrupted files may already be at the destination (not recovered automatically; run \`kaseifu doctor\` ...)` を stdout に表示。Undo 対象条件（success のみ）は不変 = pending は Undo されない |
| C3-6 | 二重実行なし | pending 行があっても `undo` は通常どおり最後の success を戻せる（pending を対象にしない）。2 回目も同じ pending 警告 + `nothing left to undo` |
| C3-7 | 自動復旧しない理由 | 「不整合の自動解消はデータを壊し得る」をコードコメント（history.rs / undo 警告）と本計画 §3.3 に記録 |

### C4. F-05/F-12: 設定の正直さ

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C4-1 | default.toml | `[scanner]` `[context]` `[storage]` を削除。残るセクションの値は `src/config/defaults.rs` / policy 定数と一致（テストで固定） |
| C4-2 | ユーザー TOML 互換 | `[operations] dry_run = true / enable_undo = true` を含む既存 TOML が引き続きロードできる（エラーにならない） |
| C4-3 | config show（text） | `dry_run = true (inactive; moves are controlled by --execute)` / `enable_undo = true (inactive; every executed move is journaled)` |
| C4-4 | config show（JSON） | 既存キー `operations.dry_run` / `operations.enable_undo` を維持したまま `inactive_settings` を追加 |
| C4-5 | キー検証不変 | `config set` の未知キー拒否・API キー拒否・値検証は従来どおり |
| C4-6 | README | 設定表・Undo・Doctor・project structure の記述が実装と一致 |

### C5. 品質ゲート

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C5-1 | テスト | 既存 346 + 新規が全緑。`v33_doctor_local_checks_without_network` の `checks.len() == 7` のみ 9 へ更新（doctor の検査追加は意図的変更） |
| C5-2 | 複雑度 | 新規 CC 16+ なし。baseline.json の更新は不要な見込み |
| C5-3 | 依存 | 追加クレート・追加依存なし |
| C5-4 | 品質スクリプト | `bash scripts/quality_check.sh` が OVERALL PASS |
| C5-5 | 無干渉 | `discover` は `#[command(hide = true)]` のまま。`KASEIFU_KEY_SOURCE` の既定（debug=env）不変。ASCII/NO_COLOR/非TTY 不変。`--format json` の既存キー不変 |
| C5-6 | 安全 | テストは実 `~/Library/Application Support/kaseifu/`・`~/Desktop/`・旧 `./data/` を読み書きしない（注入経路と隔離 HOME のみ） |

## 3. 設計判断（決定と理由 / 記録）

### 3.1 R8 の規則化と権威の一本化

- 判定は「絶対パスの**先頭構成要素**が `/System` `/private` `/usr` `/bin` `/sbin` `/var` `/tmp`（大小無視）か」に限定する。
  macOS のシステム領域はルート直下にしかないため、これで `~/var` 等のユーザー名を巻き込まない。
- 規則は `destinations.rs`（`is_excluded_destination`）に置き、**そこが唯一の権威**とする。`similarity.rs` の重複判定は撤去し、
  ヒットの親ディレクトリを既存の `is_excluded_destination` で判定する 1 経路に統合する（二重管理の排除。C1-6）。

### 3.2 R5 のジャーナルパス

- 既定は `journal_path()` = `config::storage::app_dir()` + `kaseifu.db`。既存のユーザー設定パス（`~/Library/Application Support/kaseifu/config.toml`）と同じ
  ディレクトリに置き、`dirs::data_dir()` 経由（ハードコードのホームを持たない）の既存方式を再利用する。
- **旧 `./data/kaseifu.db` は移行しない・上書きしない・削除しない**。理由: (1) 実ユーザーデータを勝手に動かすのは破壊的、
  (2) 旧 DB と新 DB の内容同一性を保証できない（CWD ごとに別ファイルがあり得る）、(3) 設計書 §8 のとおり「まず検出可能性」。
  `doctor` の `Legacy Journal` チェックで存在を報告するのみ。既存ジャーナルの自動マージ・移行は本タスクの対象外（次バッチ以降の設計課題として記録）。
- `DEFAULT_DATABASE_PATH`（const &str）は `journal_path()`（絶対）と `LEGACY_DATABASE_PATH`（検出専用）に置き換える。旧 const は残さない
  （CWD 相対の値を使う経路を型として消すため）。

### 3.3 R1/R2 の検出（自動復旧しない）

- 検出は読み取り専用（`SQLITE_OPEN_READ_ONLY`）。`doctor` はジャーナルを作らない。`undo` は既存どおり open するが、pending は**報告のみ**。
- stale 閾値は 1 時間（`PENDING_STALE_AFTER_SECS = 3600`）。同一ジャーナルを別プロセスが使用中の「進行中 pending」と、
  中断で残った pending を区別する目安。timestamp が不正なら stale 側に倒す（保守的）。
- **自動復旧・自動削除は実装しない**。理由: pending は「rename 前」「rename 後 mark_success 前」のどちらでも残り得て、FS を見ないと確定できない。
  自動で success/failure を書き換えたりファイルを戻したりすると、正しいデータを壊す（rename が実は失敗していた場合の巻き戻し等）。
  S05 の要求どおり「まず検出できること」に限定し、復旧は人間の判断に委ねる。この理由を `history.rs` の検査関数と `undo` 警告のコメントに書く。
- Undo の対象条件（`result='success' AND undone_at IS NULL`）は**変更しない**。pending 行は Undo 対象にしない（誤って「成功した移動」と混同しない）。

### 3.4 F-05/F-12 の設定

- `config/default.toml` は実行時未読のドキュメント。**`[scanner]` `[context]` `[storage]` を削除**する。
  理由: 実装が読まない（配線するには scanner/context の実装とユーザー TOML スキーマ拡張が必要で本タスクの範囲外。`[storage]` を読ませると
  ジャーナルパスの権威が 2 つになる）。実行時未読のため既存ユーザーへの影響は無い（ユーザー TOML は別ファイル）。
  ファイル冒頭に「実行時未読・既定値は defaults.rs・ジャーナルは app_dir」を明記する。
- `operations.dry_run` / `enable_undo` は**受理継続 + 無効の明示**。強制 dry-run の安全側インターロック案は**不採用**:
  既定 `true` のため「全移動を止める」落とし穴があり、`--execute` の CLI 契約（README・既存テスト）を壊す。
  代わりに `config show`（text/JSON）と README で inactive を明示する。
- `config set` のキー検証（未知キー拒否・API キー拒否・値検証）は不変。

## 4. タスク一覧（実装順）

| # | タスク | 主なファイル | 期待する振る舞い（受け入れ） | テスト |
|---|---|---|---|---|
| T1 | R8: `destinations.rs` にシステムルート規則を追加（先頭構成要素・絶対パスのみ・大小無視）、権威コメント | `src/discovery/destinations.rs` | C1-2, C1-3, C1-4 | 単体（destinations.rs 内） |
| T2 | R8: `similarity.rs` の `contains` 判定を撤去し `is_excluded_destination` 1 経路へ統合 | `src/discovery/similarity.rs` | C1-1, C1-5, C1-6 | `tests/v051_system_directory_rule_test.rs`（新規） |
| T3 | R5: `journal_path()` / `LEGACY_DATABASE_PATH` 導入、呼び出し 3 箇所 + `WatchConfig::default` 更新 | `src/storage/history.rs`, `src/main.rs`, `src/watch/coordinator.rs` | C2-1, C2-2 | `tests/v051_journal_path_test.rs`（新規） |
| T4 | R1/R2: `inspect_journal`（read-only）/ `pending_entries` / stale 判定を history.rs に追加 | `src/storage/history.rs` | C3-1〜C3-3, C3-7 | 上記 + `tests/v051_journal_pending_test.rs`（新規） |
| T5 | R5/R1: `doctor` に `Journal` / `Legacy Journal` チェック追加（注入経路 `diagnose_with`） | `src/doctor.rs`, `tests/v33_config_auth_test.rs`, `tests/key_source_test.rs` | C2-3, C2-4, C3-4。**既存 `diagnose` 呼び出しテスト 9 箇所（v33: 3 / key_source: 6）を一時パスの `diagnose_with` へ更新**（Plan Architect F1: 実ジャーナルを読ませない） | 新規統合（隔離 HOME）/ `v33` の件数更新 |
| T6 | R1: `undo` の pending 警告（stdout、自動復旧なしの注記） | `src/main.rs` | C3-5, C3-6 | 新規統合（合成 DB + 実バイナリ） |
| T7 | F-05/F-12: default.toml 整理 + スキーマ/既定値コメント更新 + `config show` の inactive 明示 | `config/default.toml`, `src/config/{defaults,loader}.rs`, `src/main.rs` | C4-1〜C4-5 | `tests/v051_config_honesty_test.rs`（新規） |
| T8 | ドキュメント: README（設定表・Undo・Doctor・structure）+ CHANGELOG `[Unreleased]` + `documents/development.md:132` の解決順記述 | `README.md`, `CHANGELOG.md`, `documents/development.md` | C4-6 + Plan Architect F5 | 目視 + 品質ゲート |
| T9 | 品質ゲート実行・証跡化（fmt/clippy/test/complexity、必要時のみ baseline 更新） | `reports/quality/`（生成物は git 管理外） | C5-1〜C5-4 | `bash scripts/quality_check.sh` |

## 5. 受け入れ基準（実測で示す）

1. ユーザーフォルダ（`~/var`, `~/tmp`, `~/system-backup`, `~/private-docs` 相当）が候補になり得る（テスト + 実測）。
2. 新規ジャーナルが絶対パス `~/Library/Application Support/kaseifu/kaseifu.db` に作られ、CWD 非依存。`doctor` がパス表示。旧 `./data/kaseifu.db` の表示/非移行を実測。
3. pending 合成 DB で `doctor` / `undo` が件数・影響を報告し、自動変更しない（DB の不変 = 検査前後で行数と内容が同一、ファイル不変）。
4. `config show` / README が実装と一致し、既存 TOML（`operations.dry_run = true` 等）がエラーにならない。
5. `bash scripts/quality_check.sh` OVERALL PASS、全テスト緑、新規 CC16+ なし、新規依存なし。

## 6. リスクと対策

| リスク | 対策 |
|---|---|
| 既存テスト `v33_doctor_local_checks_without_network` の `checks.len()==7` が落ちる | doctor の検査追加は意図的変更。テストの意図（ネットワーク無しでローカル検査が走る）を保ったまま 9 へ更新 |
| `/var/folders`（macOS 一時領域）の除外が変わる | 先頭構成要素 `var`/`private` で従来どおり除外されることをテストで固定 |
| doctor がジャーナルを作る/壊す副作用 | 読み取り専用 open・存在チェックのみ。テストで「doctor 後にファイルが無い/不変」を固定 |
| 実ユーザーデータへの接触 | テストは隔離 HOME（`HOME=<tmp>`）と一時ディレクトリのみ。`~/Desktop`・実 app support を触らない |
| 複雑度ゲート | 検査・整形は小さなヘルパに分割（CC 16+ を作らない） |

## 7. 検収記録

| 工程 | 成果物 | 判定 |
|---|---|---|
| Plan Architect | `issues/20261015-plan-architect-journal-safety.md` | 指摘 F1（既存 diagnose テスト 9 箇所を注入経路へ）/ F5（development.md:132）を計画へ反映済み。他は承認 |
| Reviewer | `issues/20261015-reviewer-journal-safety.md` | 承認。指摘は既知事項のみ（setup verdict の warn 化、dry-run での open 継続、0 バイトジャーナルの FAIL は安全側） |
| QA | `issues/20261015-qa-journal-safety.md` | PASS（quality_check.sh OVERALL PASS / 367 passed / 新規 CC16+ 0 / バス・リンク整合） |

### 完了時の実測サマリ（2026-10-15）

- テスト: 既存 346 → **367 passed / 0 failed**（新規 19 + lib 単体 2）。
- `bash scripts/quality_check.sh`: rustfmt / clippy / test / complexity / baseline-gate / report-tests / coverage すべて PASS。
- ジャーナル実測: 隔離 HOME の 2 つの CWD から同一の `<HOME>/Library/Application Support/kaseifu/kaseifu.db` に作成。doctor 表示 OK。旧 `./data/kaseifu.db` は不変。
- pending 実測: 合成 DB（3h 前 pending）→ `FAIL Journal: ... 1 possibly interrupted` / `undo` warning、自動変更なし（実行前後で pending 行同一）。
- 設定実測: `config show` inactive 表示 + JSON `inactive_settings`、既存 TOML 受理、未知キー拒否維持。
