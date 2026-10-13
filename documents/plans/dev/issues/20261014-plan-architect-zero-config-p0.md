# Plan Architect レビュー: ゼロコンフィグ P0 実装計画

- 日付: 2026-10-14
- 対象: `documents/plans/dev/20261014e-zero-config-p0.md`
- 関連コード: `src/main.rs`, `src/doctor.rs`, `src/auth/{mod,prompt,keychain}.rs`,
  `src/config/{resolver,key_source,loader}.rs`, `src/storage/history.rs`, `tests/v051_journal_path_test.rs`
- 確認手段: 直接読解 + `grep`（serena / codebase-memory-mcp / graphify は本環境で未接続）

## 検証したこと

- `Cli.command` は非 optional（`src/main.rs:33-44`）。`--help` の Commands 一覧は clap が生成するため、optional 化しても一覧は変わらない（`tests/completion_test.rs:237-270`
  は Commands セクションのみを読む）。
- 対話入力は `auth::prompt::prompt_secret`（TTY 必須・エコー無効・値はログに出ない）が既存。**再利用が正**。
- API キーの保存経路は `SecretStore::set`（Keychain）が既存。`.env` 書き込みは存在しない（読取は `dotenvy`）。
- `KeySourceMode::keychain_allowed()`（`src/config/key_source.rs:65-67`）が「env だけ Keychain 非接触」の権威。分岐はこれを再利用すべき。
- 既存テストで ZC-1/ZC-2 に影響するものは `tests/key_source_cli_test.rs:251-264`（`no API key is configured` / `KASEIFU_KEY_SOURCE=env` を固定）
  と `tests/v051_journal_path_test.rs:189-191`（旧 `Legacy Journal` 文言を固定）の 2 箇所のみ。前者は文言維持で対応可能、後者は ZC-3 の意図的表示変更に伴い更新。
- `doctor` の検査数 9 は `tests/v33_config_auth_test.rs:520` で固定。検査追加はしない方針なので更新不要。

## 指摘事項（反映済み）

### 🔴 F1. `.env` 書き込みの往復整合（引用・エスケープ）

`dotenvy` は引用・コメント・空白を解釈するため、生値を無条件に書くと「保存したのに読めない」状態になり得る。

**改善案**: 値を検証（非空・改行/NUL 禁止）した上で、保守的な安全文字種は生値、それ以外は二重引用符 + エスケープで書き、
`dotenvy` で読み戻して一致することをテストで固定する。`.env` の他行は保持、ファイルは 0600。

### 🟡 F2. 保存後の再解決でモード保証を壊さない

保存→再解決の実装で、`env` モードにもかかわらず Keychain を読む経路を混ぜないこと。

**改善案**: 再解決は既存 `resolve_for`（resolver が `keychain_allowed()` を尊重）をそのまま使う。テストでは「呼び出し回数を数えるストア」を注入し、
`env` モードで読み書き 0 回を固定する。

### 🟡 F3. PTY テストの環境依存を閉じる

対話経路の自動テストは外部ツール（`script` / `expect` / python）に依存させない。

**改善案**: 既存依存の `libc::openpty` で自己完結の PTY を作り、`#[cfg(target_os = "macos")]` でガード。入力は master へ事前に書けるためプロンプト検出に頼らず、
必ずデッドラインで kill する。判定表示は候補 0 件でも `NONE`/`LEAVE` に到達する（`src/classifier/pipeline.rs:225-230`）ことを利用し、ネットワーク不要で固定する。

### 🟢 F4. `doctor` の verdict 行と JSON 追加キーの互換

**改善案**: テキストは行追加のみ、JSON は `checks[].next` / `ready` / `failing` の追加のみ。既存キー・既存 detail 語は不変。

### 🟢 F5. 新規ファイルの tracked 化

**改善案**: `src/auth/acquire.rs` と新規テスト 2 ファイルを `git add` する（親のコミット対象に含める）。

## 備考

- F1〜F3 は計画 §3.1 / §6 に反映済み。F4・F5 もタスク T4/T7 に含まれる。
- 上記反映をもって計画を**承認**し、Implementer へ実装を依頼する。
