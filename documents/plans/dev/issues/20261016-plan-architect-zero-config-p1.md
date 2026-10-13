# Plan Architect レビュー: ゼロコンフィグ P1 実装計画

- 日付: 2026-10-16
- 対象: `documents/plans/dev/20261016a-zero-config-p1.md`
- 関連コード: `src/main.rs`, `src/presentation/renderers/{generic,classify,organize}.rs`,
  `src/operations/{plan,mover,organize}.rs`, `src/classifier/{pipeline,jev}.rs`,
  `src/discovery/{mod,destinations,similarity}.rs`, `src/context/image_metadata.rs`,
  `tests/v052_zero_config_cli_test.rs`, `tests/completion_test.rs`, `tests/organize_output_test.rs`, `scripts/quality_check.sh`
- 確認手段: 直接読解 + `grep`（serena / context7 / codebase-memory-mcp / graphify は本環境で未接続）

## 検証したこと

- `organize --format json` を固定する既存テストは無い（`grep` で 0 件）。`--format json` の organize/undo/scan/watch 非対応は
  原典 §8 F-07 の指摘と一致し、ZC-7 の追加は互換破壊ではない。
- `render_panel` / `render_text` は `tests/organize_output_test.rs`, `tests/v31_render_test.rs`, `tests/presentation_test.rs` が**現行シグネチャで**呼ぶ。
  ZC-5 は `*_with` を追加し、既存関数は既定閾値で委譲する方針（計画 §3.1）が正。
- トップレベル help は `tests/v052_zero_config_cli_test.rs::v052_help_still_lists_every_command` が「全コマンド列挙 + `discover` 非表示」を固定。
  計画のヘルプ文言は**コマンド名を列挙するだけ**でこの契約を維持し、`--format` の説明に `discover` を書かない（§3.5）必要がある。
- `completion_test.rs` は生成スクリプトのコマンド名と `discover` 非混入のみを固定し、説明文には依存しない（文言変更は安全）。
- `mdfind` は PATH から `Command::new` で実行される（`src/platform/macos/spotlight.rs:119-131`）。テストの偽 `mdfind` 注入が可能。
- `is_excluded_destination` は「絶対パスの第一成分」で system root を拒否する（`src/discovery/destinations.rs:126-133`）。
  したがって `std::env::temp_dir()`（`/var/folders/...`）や `$HOME=/var/...` 配下は候補にならない。候補フィクスチャは
  `CARGO_TARGET_TMPDIR`（worktree 内・`target/` は gitignore 済み）に置く必要がある（§6 に反映済み）。
- `PlanError` の網羅 match は `renderers/organize.rs:152-180`（`plan_error_message`）のみ。`DestinationExcluded` 追加時はここを更新する。
- `pipeline::discover_candidates_with_options`（`src/classifier/pipeline.rs:202-215`）は observe → normalize → discover の再利用候補そのもの。
  public 化（`discover_candidates`）で select 経路の重複実装を避けられる。
- `doctor` の 9 チェックは `tests/v052_zero_config_cli_test.rs:425` が固定。ZC-8 で検査を増やさない方針が正。
- `libc` は通常依存（`Cargo.toml:26`）で、`cfg(target_os = "macos")` ガードの `openpty` / `kill` は既存パターン内。

## 指摘事項（逐次反映）

### 🔴 F1. ヘルプの禁止語は「全出力」で機械的に走査する

`-h` は短い説明、`--help` は長い説明を出すため、修正漏れは片方にだけ残り得る。入れ子（`auth` / `config`）と隠し
`discover` の直接 invoke も走査対象にする。

**改善案**: テストで `Cli::command()` から可視コマンドを再帰列挙し、各 `-h` / `--help` を子プロセスで実行して
`Phase` / 型名 / パイプライン語の 0 件を固定する。列挙は手書きリストにしない（将来のコマンド追加でも自動で対象になる）。

### 🔴 F2. 「既存の表示・JSON は不変」の線引き

ZC-5 の JSON は `--explain` 指定時のみ `explanation` を**追加**する。既定の classify JSON（`decision` オブジェクト）は
キー順・値とも完全不変でなければならない。

**改善案**: `serde_json::to_value` → 追加キー挿入 → `to_string_pretty` の順で組み立て、`--explain` 無しでは
既存の `to_string_pretty(&decision)` と同じ結果になることをテストで固定する。

### 🟡 F3. ZC-13 は `resolve_for_jev_command` を絶対に通さない

通過すると TTY で鍵プロンプトが出る（ゼロコンフィグの否定）。鍵の有無で分岐する余地も作らない。

**改善案**: select 経路は `config/acquire/jev` のいずれも import しないか、せいぜい候補列挙のみに留める。
テストは「鍵プロンプトが出ない」「閉ポートの `JEV_API_URL` でも成功する」で構造を固定する。

### 🟡 F4. 非TTY の `--select` は副作用ゼロで即エラー

TTY 判定は discovery より**前**に行う（無駄な `mdfind` も走らせない。テストも高速・決定論的になる）。

**改善案**: `handle_organize_select` の先頭で `stdin.is_terminal()` を判定し、非TTY は案内文付きで `bail`。
`--format json` のときだけ対話なしで候補列挙を許可する。

### 🟡 F5. `--execute` + `--select` + `--format json` の沈黙無視を禁止

JSON は対話しないため `--execute` が効かない。ヘルプで「実際に機能するフラグ」を守るため、この組合せは明示エラーにする。

**改善案**: 実行時エラー「--format json prints the candidate list and does not select; run without --format json with --execute」。

### 🟢 F6. R6 の README 文言は既存の Spotlight 節と重複させない

既存 README:52-57 に「about 4 s measured」がある。新規 ZC 番号は振らず、`mdfind`（Discovery の検索）を明示する 1〜2 行に整理する。

### 🟢 F7. 新規ファイルの扱い

`src/presentation/renderers/{explain,select}.rs`, `src/operations/select.rs`, 新規テスト 4 ファイルは `git add` しない（親が実施）。
`reports/quality/` は生成物（管理外）。

## 備考

- F1〜F5 は計画 §2（C5-1 / C1-5 / C6-11 / C6-5 / C3-5）と §3 に反映済み。F6・F7 もタスク T4 / T8 に含まれる。
- 上記反映をもって計画を**承認**し、Implementer へ実装を依頼する。
