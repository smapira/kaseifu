# Plan Architect レビュー: Apple Foundation Models 統合（AI リネーム基盤）Phase 0-2

- 日付: 2026-10-14
- 対象計画書: `documents/plans/dev/20261014f-ai-rename-p0.md`
- 原典: `documents/plans/user/20261014a.md`（§3 FFI / §4 FM / §12 エラー / §14 Phase / §15 受け入れ基準）
- 判定: **承認（F1〜F8 を計画に反映済み）**
- レビュー方法: 実コードの直接読解（`src/lib.rs` / `src/main.rs` / `src/classifier/{jev,mock_jev,pipeline}.rs` / `src/models/file_context.rs` / `src/context/builder.rs` / `src/normalize/mod.rs` / `src/storage/history.rs` / `src/operations/undo.rs` / `src/doctor.rs` / `src/config/storage.rs` / `Cargo.toml` / `scripts/quality_check.sh` / `.github/workflows/rust-quality.yml` / `tests/v070_zero_config_p1_help_test.rs` / `tests/v080_zero_config_finish_test.rs`）、SDK 一次情報（`FoundationModels.swiftinterface`）の読解、実機スパイク（Swift -> アーカイブ -> Rust リンク、weak link、`@Generable` のコンパイル）

## 検証項目のチェック

| 項目 | 結果 |
|---|---|
| コードベースの規約・パターンとの整合 | OK（Provider は `trait JevIntegration`（`src/classifier/jev.rs:84-90`）+ mock（`src/classifier/mock_jev.rs`）の前例を踏襲。`emit_report` / CLI には触れない） |
| 既存ユーティリティの再利用 | OK（`FileContext` / `NormalizedFileContext` / `normalize` / `is_image_file` / Journal 設計を調査済み。今回の実装は新しい FFI 層と Provider だけ） |
| アーキテクチャの妥当性（レイヤー分離・依存方向） | OK（`contract` <- `ffi` <- `bridge` <- `provider` の一方向。Swift 側にファイル操作 / SQLite / Journal を置かない = 原典 §2 の責務分離） |
| 編集範囲の遵守 | OK（`src/main.rs` の `enum Commands` 不変。CLI 面の変更ゼロ。`documents/plans/user/**` は読み取りのみ） |
| テスト計画 | OK（モデル非依存。fake 注入 + JSON 契約 + 実機スモーク + スタブ経路 + leaks） |
| セキュリティ | OK（秘密なし。ファイルシステム非接触テスト。プロンプト注入への言及あり） |
| パフォーマンス | OK（コンテキスト / セッションの再利用は Phase 9 に明示的に繰り延べ） |

## 指摘事項と対応

### F1 🟡 公開 API の骨子が計画に無い（実装者の判断余地が大きい）

**指摘**: trait / 型 / 関数名の骨子が無く、実装者ごとに命名が振れる余地があった（特に 2 層の trait を置くかどうか）。
**対応**: 計画に **D7（公開 API の骨子）** を追加。C ABI 名は原典 §3.2 の仮称を確定（`kaseifu_fm_*` は既存の `kaseifu` / `KASEIFU_*` と整合）、trait は `JevIntegration` と同型、`FmBridge`（FFI 面）と `SemanticProvider`（ドメイン面）の 2 層とする。

### F2 🟡 スタブ経路とブリッジ経路の**両方**が品質ゲートの対象になる

**指摘**: CI（Xcode < 26）はスタブ、ローカルはブリッジでコンパイルされる。片方でしか通らないコードはゲートを落とす。計画の完了条件がブリッジ経路前提に見えた。
**対応**: タスク一覧の後に「両ビルド経路で `clippy -D warnings` 警告 0」を明記し、検証手段 V10 を追加。

### F3 🟡 `examples/` は `cargo test --all-targets` のビルド対象

**指摘**: スモークを `examples/` に置く場合、`cargo test --all-targets` / `clippy --all-targets` がビルド対象に含める。警告や未使用 import があるとゲートが落ちる。
**対応**: F2 の完了条件に `examples/` も含めると明記。

### F4 🟢 `unexpected_cfgs` の回避

**指摘**: build.rs が独自 cfg を立てるため、`cargo:rustc-check-cfg` を宣言しないと clippy が `unexpected_cfgs` を出す可能性がある。
**対応**: タスク T1/T2 の実装条件に `cargo:rustc-check-cfg=cfg(kaseifu_fm_stub, values("no-toolchain", "no-sdk"))` を明記。

### F5 🟢 README 追記の位置と既存契約

**指摘**: README には既存テスト契約（Quick Start が冒頭・見出し・アンカー解決、`tests/v080_zero_config_finish_test.rs:77-140`）がある。
**対応**: 追記は `## Project structure`（`semantic/` / `swift/` / スキーマ 1 行）と `## Code quality` 周辺の開発者向け記述に限定し、見出しの改名・移動をしない（計画 §D6 に反映済み。実装時にテストで確認）。

### F6 🟢 新規ファイル配置の妥当性

**指摘**: `swift/` はリポジトリ初の非 Rust ソース置き場。`build.rs` は root 慣例。
**対応**: `swift/kaseifu_fm_bridge.swift`（単一ファイル・C ABI のみ）、`build.rs`（root）を採用。SPM / Xcode プロジェクトは導入しない（D1 の代替案として記録済み）。**妥当と判断**。

### F7 🟢 テスト隔離の明示

**指摘**: 新規テストが実ユーザー領域に触れないことを計画で明示すべき。
**対応**: §2.1 の隔離テスト行と B5-3 に「ファイルシステム非接触（パスを一切開かない）」を明記済み。`leaks` / スモークもパスを使わない。

### F8 🟢 スコープ境界の固定

**指摘**: Phase 3 以降（rename / 命名ルール / Undo）に手を出さないことを完了条件に固定すべき。
**対応**: 計画冒頭の「対象外」と §8 リスク表に明記済み（`enum Commands` に触れないことを完了条件とする）。

## 再利用性の提案（採用）

- `is_image_file`（`src/context/image_metadata.rs`）を `target_type` の判定に再利用する（新規述語を作らない）。
- `NormalizedFileContext` の `semantic_signals` / `image_metadata` をそのまま入力に使う（別の収集経路を作らない）。
- エラー分類は既存 `JevError` の粒度（Transport / InvalidResponse / Configuration 相当）に合わせ、`Display` を実装する（既存 `JevError` と同じ流儀）。

## 備考

- 原典 §14 Phase 1 の完了条件「Foundation Models の推論成功」は本環境で外部ブロッカー（`appleIntelligenceNotEnabled`）。計画 §2.4 で読み替えを明示済みであり、**計画の妥当性には影響しない**（暗黙フォールバックを実装しないことが原典の要求）。
- MCP（serena / context7 / codebase-memory-mcp / graphify）は本環境で未接続のため、直接読解と SDK 一次情報で代替した。
