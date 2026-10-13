# KASEIFU_KEY_SOURCE 実装の Reviewer 検収

- 日付: 2026-10-13
- 対象: `src/config/key_source.rs`（新規）, `src/config/resolver.rs`, `src/config/mod.rs`,
  `src/doctor.rs`, `src/main.rs`, `tests/key_source_test.rs`（新規）, `tests/key_source_cli_test.rs`（新規）,
  `tests/v33_config_auth_test.rs`, `tests/v32_image_metadata_test.rs`, `tests/completion_test.rs`
- 検証方法: 実コード読解、`grep` による全 store 呼び出し元の列挙、実バイナリ実行（debug/release）、
  テスト実行。仕様書は親の受領要件（AC1〜AC7）を正とした

## 検証結果

### 受入条件との対応（トレーサビリティ）

| AC | 実装箇所 | 検証 |
|---|---|---|
| 1 ハングなし・明確な未設定 | `require_jev_api_key`（main.rs）, `missing_key_detail`（doctor.rs） | 実バイナリ: env モード・キー無しで classify 0.013s / organize 0.029s / watch 0.010s、doctor 0.014s、auth status 0.00s。全て exit 0/1 で終了（ハングなし） |
| 2 既定の固定 | `KeySourceMode::default_for_build` / `build_default` | `key_source_debug_build_defaults_to_env_and_release_to_auto` テスト + 実バイナリ表示（debug=`env (default)` 実測、release=`auto (default)` 実測） |
| 3 env モード 0 アクセス | `pick_api_key`（env 早期分岐）, ゲート順序, `diagnose` の store 除去 | CountingStore テスト（get/set/delete 個別）で resolve+doctor の 0 を証明。store 呼び出し 5 箇所を grep で全列挙し、env モードでは全て到達不能なことを確認 |
| 4 keychain モードの逆分離 | `pick_api_key`（Keychain 早期分岐） | `keychain_mode_prefers_the_store_over_environment_and_dotenv`（get 1 回、env/.env 無視） |
| 5 表示と即時性 | `check_api_key`, `key_state` / `key_storage` / `key_source_line` | doctor / auth status の実測（Source 行・API Key 行）。env モード < 1 秒 |
| 6 品質 | — | `bash scripts/quality_check.sh` OVERALL PASS、346 passed（既存 314 + 新規 32）、新規 CC ≥ 16 なし、依存追加なし |
| 7 秘密の非混入 | — | `.env` は未読・未作成（gitignore 確認）、テストのキーは明白なダミーのみ、出力にキー値なしを CLI テストで assertion |

### 敵対的レビューで発見・修正した問題（Blocking 1 件）

#### R1. 🔴（修正済み）Auto モードで空の `JEV_API_KEY` が Keychain にフォールバックしていた

- **問題**: 旧 `pick_api_key` は env/.env に**値が存在して空**の場合、Keychain を読まずに「キー無し」を返していた。
  最初の実装は「空 = 無し」として Auto のフォールバックに合流しており、`.env.example` をコピーした
  だけで未設定の `.env`（`JEV_API_KEY=`）を持つリリースビルドのチェックアウトで、Keychain 読みが
  発生する回帰だった（本機能が減らそうとしているダイアログを誘発する方向）。
- **証拠**: 修正前の分岐 `Auto => match pick_env_key(lookup) { (None,_) => pick_keychain_key(store), … }`。
- **修正**: `env_key()` を導入し、「値が存在して空」は全モードで `Missing` のままフォールバックしない。
- **Regression Test**: `auto_mode_blank_env_key_never_falls_back_to_the_keychain`（store アクセス 0 を assertion）。
  実バイナリでも `.env` に空値 + `KASEIFU_KEY_SOURCE=auto` で 0.015 秒・`Source: not configured`（Keychain 項目が
  存在する実機で無ダイアログ）。

### 非 Blocking の指摘（仕様どおり / 文書化済み）

- **R2. 🟡 doctor のチェック名変更** — `Keychain` → `API Key`。env モードでは Keychain を読まないため
  「Keychain」名が事実と矛盾するための変更。`--format json` の構造・キーは不変（値のみ変更）。
  CHANGELOG の Added/Changed に明記。
- **R3. 🟡 auto のキー欠落は従来どおり試行** — 受領要件「auto＝現行挙動を維持」に従い、プリフライトを
  入れない（env/keychain のみ即時エラー）。`require_jev_api_key` の分岐で表現し、レビューで確認済み。
- **R4. 🟡 env モードの `auth set-key`/`remove-key` は exit 0** — 「書かない」を最優先し、失敗ではなく
  モード方針の案内として JSON `reason` で理由を返す設計（スクリプトから判定可能）。
- **R5. 🟢 doctor の Keychain 再読み廃止** — 診断が 2 回目の Keychain 読みをしない（resolver の 1 回の結果を共有）。
  リリースの auto モードでもダイアログ発生確率が下がる改善であり、旧表示（Unknown + エラー文言）は
  `Unavailable` の保持で維持されることをコードとテストで確認。

### セキュリティ・秘密情報

- キー値は `ApiKey`（Debug レッドアクト）と `LocatedValue`（Debug 未実装）の内側に閉じており、
  表示・JSON・ログ・Debug 出力のどこにも出ない（既存 `v33_api_key_debug_output_is_redacted` + 新規 CLI assertion）。
- `.env` は読解・表示・作成していない（`git status` に変化なし、gitignore 確認済み）。

## 判定

**APPROVE**（R1 修正済み・品質ゲート再実行済み）。
