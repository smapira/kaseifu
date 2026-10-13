# QA 検証: ゼロコンフィグ P1（ZC-5 / ZC-6 / ZC-7 / ZC-8 / ZC-12 / ZC-13）

- 日付: 2026-10-16
- 対象: `documents/plans/dev/20261016a-zero-config-p1.md` の実装（未コミット差分 + 新規 6 ソース/テスト + ドキュメント）
- 判定: **PASS**
- 実施コマンド: `bash scripts/quality_check.sh` / `cargo test --all-targets --all-features` / `cargo fmt --all -- --check` / `cargo clippy --all-targets --all-features -- -D warnings` / 実 CLI（モック Jev + 偽 mdfind + 実 PTY + 隔離 HOME）

## 品質ゲート（最終実行、2026-10-16）

| 項目 | 結果 |
|---|---|
| rustfmt | PASS（violations 0） |
| clippy `-D warnings` | PASS（warnings 0） |
| cargo test | **PASS: 428 passed / 0 failed**（ベース 387 + 新規 41。減ったテストなし） |
| complexity | PASS（analyzer 0.0.25、687 関数） |
| baseline-gate | PASS（no new/worsened CC>=16） |
| report-tests | PASS |
| coverage | PASS（report-only、line **85.05%**） |
| **OVERALL** | **PASS** |

- CC 分布: CC<=10 = 683 / 687（**99.4%**、ベースライン同値）、CC 11-15 = 4、CC>=16 = **0**、最大 CC **15**（悪化なし）。
- 新規関数で 11-15 帯は `dispatch`（14、warn-only）のみ。`main` は 16 → 2 に低下。

## パス・構造チェック

- 新規ファイル配置: `src/operations/select.rs`（純ロジック）、`src/presentation/renderers/{explain,select}.rs`（表示）、テストは既存慣行の `tests/v070_zero_config_p1_*.rs` 4 ファイル。命名規約（版数接頭辞 `v070`）に一致。
- ドキュメント: 計画書 1 + issues 3（Plan Architect / Reviewer / QA）。P0 の計画書・issues・原典 `documents/plans/user/**` は無変更。
- `.gitignore`: テストフィクスチャは `target/tmp`（`/target` で ignore 済み）と隔離 HOME（`/var/folders/...`）のみ。リポジトリに生成物の混入なし。
- 未追跡のまま残したもの（親がコミット対象を選ぶ）: 新規 3 ソース + 4 テスト + 3 ドキュメント。`git add` は未実施。
- デバッグ残骸: 新規コードに `TODO` / `FIXME` / `dbg!` なし。

## セキュリティ・隔離監査

- 秘密: 鍵はテスト用ダミーのみ。鍵の値が stdout / stderr / JSON / テスト名に現れないことをテストで固定。`.env` の中身を読む・表示するテストは無い。実 Keychain は全テストで未使用（`KASEIFU_KEY_SOURCE=env` 固定、`env` モードは Keychain 非接触）。
- 実ユーザー領域: 全子プロセスが隔離 HOME + 一時 CWD。`~/Library/Application Support/kaseifu/`・`~/Desktop/`・旧 `./data/` を読み書きするテストは無い。`KASEIFU_NO_COMPLETION_UPDATE=1` で完了スクリプトの自動更新も遮断。
- ネットワーク: ZC-5/7 のテストは 127.0.0.1 のローカル HTTP モックのみ。ZC-13 は閉ポートを指し、Jev を構築しないことを実測（鍵プロンプト 0 回、完走）。
- 互換: `discover` 非表示（help / zsh 補完 0 件）、`--format json` の既存キー削除なし（追加のみ）、`KASEIFU_KEY_SOURCE` の挙動不変（`src/config` 未変更）、ドライラン既定不変。

## 残存事項（非阻害）

- `dispatch` CC 14（warn-only）: サブコマンド追加時は分割が必要。
- `--explain` 時の JSON キー順がアルファベット順（Reviewer MINOR-1）。
- ヘルプの閾値は既定値表示（実効値は `config show`）。
