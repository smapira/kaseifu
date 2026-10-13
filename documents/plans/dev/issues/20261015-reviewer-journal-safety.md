# Reviewer 検収: ジャーナル安全性と設定の正直さ（20261015b）

## 判定

**承認（ブロッカーなし）**。指摘はすべて情報提供/既知事項で、実装の修正要求は無し。

## 対象

- 実装: `src/discovery/{similarity,destinations}.rs`, `src/storage/history.rs`, `src/doctor.rs`,
  `src/main.rs`, `src/watch/coordinator.rs`, `src/config/{defaults,loader}.rs`, `config/default.toml`
- テスト: `tests/v051_*`（4 ファイル新規）, `tests/{v33_config_auth,key_source}_test.rs`（既存更新）
- 文書: `README.md`, `CHANGELOG.md`, `documents/development.md`, 計画書 `20261015b-...md`

## 検証した項目（実測）

| 観点 | 結果 |
|---|---|
| 受け入れ基準 1（R8） | 新規テスト 3 件 + lib 単体 2 件が緑。旧 `contains` 相当の判定が `~/var` 等を除外し、新判定が除外しないことを python 再現で before/after 実測 |
| 受け入れ基準 2（R5） | 隔離 HOME で実バイナリ 2 回（CWD 変更）→ 同一絶対パスにジャーナル作成を実測。doctor が `ok Journal: <path> (no journal yet)` を表示し、ファイルを作らない（テスト + 実測） |
| 受け入れ基準 3（R1/R2） | 合成 DB（3h 前の pending 行）で doctor が `FAIL Journal: ... 1 pending, 1 possibly interrupted; ... source absent, destination present` を表示。undo が `warning: 1 pending journal row (1 move, 0 undo)` を出しつつ、通常の success move だけを復元。pending 行は実行後も pending のまま（実測 + テスト） |
| 受け入れ基準 4（F-05/F-12） | `config show` の inactive 表示と JSON `inactive_settings` を CLI で実測。既存 TOML（dry_run=true 等）でエラーなし。`config set storage.database` は従来どおり未知キー拒否 |
| 受け入れ基準 5 | `bash scripts/quality_check.sh` = rustfmt/clippy/test/complexity/baseline-gate/report-tests/coverage すべて PASS、OVERALL PASS。367 passed / 0 failed |
| 実ユーザーデータ不接触 | 変更したテストは `diagnose_with` に一時パスを注入（実ジャーナルを開かない）。新規 CLI テストは隔離 HOME + 一時 CWD。旧 `./data/` は作成したフィクスチャのみ操作 |
| JSON 互換 | `config show --format json` は既存キー維持 + `inactive_settings` 追加のみ。`doctor --format json` は `checks[]` への追加のみ |
| 秘密・パス漏れ | doctor 詳細・警告・テスト出力に実ユーザーパスの混入なし（grep）。secret は従来どおり非表示 |
| 無干渉 | `discover` の `#[command(hide = true)]` 維持。`KASEIFU_KEY_SOURCE` / `src/auth` / `Cargo.toml` / `Cargo.lock` / `.gitignore` は diff 0 行 |
| 新規依存 | なし（テストは既存 [dependencies] の chrono / rusqlite / toml / serde_json のみ使用） |

## 指摘・既知事項（修正不要）

1. 🟢 `setup` の verdict（`report.checks.iter().all(|check| check.ok)`）は新チェックの影響を受ける。CWD に旧 `./data/kaseifu.db` がある、または stale pending がある場合は「complete with warnings (run doctor)」になる。
   — 意図した warn 挙動（隠さない）。README §Doctor に「run doctor」導線あり。
2. 🟢 `organize` の dry-run でもジャーナルを open（作成）する既存挙動は不変で、作成先が絶対パスに移動しただけ。
   — 本タスクの範囲外（既存挙動の移設）。安全側（データを書くのは open のみで、行は execute 時のみ）。
3. 🟡 0 バイトのジャーナルファイル（open 直後にクラッシュした場合）は `Journal` チェックで「not an initialized kaseifu journal」となり FAIL する。
   — メッセージは対処可能で、次の `OperationHistory::open` がスキーマを初期化する。誤警報の可能性は極小で、検出漏れより安全側。
4. 🟢 旧設計文書（`20261014d-*`）は「実測日 2026-10-13 の記録」として更新しない（歴史的記録）。今回の変更は `20261015b` 計画書 + CHANGELOG が正。

## 実装の確認（コードリード）

- `history.rs`: 検査は `SQLITE_OPEN_READ_ONLY` + 存在チェックのみ。`CREATE TABLE` を実行しない。stale 判定は `PENDING_STALE_AFTER_SECS`、timestamp 不正は stale 側（コメントあり）。
- `doctor.rs`: `diagnose_with` の注入境界でテストが実ジャーナルを読まない。detail は ASCII のみ、最大 3 行分の状態 + N more。
- `main.rs`: undo 警告は stdout（既存出力と同じ）、pending の選抜条件（`result='success'`）は不変。警告は SELECT のみ。
- `destinations.rs`: 規則は絶対パスの先頭構成要素、大小無視。`similarity.rs` からの二重判定を撤去し権威コメントを追加。
- テスト削除・スキップ無し（既存 2 箇所の更新は意図的な仕様変更に対するもの）。

## 備考

品質ゲートの baseline 比較 WARN（apply_setting CC 15 等）は既存関数で、baseline.json の更新は不要と判断（新規 CC16+ 0、ゲート PASS）。
