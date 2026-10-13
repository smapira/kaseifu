# Plan Architect レビュー: ゼロコンフィグ仕上げバッチ（ZC-11 / ZC-9 / ZC-10）実装計画

- 日付: 2026-10-16
- 対象: `documents/plans/dev/20261016b-zero-config-finish.md`
- 関連コード: `src/main.rs`, `src/auth/{keychain,prompt}.rs`, `src/completion/mod.rs`, `src/config/{storage,key_source}.rs`,
  `src/storage/history.rs`, `src/doctor.rs`, `src/presentation/{format.rs,components/panel.rs,renderers/generic.rs,renderers/mod.rs}`,
  `src/watch/runner.rs`, `tests/v052_zero_config_cli_test.rs`, `tests/v070_zero_config_p1_help_test.rs`, `tests/completion_test.rs`,
  `tests/user-test-plan.csv`, `README.md`, `scripts/quality_check.sh`
- 確認手段: 直接読解 + `grep`（serena / context7 / codebase-memory-mcp / graphify は本環境で未接続）

## 検証したこと（計画の前提が実コードと一致するか）

- `Commands` の match は `src/main.rs` の `dispatch` 1 箇所のみ（`grep 'Commands::' src/` = 18 件すべて main.rs）。
  `Uninstall` 追加は dispatch と enum の 2 箇所で完結し、他の網羅 match を壊さない。
- `tests/completion_test.rs:237-266` はトップレベルコマンドを**バイナリの help から自動列挙**して zsh スクリプトとの整合を固定する。
  可視コマンド追加は自動で対象になる（uninstall が補完に載って当然のテストになる）。
- `tests/v070_zero_config_p1_help_test.rs:168-227` も可視コマンドを自動列挙し、`-h`/`--help` を禁止語・ASCII で走査する。
  uninstall の help 本文はこの走査だけでカバーされる（計画 C1-11 の前提どおり）。
- `history::LEGACY_DATABASE_PATH` / `inspect_journal` / `completion::RefreshPlan::from_env().candidates` / `KeySourceMode::keychain_allowed()` /
  `user_config_path()` はすべて pub。uninstall はこれら 4 つの既存入力だけで対象を決定できる（新しい探索ロジックを作らない）。
- `src/auth/prompt.rs:35-41` は全 prompt が非TTYで即エラー（ハングしない）。C1-9（非TTY + 確認）はこの原則に合わせ、
  **削除前に** `stdin.is_terminal()` を検査する設計が正しい。
- `emit_report`（`src/main.rs:1073-1089`）は JSON / Rich パネル / plain 行の 3 系統を共通化しており、uninstall もこれに乗せるのが最小差分。
- README 内の `](#...)` アンカーリンクは現状 **0 件**（`grep '](#' README.md` = 0）。他ドキュメントから README アンカーへの参照も 0 件。
  したがって ZC-10 のアンカー破壊リスクは「新規に張るリンク」の解決性だけであり、テストで固定すればよい。
- `tests/phase6_watch_test.rs` は `log_outcome` / `watch_line` の文言を参照しない（`grep 'watching|dry-run|would move' tests/` の
  ヒットは手動計画 CSV と他コマンドのみ）。文言変更で落ちる既存テストは無く、CSV T55/T56 だけが意図的更新対象。

## 指摘事項（実装に反映済み）

### 🔴 F1. uninstall は main の補完自動更新から除外する

`src/main.rs:263-271` は `Completions` / `Doctor` / 引数なしを自動更新から除外している。uninstall が除外されないと、
ドライラン直前に補完が再生成されて報告と実態がずれ、「消したのに次の実行で戻る」混乱を自前で作る。

**反映**: 計画 §3.1 / T1（`Uninstall` を除外リストに追加）。

### 🔴 F2. `env` モードの Keychain 非接触は 2 重に担保する

原典 §7 の「`env` では触れない（読みも削除も）」は、plan 段の action=Keep だけでは実行時バグで破られ得る。
`execute` にも `keychain_allowed` を渡し、**両方**でゲートする。spy ストア（全呼び出しを数える `SecretStore`）で 0 件を固定する。

**反映**: 計画 §3.1 / C1-4 / T1。

### 🔴 F3. 旧ジャーナルの削除は「CWD のもの」かつ「読めたもの」だけ

原典 §7 の表と受入基準 6 をそのまま実装する。他ディレクトリは探索対象にしない（列挙・案内のみ）。
`inspect_journal` が失敗するファイルは内容不明のため削除しない（doctor の「unreadable は check yourself」と同じ原則）。

**反映**: 計画 §3.1 / C1-6 / T1。

### 🟡 F4. 部分削除と exit code の整理

原典は exit code を規定していない。`doctor` が FAIL 行があっても 0 で終わる前例に合わせ、uninstall も常に 0 とし、
失敗は `FAIL` 行 + サマリの `failed (partial result; nothing is rolled back)` で伝える。受入基準が要求する exit 0
（ドライラン・冪等）と矛盾しない。唯一の例外は **非TTY + `--remove-key`（`--yes` なし）で、これは削除前に非 0 で停止**する
（prompt 群の既存原則。partial state を作らない）。

**反映**: 計画 §3.1 / §7-1 / C1-9。

### 🟡 F5. `--purge` の undo 履歴警告は「削除前」から見えること

原典 §7 の「`--purge` 時に影響を明示して削除」は、ドライラン表示と実行の両方で「削除すると undo 履歴が失われる」が
読めることを要求する。詳細文を status 別に生成する renderer 設計で担保する。

**反映**: 計画 §3.1 / C1-3 / T1。

### 🟡 F6. 非TTY の `--execute --remove-key` は「削除前に」失敗させる

TTY 判定を最初の削除の前に置く（prompt 失敗による partial state を構造的に排除）。
`--purge --execute --yes` は非TTYでも完遂できることをテストで固定する。

**反映**: 計画 §3.1 / C1-9 / T1。

### 🟢 F7. watch 文言のテスト可能化

`log_outcome` は `println!` 直書きで文言をテストできない。出力文字列を組み立てる `outcome_text` を抽出し、
`log_outcome` はそれを印字するだけにする（挙動不変、`grep` で他呼び出し 0 件を確認）。手動計画 CSV の T55/T56 も更新する。

**反映**: 計画 §3.2 / T2。

### 🟢 F8. README は「冒頭の完結」と「後段の不変」を両立させる

P1 の README テスト（対象拡張子・Spotlight R6）が参照する節は移動しない。移動するのは `### Install` / `### Quick Start` のみ。
新設 `## Quick Start` に情報を集約し、`#usage` / `#configuration` / `#how-it-works` へのリンクをテストで解決させる。

**反映**: 計画 §3.3 / C3 / T3。

## 備考

- F1〜F8 は計画 §3・§4・§7 に反映済み。上記をもって計画を**承認**し、Implementer へ実装を依頼する。
