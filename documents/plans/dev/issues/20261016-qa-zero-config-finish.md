# QA 検証: ゼロコンフィグ仕上げバッチ（ZC-11 / ZC-9 / ZC-10）

- 日付: 2026-10-16
- 対象: `documents/plans/dev/20261016b-zero-config-finish.md` の実装（未コミット差分 + 新規 5 ソース/テスト + ドキュメント）
- 判定: **PASS**
- 実施コマンド: `bash scripts/quality_check.sh` / `cargo test --all-targets --all-features` / `cargo fmt --all -- --check` / `cargo clippy --all-targets --all-features -- -D warnings` / 実 CLI（隔離 HOME + 一時 `KASEIFU_COMPLETION_DIR` + `env` モード）/ ベースライン `d436451` バイナリとの比較

## 品質ゲート（最終実行、2026-10-16）

| 項目 | 結果 |
|---|---|
| rustfmt | PASS（violations 0） |
| clippy `-D warnings` | PASS（warnings 0） |
| cargo test | **PASS: 450 passed / 0 failed**（ベース 428 + 新規 22。減ったテストなし） |
| complexity | PASS（analyzer 0.0.25、725 関数） |
| baseline-gate | PASS（no new/worsened CC>=16。warning 6 件はすべて既存関数） |
| report-tests | PASS |
| coverage | PASS（report-only、line **85.26%**） |
| **OVERALL** | **PASS** |

- CC 分布: CC<=10 = 721 / 725（**99.4%**）、CC 11-15 = 4（既存のみ）、CC>=16 = **0**、最大 CC **15**（悪化なし）。
- 新規コードの CC: uninstall 系 37 関数（lib 17 + renderer 17 + main 3）で最大 **8**（`detail` / `kind_label` / `legacy_detail` / `keychain_detail`）。当初 CC 13 だった `file_detail` は `config_detail` / `journal_detail` に分割して Normal 帯に収めた。

## 新規テストの内訳

- `tests/v080_zero_config_uninstall_test.rs` = **17 passed**（library 9: plan 読み取りのみ・report 行・execute 補完のみ・purge の確認拒否/承認・env 非接触 spy・削除失敗の続行・冪等。CLI 8: ドライラン・execute・purge・env モードの keychain 不接触・非TTY 拒否・冪等・JSON・副作用なし・help）
- `tests/v080_zero_config_finish_test.rs` = **5 passed**（ZC-9 watch 文言 3・ZC-10 README 2）
- 既存テストの意図的更新: `v052_help_still_lists_every_command` に `uninstall` を追加、`tests/user-test-plan.csv` T55/T56 の文言更新 + uninstall 行 T85〜T91 追加

## パス・構造チェック

- 新規ファイル配置: `src/uninstall.rs`（純ロジック）、`src/presentation/renderers/uninstall.rs`（表示）、テストは既存慣行の `tests/v080_zero_config_{uninstall,finish}_test.rs`。版数接頭辞 `v080` で統一。
- ドキュメント: 計画書 1 + issues 3（Plan Architect / Reviewer / QA）。P0/P1 の計画書・issues・原典 `documents/plans/user/**` は無変更。
- `.gitignore`: テストフィクスチャは `std::env::temp_dir()` 配下の一時ディレクトリと `CARGO_TARGET_TMPDIR` のみ。リポジトリに生成物の混入なし（`reports/quality/` は既存どおり管理外）。
- 未追跡のまま残したもの（親がコミット対象を選ぶ）: 新規 4 ソース/テスト + 計画書/issue 3 + テスト 2。`git add` は未実施。
- デバッグ残骸: 新規コードに `TODO` / `FIXME` / `dbg!` なし。em dash なし（テストで ASCII を固定）。

## セキュリティ・隔離監査

- 秘密: uninstall のテストは実 Keychain を一切使用しない。削除経路は `MemoryStore` / spy ストアの注入のみで、`env` モードは get/delete 0 回を固定。CLI テストは `KASEIFU_KEY_SOURCE=env` 固定（非TTY 拒否テストのみ `keychain` だが、削除前に停止するため Keychain 非接触）。鍵の値は一切表示・保存されない。
- 実ユーザー領域: 全 CLI テストが隔離 HOME + 一時 CWD + 一時 `KASEIFU_COMPLETION_DIR`。実 `~/Library/Application Support/kaseifu/`・実補完ディレクトリ（Homebrew / XDG / `~/.zsh`）・`~/Desktop/`・実 `./data/` を読み書きするテストは無い。uninstall のテストは必ず `KASEIFU_COMPLETION_DIR` を上書きするため、実行環境に実 `_kaseifu` があっても対象にならない。
- ネットワーク: uninstall / watch 文言テストはネットワーク不要（uninstall は Jev を構築しない）。
- 互換: `discover` 非表示（help / zsh 補完 0 件）、`--format json` の既存キー削除なし（uninstall は新規コマンドで追加のみ）、`KASEIFU_KEY_SOURCE` の挙動不変（`src/config` 未変更）、ドライラン既定不変。

## 残存事項（非阻害）

- uninstall は削除失敗でも exit 0（Reviewer MINOR-1。JSON の `status: "failed"` で検知可能）。
- 非TTY の確認要求は鍵の有無に関わらず拒否し `--yes` を案内（MINOR-2）。
- 読めない旧ジャーナルは保持（MINOR-4）。
- `dispatch` CC 15（既存 warn-only、本バッチ由来ではない）。
