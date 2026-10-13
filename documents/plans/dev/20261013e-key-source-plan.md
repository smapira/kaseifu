# APIキー取得経路の明示分離（KASEIFU_KEY_SOURCE）実装計画

- 起票: 2026-10-13
- 対象: `src/config/key_source.rs`（新規）, `src/config/resolver.rs`, `src/config/mod.rs`,
  `src/doctor.rs`, `src/main.rs`, `tests/key_source_test.rs`（新規）,
  `tests/key_source_cli_test.rs`（新規）, `tests/v33_config_auth_test.rs`, `tests/v32_image_metadata_test.rs`,
  `README.md`, `documents/development.md`, `CHANGELOG.md`, `.env.example`
- 制約: リリース既定=auto（Keychain 利用）を変えない / 新規依存なし / JSON互換（既存キー維持・追加のみ）/
  ASCII・NO_COLOR 方針維持 / commit・tag・push は親が実施 / `.env` の値は読まない・表示しない・コミットしない
- 確認方法: `read_file`/`grep` で実コードを読解（`serena`/`context7` 等の MCP は本環境で利用不可）。
  確認済み: `src/config/resolver.rs:228-245`（pick_api_key）, `src/doctor.rs:101-110`（check_keychain）,
  `src/main.rs:578-581`（resolve_for）, `:631-649`（setup）, `:810-912`（auth）, `:993-1004`（doctor）,
  `tests/v33_config_auth_test.rs:473-521`（doctor テスト）, `tests/completion_test.rs:31-45`（実バイナリ起動）,
  `scripts/quality_check.sh`（ゲート構成）、git HEAD=6819ac9・テストベースライン 314 passed / 0 failed（実測）

## 1. 背景（親の実測と原因）

- 現行の解決順は env → `.env` → Keychain（`pick_api_key`）。env があれば Keychain に触れず早期リターン。
- `.env` が無い/キーが無いと Keychain を読み、macOS のアクセス承認ダイアログで**プロセスが無期限にブロック**する。
  実測: `.env` なし → `doctor` 25 秒タイムアウト（ハング）、`auth status` もブロック。`.env` あり →
  `auth status` 0.046 秒 / `doctor` 0.051 秒。
- 原因: macOS Keychain のアクセス制御は**バイナリ単位**で、項目を承認したバイナリ以外（`cargo run`・
  `target/debug`・再ビルドしたバイナリ）が読むと同意ダイアログが出る。
- ユーザー要望: **テストやデバッグのときは `.env` を使う。ダイアログ運用はおかしい**。

## 2. 振る舞い（BDD シナリオ / 具体例）

### K0. 取得元モードの選択（`KASEIFU_KEY_SOURCE`）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| K0-1 | 未設定・デバッグビルド | 既定 `env`（`default_for_build(true) == Env`。テストビルドでは `effective` が `env`） |
| K0-2 | 未設定・リリースビルド | 既定 `auto`（`default_for_build(false) == Auto`＝現行挙動） |
| K0-3 | `env` / `keychain` / `auto` | 前後空白除去・大小文字無視で受理し explicit=true |
| K0-4 | 空文字・空白のみ | 未設定扱い（ビルド既定へ。エラーにしない） |
| K0-5 | 不正値（例 `xyz`） | 即時エラー `invalid KASEIFU_KEY_SOURCE value "xyz": expected auto, env, or keychain`（exit 1） |
| K0-6 | プロセス env と `.env` の両方にあり | プロセス env 優先。env が空なら `.env` の値、両方無効なら既定 |
| K0-7 | 表示 | モードラベル `auto`/`env`/`keychain`。未設定由来は `(default)` 付記。由来ラベル `env` / `.env` / `keychain` / `not configured` / `unavailable (<原因>)` |

### K1. `env` モード（デバッグビルド既定）— Keychain 完全非接触

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| K1-1 | プロセス env `JEV_API_KEY` あり | `Configured` / `Source: env`。注入 store の get/set/delete 呼び出し **0 回** |
| K1-2 | `.env` に `JEV_API_KEY` あり | `Configured` / `Source: .env`。store 呼び出し **0 回** |
| K1-3 | どちらも無い | `Not configured` / `Source: not configured`。**ハングしない**（即時終了）。store 呼び出し **0 回** |
| K1-4 | `.env`/env の値が空・空白のみ | 未設定（現行同等）。store 呼び出し **0 回** |
| K1-5 | Keychain にキーが存在する | **無視**（読まない・書かない） |
| K1-6 | `setup` / `auth set-key` / `auth remove-key` | Keychain に触れず、明示メッセージ（`.env` を使う案内、リリースビルドでの登録案内）を返す |
| K1-7 | Jev を呼ぶコマンド（classify / organize / watch）でキー無し | 401 試行やハングではなく、明確な未設定エラーで exit 1（例は §5） |

### K2. `keychain` モード（Keychain 経路の検証用）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| K2-1 | store にキーあり + env/.env にもキーあり | store の値を採用（`Source: keychain`）。env/.env のキーは無視。store.get は **1 回** |
| K2-2 | store が空 | `Not configured`（get 1 回）。env/.env のキーは無視 |
| K2-3 | store がアクセス拒否 | `unavailable (<原因>)` を保持（get 1 回。診断は原因文字列を表示） |
| K2-4 | Jev を呼ぶコマンドでキー無し | 明確な未設定エラーで exit 1（Keychain 登録の案内） |

### K3. `auto` モード（現行挙動の維持）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| K3-1 | env/.env あり | env/.env を使用。store 呼び出し **0 回**（現行どおりの早期リターン） |
| K3-2 | env/.env 無し + store にキーあり | Keychain を使用（get 1 回） |
| K3-3 | どちらも無し | `Missing`。Jev 呼び出しコマンドは**現行どおり**試行（プリフライトなし＝挙動不変） |

### K4. `doctor` / `auth status` の表示

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| K4-1 | doctor・キー解決成功 | `ok API Key: provided via environment (JEV_API_KEY)` / `ok API Key: provided via .env` / `ok API Key: resolved from macOS Keychain` |
| K4-2 | doctor・env モードでキー無し | `FAIL API Key: not configured (KASEIFU_KEY_SOURCE=env); set JEV_API_KEY in the environment or in .env`（ハングなし・即時） |
| K4-3 | auth status | `Source:` 行（由来）と `KASEIFU_KEY_SOURCE:` 行（実効モード、既定時は `(default)` 付記）を追加 |
| K4-4 | auth status JSON | 既存キー（provider/api_key_configured/storage/endpoint）を維持し、`api_key_source` / `key_source_mode` / `key_source_default` を追加 |
| K4-5 | doctor の構造 | `diagnose` から store 引数を削除（env モードで Keychain に到達するコードパスが存在しない） |
| K4-6 | 表示一般 | ASCII のみ・既存の NO_COLOR/非 TTY/パネル方針を維持。キー値は決して出ない |

### K5. 品質ゲート

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| K5-1 | テスト | 既存 314 全緑（doctor テストは新シグネチャ・新チェック名へ更新、意図は不変）+ 新規テスト全緑 |
| K5-2 | 複雑度 | 新規 CC ≥ 16 を作らない |
| K5-3 | 依存 | 新規依存なし（`Cargo.toml`/`Cargo.lock` 変更なし） |
| K5-4 | 品質 | `bash scripts/quality_check.sh` が OVERALL PASS |
| K5-5 | 秘密 | `.env` の内容・キー値がコード/テスト/ログ/ドキュメント/コミットに無い（grep で確認） |
| K5-6 | 無干渉 | `discover` はデバッグ専用のまま、`inspect`/`classify`/`discover` の JSON 形状不変（doctor/auth status 以外） |

## 3. タスク一覧（実装順）

| # | タスク | 主なファイル | 期待する振る舞い（受け入れ） | テスト |
|---|---|---|---|---|
| T1 | `KeySourceMode` / `KeySourceSetting` / `ApiKeyOrigin` / パース / ラベル / ビルド別既定 | `src/config/key_source.rs`（新規）, `src/config/mod.rs` | K0-1〜K0-5, K0-7 | 単体（注入値のみ） |
| T2 | `LocatedValue`（値＋由来）導入、`resolve_with(cli, lookup, file, store, key_source)` 化、`pick_api_key` のモード分岐、`ResolvedConfig` に `key_source` / `api_key_origin` 追加（`api_key_from_env` は互換維持） | `src/config/resolver.rs` | K1-1〜K1-5, K2-1〜K2-3, K3-1〜K3-3, K0-6 | 注入 store テスト（T6） |
| T3 | `doctor::diagnose` から store 除去、`check_api_key` をモード・由来対応に | `src/doctor.rs` | K4-1, K4-2, K4-5 | T6 + 既存更新 |
| T4 | main.rs: `resolve_for` 経由のプリフライト（env/keychain のみ）、auth status / setup / auth set-key / remove-key / config show の Keychain 非接触化と表示。**規約: (1) mode 取得 → (2) `env` なら即メッセージ return → (3) store 生成/呼び出し**。setup は endpoint/model プロンプトと TOML 保存を継続し、キープロンプトと Keychain 書き込みをスキップ（intro/summary に案内、`saved to macOS Keychain` を出さない） | `src/main.rs` | K1-6, K1-7, K2-4, K4-3, K4-4 | T7（実バイナリ） |
| T5 | 既存テストのモード注入対応（`resolve_with` 新シグネチャ・`diagnose` 新シグネチャ・チェック名 `API Key`）。`tests/completion_test.rs` の `child()` に `env_remove("KASEIFU_KEY_SOURCE")` を追加（親環境継承による実 Keychain 接触のハザード除去） | `tests/v33_config_auth_test.rs`, `tests/v32_image_metadata_test.rs`, `tests/completion_test.rs` | 既存 314 の意図を保ったまま全緑 | `cargo test` |
| T6 | 注入 store（CountingStore: get/set/delete 回数記録 + 拒否モード）によるモード別・ゼロアクセス証明、doctor 表示 | `tests/key_source_test.rs`（新規） | K1-1〜K1-5, K2-1〜K2-3, K3-1〜K3-3, K4-1, K4-2, K4-5 | 単体統合 |
| T7 | 実バイナリ・一時 cwd/`.env` による CLI 検証（タイムアウト計測付き） | `tests/key_source_cli_test.rs`（新規） | K0-1(実測), K1-3, K1-6, K1-7, K4-2〜K4-4 | 子プロセス（env は子のみ） |
| T8 | ドキュメント | `README.md`, `documents/development.md`, `CHANGELOG.md`, `.env.example` | 解決順・モード・既定の説明、開発時の運用 | 目視 + grep |
| T9 | 品質ゲート実行と受け入れ実測（debug/release 両ビルド） | `reports/quality/`（生成物は git 管理外） | K5-1〜K5-4 | `bash scripts/quality_check.sh` |
| T10 | 新規ファイルの `git add` | 上記新規 4 ファイル + 計画/issue 文書 | tracked 化（`.gitignore` は変更しない） | `git status` |

### 失敗時・異常系の具体化

- **不正モード値**: resolve 系（doctor/auth status/setup/classify 等）は即時エラー。`completions`・`inspect` の
  JSON 出力等、解決を伴わない経路は影響なし。
- **`.env` が読めない/壊れている**: 現行どおり `.env` の解釈失敗は「無い」扱い（`dotenv_map` の既存仕様）。
- **空白のみのキー**: 未設定扱い（現行 `pick_api_key` と同じ）。
- **Keychain 拒否**: 読みは `Unavailable(原因)` として保持し、書き（setup/auth set-key）は従来どおりエラー表面化。

## 4. 設計判断と代替案

| 論点 | 採用 | 理由 / 却下した案 |
|---|---|---|
| モード選択の置き場所 | `src/config/key_source.rs`（純粋）＋ resolver に env/.env 読み取りの薄い関数 | resolver.rs に全部載せると責務が混ざる。key_source は入力を注入可能な純関数に保つ（既存規約「no process-environment mutation」） |
| 由来（env と `.env`）の区別 | lookup の戻り値を `LocatedValue { value, from_dotenv }` に拡張 | cwd 依存のテストを避けつつ `.env` 由来を決定的に扱える。テストはフラグ注入のみ |
| モード既定の判定 | `default_for_build(cfg!(debug_assertions))` を公開し、テストで両分岐を固定 | リリースビルドでのみ auto になることをテストで証明可能。ビルド成果物を 2 種作る必要がない |
| リリース既定の実測 | `auth status` が `KASEIFU_KEY_SOURCE: auto (default)` を表示 | リリースバイナリで Keychain を叩かずに既定モードを観測できる（env キーを与えて早期リターンさせれば無ダイアログ） |
| 診断での Keychain 再読み | **廃止**（resolver の 1 回の結果に由来・失敗原因を保持） | 現行は resolver と doctor/auth status で最大 2 回読む。再読みはダイアログ再発の温床。`diagnose` から store を消し構造的に 0 回を保証 |
| `auto` のキー欠落 | 現行どおり（試行）プリフライトなし | 仕様「auto ＝現行挙動を維持」を厳守。リリース一般ユーザーの挙動を変えない |
| `env`/`keychain` のキー欠落 | Jev 呼び出し前に明確なエラー（exit 1） | 仕様「キーが無ければプロンプトやハングではなく明確な未設定/エラー」。ローカルモック利用時はダミーキーで回避可能（development.md に記載） |
| `auth set-key` / `remove-key`（env モード） | Keychain 非接触で案内メッセージ、exit 0 + JSON `reason: "env-mode"` | 「書かない」を最優先。操作は失敗ではなくモードの方針（スクリプト向けに JSON で理由を返す） |
| doctor のチェック名 | `Keychain` → `API Key` | 由来が Keychain とは限らなくなったため。件数は 7 のまま、既存テストは名前のみ更新 |
| `Storage:` 行（auth status 人向け） | `Source:` に置換。JSON の `storage` は互換のため残す | 人向けは解決元の明示が要件。JSON は既存キー・値とも維持し追加のみ |
| `.env` からの `KASEIFU_KEY_SOURCE` | 読む（プロセス env 優先） | 開発チェックアウトの `.env` でモードを固定できる。既存 `dotenv_value` を再利用 |
| ヘルプ文言 | `setup`/`auth` の 1 行説明のみ調整 | デバッグで setup が Keychain に書かない事実と乖離しないため |
| JSON `storage` 値のマッピング（互換固定） | `Environment`/`Dotenv` → `"Environment"`, `Keychain` → `"macOS Keychain"`, `Missing` → `"-"`, `Unavailable(msg)` → `msg` | 由来ベース実装でも現行と同一の値を再現する（追加キー `api_key_source` / `key_source_mode` / `key_source_default` のみ新規） |
| `api_key_from_env` | 互換維持（`Environment`/`Dotenv` で true） | 既存テスト・公開面の破壊を避ける（導出値のため乖離しない） |

## 7. Plan Architect レビュー反映（`issues/20261013-plan-architect-key-source.md`）

| # | 指摘 | 反映 |
|---|---|---|
| A1 | 🔴 呼び出し元総数と型推論 | §5 に呼び出し元一覧を追記。v32 の `None` は必要なら `Option::<LocatedValue>::None` と明示（T5） |
| A2 | 🔴 `env_or_dotenv` の他用途追従 | T2 で `pick_endpoint`/`pick_model` を同時更新し、既存 endpoint/model テストを先に緑化（§3 T2 の列に反映） |
| A3 | 🔴 main.rs の store ゲート順序 | T4 に「mode 取得 → env 即 return → store」の規約とアクセス点削減（key_status の store 読取廃止）を明記 |
| A4 | 🟡 setup の env 仕様 | T4 に詳細（TOML 継続・キー工程スキップ・summary 文言）を明記 |
| A5 | 🟡 既存テストの環境ハザード | T5 に `completion_test.rs` の `env_remove` 追加を明記 |
| A6 | 🟡 受け入れ網羅 | §5 に organize/watch の手動実測を追記（T7 は classify を自動化） |
| A7 | 🟢 JSON 互換マッピング | §4 にマッピング表を追加 |

## 5. 検証（受け入れ基準 → 証跡）

| AC | 証跡 |
|---|---|
| 1 ハングなし・明確な未設定 | 一時 cwd（`.env` なし）で `KASEIFU_KEY_SOURCE=env` + `env -u JEV_API_KEY` の `doctor` / `auth status` / `classify` を実バイナリ実行し、経過時間・exit code・出力を記録。タイムアウト付き実行はテスト内ヘルパで自動化 |
| 2 既定の固定 | `default_for_build(true)==Env` / `default_for_build(false)==Auto` のテスト + 実バイナリ表示（debug: `env (default)` / release: `auto (default)`） |
| 3 env モード 0 アクセス | CountingStore（get/set/delete 個別カウント）で resolve + doctor が 0 を記録。CLI はゲートの位置（全 store 呼び出しがモード判定の後）をレビューで確認 |
| 4 keychain モードの逆分離 | env/.env にキーがあっても store の値を使用。get 1 回をカウントで証明 |
| 5 表示と即時性 | `doctor` / `auth status` の `Source:` / `API Key` 行と、env モード < 1 秒（debug 実測） |
| 6 品質 | `bash scripts/quality_check.sh` OVERALL PASS、314 + 新規 all green、新規 CC ≥ 16 なし、依存追加なし |
| 7 秘密の非混入 | `git diff`/`git status` に `.env` なし。新規コード・テストはダミー値のみ。`grep` でキー様文字列なしを確認 |
| 追加 | 全呼び出し元の影響範囲確認: `resolve_with`=v32 2 + v33 1、`resolve_with_env`=main 5 + v33 3、`diagnose`=main 2 + v33 3、`ResolvedConfig` 直接構築=resolver のみ（grep で確認済み） |
| 追加 | organize / watch の env モード実測（手動）: 一時 cwd・キー無しで即時 exit 1、fsevents 起動前に停止 |

## 6. リスク

| リスク | 影響 | 緩和 |
|---|---|---|
| デバッグ既定 `env` への変更で、デバッグ時に Keychain キーが使えなくなる | 開発者が `cargo run` で本番キーを使えない | `KASEIFU_KEY_SOURCE=auto cargo run -- …` またはリリースビルドで明示的に可能（README/development.md に記載） |
| `resolve_with` のシグネチャ変更 | 既存テストのコンパイルエラー | 2 ファイルのみ（v32/v33）。機械的な更新で済むことを事前確認済み |
| doctor のチェック名変更 | 既存テスト 2 箇所の文字列 | 意図（キー欠落で fail）は不変のまま更新 |
| 実バイナリテストが実 Keychain / 実補完ディレクトリに触れる | 環境汚染 | 子プロセスのみ env 変更、`KASEIFU_NO_COMPLETION_UPDATE=1`、HOME を一時ディレクトリへ |
| doctor のエラー文言の変化 | 既存利用者の目視出力 | CHANGELOG に明記 |
