# KASEIFU_KEY_SOURCE 実装の QA 検証

- 日付: 2026-10-13
- 対象: `src/config/key_source.rs`（新規）, `src/config/resolver.rs`, `src/doctor.rs`, `src/main.rs`,
  `tests/key_source_test.rs`（新規）, `tests/key_source_cli_test.rs`（新規）, ドキュメント 4 件
- 検証方法: `bash scripts/quality_check.sh`、debug/release 実バイナリの実測、秘密情報 grep、
  store 呼び出し元の全列挙（`grep`）、`.env` 非接触確認

## 1. 品質ゲート

```
rustfmt                PASS   violations 0
clippy                 PASS   warnings 0
cargo-test             PASS   passed 346, failed 0
complexity             PASS   analyzer 0.0.25
baseline-gate          PASS   no new/worsened CC>=16
report-tests           PASS   complexity_report self-tests green
coverage               PASS   report-only (line 78.92%)
OVERALL: PASS
```

- テストは既存 **314 + 新規 32 = 346 passed / 0 failed**。既存への変更は次のみ:
  v33 doctor テスト 3 件のモード注入・新シグネチャ化（意図不変）と check 名 `API Key` 化、
  v32 の `resolve_with` 2 箇所のモード引数化、`completion_test.rs` の `child()` に
  `env_remove("KASEIFU_KEY_SOURCE")` 1 行（親環境からの実 Keychain 接触ハザード除去）。
- 複雑度: 新規関数の最大 CC は `pick_api_key` / `check_api_key` の 6。新規 CC ≥ 16 は 0 件。
- 依存追加なし（`Cargo.toml`/`Cargo.lock` は今タスクでは不変）。

## 2. 受け入れ基準の実測（debug ビルド・一時 cwd・`.env` なし）

| # | コマンド（一時ディレクトリ・JEV_API_KEY なし） | 実測 |
|---|---|---|
| 1a | `KASEIFU_KEY_SOURCE=env kaseifu auth status` | 0.00〜0.01s、exit 0、`API Key: Not configured` / `Source: not configured` / `KASEIFU_KEY_SOURCE: env`（ハングなし） |
| 1b | `KASEIFU_KEY_SOURCE=env kaseifu doctor` | 0.014s、exit 0、`FAIL API Key: not configured (KASEIFU_KEY_SOURCE=env); set JEV_API_KEY in the environment or in .env` |
| 1c | `KASEIFU_KEY_SOURCE=env kaseifu classify note.txt` | 0.013s、exit 1、`Error: no API key is configured (KASEIFU_KEY_SOURCE=env); set JEV_API_KEY ...` |
| 1d | `KASEIFU_KEY_SOURCE=env kaseifu organize note.txt` | 0.029s、exit 1、上記と同一の明確なエラー |
| 1e | `KASEIFU_KEY_SOURCE=env kaseifu watch --path <tmp>` | 0.010s、exit 1（fsevents 起動前に停止） |
| 2a | debug 既定（`KASEIFU_KEY_SOURCE` 未設定）`auth status` | `KASEIFU_KEY_SOURCE: env (default)`、0.00s |
| 2b | release ビルド（`cargo build --release`）で `JEV_API_KEY=<dummy>` + 未設定 | `KASEIFU_KEY_SOURCE: auto (default)` / `Source: env`（env キーで早期リターンするため Keychain 非接触で観測可能） |
| 3 | 注入 store の記録 | `tests/key_source_test.rs`: env モードの resolve / doctor で get/set/delete 合計 **0**。store 呼び出しは src 全体で 5 箇所のみ（resolver 1 + setup 1 + set-key 2 + remove-key 2 相当）で、env モードでは全てゲート後 |
| 4 | `keychain` モード（注入 MemoryStore） | env/.env のキーを無視し store の値を採用、get は解決あたり 1 回。doctor も再読しない |
| 5a | `.env`（ダミーキー）+ env モード `auth status` | 0.027s、`API Key: Configured` / `Source: .env`、doctor は `ok API Key: provided via .env` |
| 5b | `.env`（ダミーキー）+ env モード `classify --jev-endpoint http://127.0.0.1:9` | 1.07s（Spotlight 探索含む）で Jev 段階まで到達（キー検査を通過）→ TransportError（キー未設定エラーではない） |
| 6 | `auth set-key` / `auth remove-key --yes`（env モード、stdin=/dev/null） | 両方 exit 0。前者は「env mode ... Keychain is not used」、後者は JSON `{"removed":false,"reason":"env-mode","key_source_mode":"env"}` |
| 7 | `setup`（env モード、stdin=/dev/null） | ガイダンス表示後に TTY エラーで exit 1（キープロンプトなし・Keychain 非接触）。実測は CLI テストで固定 |
| 8 | 不正値 `KASEIFU_KEY_SOURCE=xyz`（env / `.env` の両方） | `Error: invalid KASEIFU_KEY_SOURCE value "xyz": expected auto, env, or keychain`、exit 1 |
| 9 | read-only 経路（`inspect` 0.090s / `discover` 1.078s、env モード） | ハングなし（`inspect_image_options` の解決も Keychain 非接触） |
| 10 | 空 `JEV_API_KEY` + `auto`（実機 Keychain に項目あり） | 0.015s で `Source: not configured`（旧実装と同じくフォールバックしない。ダイアログなし） |

### Keychain 非アクセスの証明方法

1. **注入 store の記録（一次証拠）**: `CountingStore`（get/set/delete を個別カウント、`AtomicUsize`）で
   env モードの resolve + doctor が 0 回であることを assertion（`env_mode_*` / `doctor_env_mode_*` テスト）。
   空値・store にキーありのケースも含む。
2. **到達可能性の全列挙（構造的証拠）**: `store.get/set/delete` の全出現を grep で列挙し、env モードでは
   （a）resolver の `pick_api_key` が env 分岐で store を呼ばない、（b）main の write 系は モード判定→即 return、
  の 2 点で到達不能。`doctor::diagnose` は store 引数自体を削除（コンパイルレベルで保証）。
3. **実測（行動的証拠）**: Keychain 項目が存在する実機（親の計測と同じ環境）で、env モードの
   doctor / auth status / classify / organize / watch / inspect が 0.01〜1.08 秒で完了（ダイアログが出る場合は
   無期限ブロックするため、即時完了 = 非接触）。CLI テストは 10 秒デッドラインでハングを検知して fail する。

## 3. JSON 互換・表示

- `auth status --format json`: 既存キー `provider` / `api_key_configured` / `storage` / `endpoint` を値ごと維持し、
  `api_key_source` / `key_source_mode` / `key_source_default` を追加（実測: `.env` キーで
  `"storage":"Environment","api_key_source":".env","key_source_mode":"env"`）。
- `doctor --format json`: `checks[]` の構造不変。`API Key` チェックは name と detail のみ（CHANGELOG に明記）。
- 人向け出力は ASCII のまま（キー値は一切出ない。CLI テストで `!contains(DUMMY_KEY)`）。
- `NO_COLOR`/非 TTY・TUI 禁止・`discover` デバッグ専用の既存方針に変更なし。

## 4. テストの安全性

- 実バイナリテストは毎回 一時 cwd + `HOME=<tmp>` + `KASEIFU_NO_COMPLETION_UPDATE=1` + stdin=null で実行し、
  実 `.env`・実 config・実補完ディレクトリに触れない。`keychain` モードは意図的に CLI テスト対象外
  （実 Keychain を読む唯一のモード。検証は注入 store のみ）。
- `cargo test` 中に実ユーザー Keychain へ読み書きするテストは存在しない（実サービス
  `com.routeflags.kaseifu.jev` は定数比較のみ。Keychain 往復テストは既存の `.test` サービス）。

## 5. 秘密情報・構造の一貫性

- `.env` は存在せず（`ls`: No such file）、gitignore 済み（`git check-ignore` で確認）。読解・作成なし。
- 変更・新規ファイルにキー様リテラルなし（正規表現 grep）。テストの値は明白なダミーのみ。
- README（モード表・setup/auth/doctor の説明）/ `documents/development.md`（env 表・解決順・モック時の注意）/ 
  CHANGELOG `[Unreleased]`（Added/Changed）/ `.env.example`（コメント例）を更新。
- 新規ファイル 5 件（`src/config/key_source.rs`, `tests/key_source_test.rs`, `tests/key_source_cli_test.rs`,
  計画書, Plan Architect issue, Reviewer issue, 本 QA issue）を `git add` で tracked 化（§6）。

## 6. git 状態

- `git add` 対象（今タスクの新規）: 上記。`.gitignore` は変更しない。
- コミット・タグ・バージョン変更は親が実施（未実施）。

## 7. 結論

受け入れ基準 1〜7 を満たすことを実測で確認。**QA 承認**。

### 参考（スコープ外の観察）

- `CHANGELOG.md` の [Unreleased] 既存エントリ（`organize` 出力）末尾が
  "...without ANSI" で文が途切れている（今タスク以前からの既存。親の判断で修正推奨）。
