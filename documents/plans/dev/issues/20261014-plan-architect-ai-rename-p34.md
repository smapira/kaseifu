# plan-architect review: AI rename P34 plan

# plan-architect レビュー: AIリネーム P34 計画書（`20261014i-ai-rename-p34.md`）

レビュー方式: 本環境に `/plan-architect` スキルが無いため手順をなぞって実施（コードベース照合・再利用確認・制約検査・差し戻し判定）。照合は `read_file` / `grep` / `terminal` の実測のみに基づく。

## 判定: 条件付き承認（下記 A1〜A4 を計画書へ反映すること。反映後の再レビューは不要）

## 確認済み項目

| 観点 | 結果 |
|---|---|
| 既存ユーティリティ再利用 | OK。`SemanticProvider` / `FakeFmBridge` / `NormalizedFileContext` / `OperationHistory` / `undo_last_move` / `emit_report` / clap_complete 生成補完の再利用は実コードと一致 |
| journal `move` 再利用 | OK。`last_successful_move`（`src/storage/history.rs:166-181`）は `operation='move'` 前提。undo（`src/operations/undo.rs:38-46`）は source/destination のみ参照。doctor（`src/doctor.rs`）は行数・pending のみ。いずれも無変更で整合する |
| CLI 形状 | OK。`rename patterns` 位置引数形は clap derive と衝突する（未知サブコマンドは位置引数へ落ちない）ため `--patterns` フラグ形への読み替えは正当。`organize` 流儀（`--execute` / `--dry-run` conflict）踏襲も正当 |
| 新規依存・新設定ファイルなし | OK。`Cargo.toml:23-42` に追加なしで実装可能。pdf 復号・NFC 用 crate を使わない方針と整合 |
| テスト隔離 | OK。`dirs::data_dir()` は `$HOME` 準拠（`src/config/storage.rs` 実測）のため、バイナリ経由の結合テストは一時 HOME（`v070` テストと同方式）で隔離できる |
| 複雑度 | 条件付き。validate・resolve 系は小関数に分割し CC<16 を維持すること（実装時計測） |

## 指摘（計画書へ反映すること）

- A1 `--pattern` の値は ID（`P01`〜`P12`、大文字小文字不問）と slug（原典 §8.2 の `ai` / `date-title` / `prefix-title` 等）の両方を受け付けること。原典例 `rename document.pdf --pattern ai` が ID のみ受けでは動かない。slug 対応表: P01 `ai`, P02 `date-title`, P03 `title-date`, P04 `prefix-title`, P05 `title-suffix`, P06 `category-title`, P07 `date-category-title`, P08 `seq-title`, P09 `date-seq-title`, P10 `project-title`, P11 `meta`, P12 `custom`（`--template` 必須）。`--patterns` 一覧に両方を表示する。
- A2 T3 の実装方式を修正: `history.rs::from_connection` に手を入れず、新 `src/rename/store.rs` の `RenameStore::open(path)` が同一 DB に対して `CREATE TABLE IF NOT EXISTS rename_presets / rename_settings` を実行する（`history.rs:86-104` と同一パターン）。`operations` 表への接触ゼロ・既存行への影響ゼロを差分で保証する。CLI は journal と同一パス（`history::journal_path()`）を開く。
- A3 Phase 4 の入力パイプラインを明示: `context::builder::build`（`src/context/builder.rs:10`）→ `normalize::normalize` → `semantic_signals` 末尾に文書 `T:` 信号（最大 12 件）を追加 → `SemanticProvider::analyze`。自前の `NormalizedFileContext` 手組みはしない（observe/normalize の再利用が原典の既存優先方針に合致）。symlink 判定は builder が canonicalize で吸収する前にサービス側で `symlink_metadata` により先行実施する。
- A4 `--format json` の新規オブジェクト形状を固定: `{"rename": {"target", "current_name", "proposal"|null, "status": "proposed|executed|noop|held|error", "reason"|null, "pattern", "date_source", "operation_id"|null}}`。既存キーの追加・削除なし。
- A5（軽微）D5 の html タグ空白化は `<>` の単純走査とし、レンダリング・外部パーサは使わないことを明記する（実装済み方針の確認）。

## 差し戻さない理由

- 上記は追加・明確化のみで、D1/D2/D4/D5/D7 の結論（`move` 再利用・フラット CLI・SQLite・bound reader・厳格検証）を覆すものではない。
- Phase 5 以降への波及なし（`rename` バリアント追加は `--help` 抽出テストに自動吸収される）。
