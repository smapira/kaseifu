# KASEIFU_KEY_SOURCE 計画の Plan Architect レビュー

- 日付: 2026-10-13
- 対象: `documents/plans/dev/20261013e-key-source-plan.md`
- 関連: `src/config/resolver.rs`, `src/config/mod.rs`, `src/doctor.rs`, `src/main.rs`,
  `tests/v33_config_auth_test.rs`, `tests/v32_image_metadata_test.rs`, `tests/completion_test.rs`
- 検証方法: 実コード読解と `grep`（`serena`/`context7`/`codebase-memory-mcp` は本環境で利用不可）。
  全呼び出し元の列挙を実施（`resolve_with`/`resolve_with_env`/`diagnose`/`store.get|set|delete`/`ResolvedConfig {` 構築箇所）

## 総評

方針・分解粒度は妥当で、既存アーキテクチャ（注入可能な純 resolver、`SecretStore`、`emit_report`）に整合する。
以下の 7 件を反映すれば実装着手を承認する。

## 指摘事項

### A1. 🔴 呼び出し元の総数確認と型推論

- **指摘**: `resolve_with` のシグネチャ変更（lookup の戻り値 `LocatedValue` 化 + mode 引数）は、
  既存テスト 2 ファイルのクロージャに影響する。`tests/v32_image_metadata_test.rs:687` の `|_: &str| None` は
  期待型からの推論に依存する。
- **改善案**: 全呼び出し元は v32: 2 箇所（`resolve_defaults()` と 1 直接）、v33: 1 ヘルパ + `resolve_with_env` 3 箇所。
  `None` の型推論が失敗する場合は `Option::<LocatedValue>::None` と明示する。`diagnose` は main 2 + v33 3 のみ。
  `ResolvedConfig` の直接構築は resolver 内のみなのでフィールド追加は安全（確認済み）。

### A2. 🔴 `env_or_dotenv` の他用途追従

- **指摘**: `pick_endpoint`/`pick_model` も同じ lookup を使うため、`LocatedValue` 化に伴う値の取り出しが漏れると
  endpoint/model 解決が壊れる（既存テストで検出されるが、実装順で先に直すべき）。
- **改善案**: T2 で `LocatedValue` 導入と同時に `pick_endpoint`/`pick_model` を `.value` 取り出しへ機械的に更新し、
  既存の endpoint/model 優先順位テスト（v33 T2/T3）を先に緑化してから pick_api_key を変更する。

### A3. 🔴 main.rs の store ゲート順序の固定

- **指摘**: 「env モードで 0 回」は、main.rs の write 系コマンドで**モード判定が store 呼び出しより前**に
  あることが必須。特に `handle_auth_set_key` は現行で先頭 `store.get()`、`handle_setup` は先頭 `store.get()`。
- **改善案**: 実装順を「(1) mode 取得 → (2) `mode == Env` なら即メッセージ return → (3) store 生成/呼び出し」と
  規約化し、`handle_setup` は `keychain_has_key` の取得を `KeySourceSetting` 判定後の分岐内に移す。
  アクセス点の全列挙（resolver 1 / setup 2 / auth set-key 2 / auth remove-key 2 / key_status 0 へ削減）を
  T4 の受け入れに含める。

### A4. 🟡 setup の env モード仕様の明文化

- **指摘**: 計画 §4 の行だけでは setup の振る舞い（TOML 保存と key プロンプトの扱い）が実装者に伝わらない。
- **改善案**: T4 に「endpoint/model のプロンプトと TOML 保存は継続、キープロンプトと Keychain 書き込みをスキップし、
  案内メッセージを intro と summary に出す（summary の『saved to macOS Keychain』を env モードでは出さない）」を明記。

### A5. 🟡 既存テストの環境依存を減らす副次修正

- **指摘**: `tests/completion_test.rs` の `child()` は親環境を継承する。開発者が `KASEIFU_KEY_SOURCE=keychain`
  を設定した状態で `cargo test` すると、既存テストの子プロセスが実 Keychain を読む恐れがある（新機能が持ち込むハザード）。
- **改善案**: `child()` に `env_remove("KASEIFU_KEY_SOURCE")` を追加する（1 行。既存 314 の意図は不変）。

### A6. 🟡 受け入れ実測の網羅

- **指摘**: AC1 の「実際に Jev を呼ぶコマンドの各経路」は classify だけでなく organize / watch も含む。
- **改善案**: CLI テストは classify を自動化し、organize / watch は手動実測（一時 cwd・env モード・exit 1・即時）を
  QA 証跡に含める。watch は fsevents 起動前に失敗することを確認する。

### A7. 🟢 JSON 互換の値マッピングを表で固定

- **指摘**: `storage` の値を「変えない」とだけ書くと、由来ベースの実装で値が変わる余地がある。
- **改善案**: マッピングを計画に固定: `Environment|Dotenv → "Environment"`, `Keychain → "macOS Keychain"`,
  `Missing → "-"`, `Unavailable(msg) → msg`（現行と同一の値を再現）。

## 承認条件

A1〜A7 を計画書へ反映した時点で実装着手を可とする（再レビュー不要）。実装時は各指摘の反映を
reviewer/QA の検証項目に含めること。
