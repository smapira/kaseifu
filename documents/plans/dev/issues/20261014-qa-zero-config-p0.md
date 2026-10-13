# QA 検収: ゼロコンフィグ P0（ZC-1 / ZC-2 / ZC-3 仕上げ / ZC-4）

- 日付: 2026-10-14
- 対象: 計画 `documents/plans/dev/20261014e-zero-config-p0.md` / Reviewer `issues/20261014-reviewer-zero-config-p0.md`
- 判定: **PASS**

## 1. 品質ゲート（実測）

```
rustfmt                PASS   violations 0
clippy                 PASS   warnings 0
cargo-test             PASS   passed 385, failed 0
complexity             PASS   analyzer 0.0.25
baseline-gate          PASS   no new/worsened CC>=16
report-tests           PASS   complexity_report self-tests green
coverage               PASS   report-only (line 83.09%)

Functions analyzed 629 / CC<=10 99.4% / CC>=16 **0** / Max CC 15 / Avg 2.94
OVERALL: PASS
```

- 既存 367 → **385 passed / 0 failed**（新規 18: ZC-1 単体 8 + CLI/PTY 10）。
- `reports/quality/baseline.json` は**未変更**（意図的な複雑度変化なし。CC16+ の新規なし）。
- 新規依存・Cargo.toml/Cargo.lock の差分なし。

## 2. 受け入れ基準の実測

| # | 基準 | 実測 | 判定 |
|---|---|---|---|
| 1 | TTY で対話→保存→判定 | 実バイナリ + PTY（`libc::openpty`）: `Jev API key:` → `.env`（0600, `JEV_API_KEY=<key>`）→ `POLICY`/`LEAVE` パネル → exit 0。手実測（python pty）でも同じ | OK |
| 1' | `env` で Keychain 0 回 | 注入 `CountingStore`（読み書きを計数）で `(0, 0)` を固定。`keychain`/`auto` は `(0, 1)`（書きのみ）で `.env` 不変 | OK |
| 2 | 非TTY で非0・案内 | `classify` / `watch` = exit 1、`no API key is configured` / `kaseifu setup` / `kaseifu inspect` / `kaseifu doctor` を含む。`.env`・`data/` を作らない | OK |
| 3 | 鍵が出力に現れない | PTY 出力・手実測出力で `grep -c <key>` = **0**。doctor/setup/status の JSON にもキー値なし。エラー文にもなし（テスト） | OK |
| 4 | 引数なし exit 0・無副作用 | exit 0。`data/` 非生成、完了ファイル未更新、stderr 空。`--help`/`-h` は従来の全コマンド表示（`discover` は非表示のまま） | OK |
| 5 | FAIL 行の次アクション / 正常全 ok / env 即時完了 | 3 FAIL 例で全て `(next: ...)` 付き（下記）、正常環境は `Ready: yes` + 9 ok + exit 0、env 未設定 doctor は即時終了（ハングなし） | OK※ |
| 6 | 旧ジャーナル 0 操作案内・不変 | `0 operation(s) recorded; safe to delete` + `(next: rm ...)`。sha256/size/mode が前後同一、隣接ファイル生成なし | OK |
| 7 | 品質ゲート | 上記 §1 | OK |
| 8 | 振る舞いテスト追加 | `tests/v052_zero_config_key_test.rs`（8）/ `tests/v052_zero_config_cli_test.rs`（10） | OK |

※1: 旧ジャーナルに N>0 行ある場合のヒントのみ「実行するコマンドが存在しない」ため `nothing to run: keep ...` という行動文
（Reviewer 注意点 1 と同じ）。他の FAIL 行はコマンドを含む。

### doctor の FAIL 表示例（実測）

```text
Ready: no (3 check(s) failed: API Key, Legacy Journal, Completion)
ok Platform: macOS supported
ok Configuration: TOML loaded
FAIL API Key: not configured (KASEIFU_KEY_SOURCE=env); set JEV_API_KEY in the environment or in .env (next: kaseifu setup (or enter the key when a Jev command asks))
ok Jev Endpoint: URL form looks valid
ok Spotlight: mdls and mdfind available
ok Storage: data directory is writable
ok Journal: /.../kaseifu.db (no journal yet)
FAIL Legacy Journal: found data/kaseifu.db (old CWD-relative journal; 0 operation(s) recorded; safe to delete; not migrated, not deleted; current journal: /.../kaseifu.db) (next: rm data/kaseifu.db (safe: 0 operations; kaseifu never deletes it))
FAIL Completion: zsh completions missing (/.../completion); run kaseifu completions zsh --install (next: kaseifu completions zsh --install)
Jev connectivity: Not tested
```

## 3. 隔離・安全性の監査

- `grep -rn "KeychainStore::production" tests/` = 0 件（実 Keychain を使うテストなし）。v33 の Keychain 往復テストは予約サービス id（`.test`）のみ。
- 新規テストは隔離 HOME（子プロセス）+ `std::env::temp_dir()` の一時ディレクトリのみを使用。`~/Desktop`・実 `Application Support/kaseifu` への書き込みなし
  （参照は `home.join(...)` の存在チェックのみ）。
- PTY テストは `libc::openpty` + 期限付き kill。デッドロックしてもスイートを固めない。
- 旧ジャーナルのフィクスチャは一時ディレクトリ；検査前後のバイト比較をテストで固定。
- 手実測で一時的に作られた可能性のある `./.env`（gitignore 対象）は削除済みで、リポジトリの作業ツリーに痕跡なし（`git status` で確認）。

## 4. ドキュメント整合

- `README.md`: Quick Start（素の `kaseifu` / `doctor` / 対話鍵入力）、Doctor（`Ready:` 行と `(next: ...)`、旧ジャーナルの行数と削除可否）、API key source（対話取得と非TTY時の失敗）を更新。
- `documents/development.md`: 解決順の節と「Local Jev mocks and the key check」を更新（注入テストの場所を明記）。
- `CHANGELOG.md` `[Unreleased]`: Added 3 件（対話鍵取得 / 素の `kaseifu` / doctor の verdict と next）、Changed 1 件（旧ジャーナルの行数表示）を追記。バージョン・タグ・commit は未実施（親の担当）。
- ZC-12（ヘルプ全面見直し）は次バッチのため、ヘルプ文言は本バッチで変更していない（`--help` 互換のみ確認）。

## 5. 未対応・次バッチ

- ZC-5〜ZC-8（閾値近傍・`--explain`・`policy` 説明・`organize --format json`・動画の明示）、ZC-9/ZC-10（P2）、ZC-11（uninstall）、ZC-12（ヘルプ見直し）、ZC-13（対話選択）は**未実装（対象外）**。
- 既知の制限: `auto` モードの鍵取得（Keychain 書き込み）は実環境でしか実行できないため、自動テストは注入経路と共有コード（`SecretStore::set`）で担保。
