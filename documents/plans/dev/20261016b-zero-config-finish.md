# ゼロコンフィグ 仕上げバッチ（ZC-11 / ZC-9 / ZC-10）実装計画

- 起票: 2026-10-16
- 原典: `documents/plans/user/20261013d.md`（ゼロコンフィグ化 改善計画 v1.0、§6 P2、§7 ZC-11、§11 実装順序 4、§12 完了条件、§13 完了報告）
- 前提: P0（v0.6.0）と P1（v0.7.0、ZC-5/6/7/8/12/13）は着地済み（`d436451`、タグ `v0.7.0`）。
  参考: `documents/plans/dev/20261016a-zero-config-p1.md`、`tests/v070_zero_config_p1_*_test.rs`。**P0/P1 の挙動を壊さない**
- 対象: `src/main.rs`, 新規 `src/uninstall.rs`, 新規 `src/presentation/renderers/uninstall.rs`,
  `src/presentation/renderers/{mod,generic}.rs`, `src/watch/runner.rs`, `src/lib.rs`,
  `README.md`, `CHANGELOG.md`, `tests/user-test-plan.csv`, 新規テスト 2 ファイル
- 対象外（実装しない）: 将来の `--to <dir>`（ZC-14 として予約）。その他の新機能。
  仕上げバッチのスコープは ZC-11 → ZC-9 → ZC-10 の直列のみ
- 版数接頭辞: 新規テストは **`v080_*`**（本バッチは ZC-11 の新コマンド面を含むユーザー向け機能追加であり、
  次のマイナー（0.8.0）として着地する想定。v051/v052/v070 の前例に倣う）
- 制約: 原典 §10 の非機能要件（秘密情報なし・ドライラン既定・ASCII のみ・TUI 禁止・NO_COLOR/非TTY 準拠・
  既存 JSON キーを削除しない・`discover` はデバッグ専用・`KASEIFU_KEY_SOURCE` の意味不変・新規依存なし・
  新規 CC16+ なし・既存最大 CC を悪化させない・実ユーザーの `~/Library/Application Support/kaseifu/`・
  `~/Desktop/`・補完ディレクトリ・実 Keychain をテストで読み書きしない）。commit / push / tag は親が実施
- ベースライン実測（2026-10-16、worktree `kaseifu`、HEAD `d436451`、clean）:
  `cargo test --all-targets --all-features` = **428 passed / 0 failed**
- 確認手段: serena / context7 / codebase-memory-mcp / graphify は本環境で未接続のため、LSP 相当の直接読解と
  `grep`（`src/main.rs` / `src/uninstall.rs` 新設箇所 / `src/auth/**` / `src/completion/mod.rs` /
  `src/config/**` / `src/storage/history.rs` / `src/watch/**` / `src/presentation/**` / `tests/**` /
  `scripts/quality_check.sh` / `README.md` / `CHANGELOG.md`）で実コードを確認した

## 1. 背景と現状実測（変更前）

| ID | 現状（実測・コード根拠） | 影響 |
|---|---|---|
| ZC-11 | `uninstall` サブコマンドは存在しない（`grep -r uninstall src/` = 0 件。`cargo uninstall` の語のみ README 外に無し）。Keychain の削除経路は `auth remove-key`（`src/main.rs:1417-1473`、`KeychainStore::production()` 固定）だけ | アンインストール手順が無く、`~/Library/Application Support/kaseifu/`・補完・Keychain をユーザーが手作業で消すしかない |
| ZC-11 | 可視コマンド 11 個のヘルプは `tests/v070_zero_config_p1_help_test.rs::every_help_output_avoids_internal_phases_types_and_pipeline_words` が自動列挙で走査。**新コマンドを追加すると自動で対象になる**（手書きリストにしない設計、P1 の F1 指摘） | 追加時は禁止語 0・ASCII 0 を新コマンドのヘルプ本文でも満たす必要がある |
| ZC-11 | `doctor` の表記は `ok Name: detail` / `FAIL Name: detail (next: ...)`（`src/main.rs:1626-1637`）。パネルは `+-- * TITLE ---+` / `| ... |`（`src/presentation/components/panel.rs`） | uninstall の表示も同系のチェック行（remove / keep の区別）に合わせる（原典 §7 表示要件） |
| ZC-11 | ジャーナルは `history::journal_path()` = `~/Library/Application Support/kaseifu/kaseifu.db`、旧は `history::LEGACY_DATABASE_PATH` = `data/kaseifu.db`（CWD 相対、`src/storage/history.rs:16-37`）。`user_config_path()` = `~/Library/Application Support/kaseifu/config.toml`（`src/config/storage.rs:15-17`） | `--purge` の対象 2 つと旧ジャーナルの位置はこの 3 関数で確定する |
| ZC-11 | `KASEIFU_KEY_SOURCE=env` は Keychain 不接触（`KeySourceMode::keychain_allowed()`、`src/config/key_source.rs:61-67`。`auth remove-key` は先頭で mode をゲート: `src/main.rs:1419-1434`） | uninstall も同じゲートを先頭に置き、`env` では読みも削除もしない |
| ZC-11 | 補完スクリプトの検出は `completion::RefreshPlan::from_env().candidates`（`KASEIFU_COMPLETION_DIR` > Homebrew/Prezto/XDG/`~/.zsh` 候補、`src/completion/mod.rs:100-127, 243-255`）。自動更新は main の dispatch 前に走り、`Completions`/`Doctor`/引数なしは除外されている（`src/main.rs:263-271`） | uninstall は「候補リストの各ディレクトリに実在する `_kaseifu`」を列挙して消す。**uninstall 自体も自動更新から除外**しないと、消した直後・報告直前に再生成され得る |
| ZC-9 | 非 Rich の `watch` 開始行は `watching {roots} (dry-run: {})`（`src/main.rs:778`、Rust の `true`/`false` を直書き）。`runner::log_outcome` の WouldMove は `dry-run {} (would move to {})`（`src/watch/runner.rs:149-153`）。他コマンドの語彙は `dry run (no files moved)` / `execute mode`（`generic::dry_run_banner`、`src/presentation/renderers/generic.rs:33-39`） | 語彙が 2 系統に分裂し、`dry-run: true` は機械値の露出。手動テスト計画 `tests/user-test-plan.csv` T55/T56 も旧文言を固定 |
| ZC-9 | Rich の `watch_line` は dry のときだけ `event (dry run (no files moved))` を付け、execute ではモードを表示しない（`src/presentation/renderers/generic.rs:70-81`） | execute でもモードを明示して揃える |
| ZC-10 | README は `## How it works`（設計）→ `## Usage` → `### Install` → `### Quick Start` の順で、最初の「動く」情報が 111 行目以降 | 冒頭で「インストール → 1 コマンド」が完結しない |

### 既存テストへの影響（意図的変更のみ）

- `tests/v070_zero_config_p1_help_test.rs` の全ヘルプ走査は可視コマンドを**自動列挙**するため、`uninstall` 追加で対象が 1 つ増える（禁止語 0 / ASCII 0 を満たす）。
- `tests/v052_zero_config_cli_test.rs::v052_help_still_lists_every_command` の列挙リストに `uninstall` を追加する（表示契約の意図的更新）。
- `tests/user-test-plan.csv` T55/T56 の期待文言を新語彙に更新する（文言の意図的更新）。
- `tests/phase6_watch_test.rs` は `log_outcome` / `watch_line` の文言を固定していない（`grep` で 0 件）。`log_outcome` は出力文字列を組み立てる `outcome_text` に小分割し、文言をテスト可能にする（挙動は不変）。
- README 変更は P1 テストの `readme_scope_list_matches_the_image_predicate`（Spotlight 節・対象/対象外）を壊さない（該当節は移動しない）。

## 2. 振る舞い（BDD シナリオ / 具体例）

### C1. ZC-11: `kaseifu uninstall`

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C1-1 | 正常系（ドライラン） | `kaseifu uninstall`（隔離 HOME・一時補完ディレクトリ・`env` モード・CWD に旧 `data/kaseifu.db`）: 何も削除せず、補完 `_kaseifu` を `remove`、config / journal / 旧ジャーナル / Keychain / バイナリを `keep`（または `none`）で列挙し、`1 target(s), N B total (dry run; nothing was deleted)` を表示。exit 0 |
| C1-2 | 正常系（`--execute`） | `kaseifu uninstall --execute`: 補完のみ削除（`removed Completion: ...`）。config / journal / 旧ジャーナルは残り、Keychain は読みも削除もしない。exit 0 |
| C1-3 | 正常系（`--purge --execute`） | config と journal を削除し、journal 行に「undo 履歴が失われる」旨を削除前から明示。旧 CWD ジャーナルも削除（`--execute --purge` のときのみ）。Keychain は `--yes` なしなら対話確認（拒否なら `keep ... cancelled`） |
| C1-4 | 正常系（`--remove-key --yes`） | 注入 `MemoryStore` で削除経路を固定（実 Keychain 不使用）。`env` モードでは 1 回も get/delete を呼ばない（spy で 0 件） |
| C1-5 | 冪等 | 2 回目の `--execute`（および `--purge --remove-key --yes`）は `nothing to remove` を表示して exit 0 |
| C1-6 | 安全（旧ジャーナル） | 他ディレクトリの `data/kaseifu.db` は列挙にも削除にも関与しない。削除は CWD の `data/kaseifu.db` かつ `--execute --purge` のときだけ。読み取り不能な旧ジャーナルは削除しない |
| C1-7 | 安全（移動済みファイル） | kaseifu が移動したファイル・ディレクトリには触れない（`Moved files` 行で対象外を明示）。フィクスチャのファイルは実行後も無傷 |
| C1-8 | 異常系（補完の失敗） | 補完パスが通常ファイルでない場合 `FAIL Completion: ...` を表示して続行し、他（config / journal）の削除は完了する（巻き戻さない） |
| C1-9 | 異常系（非TTY + 対話確認） | `KASEIFU_KEY_SOURCE=keychain` + 非TTY + `--remove-key`（`--yes` なし）は**削除前に**非 0 で終了し、`--yes` を案内。何も削除しない |
| C1-10 | JSON | `--format json`: `mode` / `purge` / `remove_key` / `key_source_mode` / `targets[]`（kind / path / action / status / size_bytes / detail）/ `totals`（remove_count / remove_bytes）。`env` モードでは Keychain 値・API キーは一切現れない |
| C1-11 | ヘルプ | `uninstall --help` は ASCII のみ・禁止語 0・表示するフラグ（`--execute` / `--purge` / `--remove-key` / `--yes`）はすべて実際に機能する。トップレベル help と `--format` の対応コマンド列挙に現れる。`doctor`・README の語彙（dry run / Keychain / undo）と矛盾しない |

### C2. ZC-9: `watch` の表示文言統一

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C2-1 | 開始行（dry run） | Rich / 非 Rich とも `watch <roots> start (dry run (no files moved))`（Rust の `true`/`false` と `dry-run:` を廃止） |
| C2-2 | 開始行（execute） | `watch <roots> start (execute mode)`（Rich でもモードを明示） |
| C2-3 | 移動行（dry run） | `log_outcome` の WouldMove は `would move <path> -> <dest> (dry run (no files moved))` |
| C2-4 | 無影響 | `moved` / `review` / `no move` / `error` / `detected` / `ignored` 行は不変。`watch` の構造化出力は追加しない（`--format json` は watch では効かない既存説明を維持） |

### C3. ZC-10: README Quick Start 短縮

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C3-1 | 冒頭 | `## Quick Start` が `## How it works` より前にあり、`cargo install kaseifu` と `kaseifu classify` の 1 コマンドが冒頭で完結する |
| C3-2 | 情報保持 | 旧 `### Install` / `### Quick Start` の情報（ソースからのビルド・鍵入力・非TTY・`setup`・`doctor`・素の `kaseifu`）は削除せず移動 |
| C3-3 | アンカー | README 内リンク（`](#...)`）はすべて実在する見出しに解決する（新しいリンクも含めて検証） |
| C3-4 | 無影響 | 後段の全節（Usage / Configuration / Safety / Quality 等）は残り、P1 の README テスト（対象拡張子・Spotlight R6）も緑のまま |

### C4. 品質ゲート（全タスク共通）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C4-1 | テスト | 既存 428 + 新規が全緑。**テスト数を減らさない** |
| C4-2 | 複雑度 | 新規 CC16+ なし、既存最大 CC 15 を悪化させない |
| C4-3 | 依存 | 追加クレート・追加依存なし |
| C4-4 | 安全 | テストは隔離 HOME・一時ディレクトリ・注入経路（`KASEIFU_COMPLETION_DIR` / `MemoryStore` / 直接パス注入）のみ。実 Keychain・実ジャーナル・実補完ディレクトリ・`~/Desktop` を読み書きしない |

## 3. 設計判断（決定と理由）

### 3.1 ZC-11: コマンド形と実行モデル

- 新規ライブラリモジュール `src/uninstall.rs` に **plan（読み取りのみ）** と **execute（削除）** を分離する。`plan` は `SecretStore` を受け取らない = 構造的に Keychain へ触れ得ない（ドライランは読みもしない）。`execute` は注入 `SecretStore` + `keychain_allowed` + 確認クロージャを受け取る。
- 対象は原典 §7 の表をそのまま実装: 補完 `_kaseifu`（既定削除） / config・journal（`--purge` のみ） / Keychain（`--remove-key` または `--purge`、`--yes` で確認省略） / CWD の旧 `data/kaseifu.db`（`--execute --purge` のみ） / バイナリ（残す・`cargo uninstall kaseifu` 案内） / 移動済みファイル（対象外を表示）。実行順は表の順（補完 → 設定・ジャーナル → Keychain）。
- **Keychain は 2 重ゲート**: plan 段で `env` モードは action を強制的に Keep にし、execute 段でも `keychain_allowed=false` なら触らない。`users` には `skip` ではなく keep 行として「env モードでは触れない」を明示。
- 非対話の `--execute --remove-key`（`--yes` なし）は **削除前に** `stdin.is_terminal()` を検査して失敗させる（既存 prompt 群の「非TTYはハングせずエラー」原則。部分削除を作らない）。TTY での確認拒否は `cancelled` として継続。
- 旧ジャーナルは**読み取り専用 inspection が成功した場合のみ**削除対象にする（`inspect_journal` が読めないファイルは削除しない: 内容不明の履歴を壊さない）。削除は `--execute --purge` かつ CWD の `data/kaseifu.db` のみ。他ディレクトリは探索しない（案内文で手動削除を促す）。
- exit code は常に 0（失敗は `FAIL` 行と部分削除の明示で伝える。`doctor` と同じ「レポート型」に統一）。削除対象が構文上あっても失敗しても、報告は必ず出す。
- 表示は `doctor` と同系の `{word:<7} {Name}: {detail}` 行（`remove` / `removed` / `keep` / `none` / `FAIL`）を ASCII のみで出し、Rich では既存 `emit_report` のパネルに載せる。
- `main()` の完了自動更新の除外リストに `Uninstall` を追加（uninstall 実行中に補完を再生成しない。状態をそのまま報告する）。

### 3.2 ZC-9: 語彙の統一

- `generic::watch_line` は常に `(...)` でモードを付ける（dry: `dry run (no files moved)` / execute: `execute mode`）。Rich / 非 Rich の両方で同じ文字列を使い、`print_watch_start` の `dry-run: {}` 分岐を削除する。
- `runner::log_outcome` は出力文字列を組み立てる `pub fn outcome_text` に抽出し、WouldMove を `would move <path> -> <dest> (dry run (no files moved))` に統一（`watch_line` と同じ括弧スタイル）。文言をテストで固定できるようにする。
- watch の `--format json` 非対応の説明（help / README）は維持する。

### 3.3 ZC-10: README 再構成

- 冒頭（hero + 目的）直後に `## Quick Start` を置き、`cargo install kaseifu` → `kaseifu classify` の 1 コマンドを中心に、鍵・非TTY・`setup`・`doctor`・素の `kaseifu`・ソースからのビルドを集約する。
- `## Usage` の `### Install` / `### Quick Start` は削除（内容は上へ移動。情報は削除しない）。詳細節（Inspect 以降）は不変。
- 新しい節から詳細節へのアンカーリンク（`#usage` / `#configuration` / `#how-it-works`）を張り、テストで全 `](#...)` リンクの解決を固定する。

## 4. タスク一覧（実装順・直列厳守）

`src/main.rs`・`README.md`・`CHANGELOG.md` が衝突するため 1 項目ずつ完了・検証してから次へ進む。

| # | タスク | 主なファイル | 期待する振る舞い（受け入れ） | テスト |
|---|---|---|---|---|
| T1 | ZC-11: `uninstall.rs` + renderer + サブコマンド + 自動更新除外 + `--format` 説明 | `src/uninstall.rs`（新規）, `src/presentation/renderers/uninstall.rs`（新規）, `src/presentation/renderers/mod.rs`, `src/lib.rs`, `src/main.rs` | C1-1〜C1-11 | 新規 `tests/v080_zero_config_uninstall_test.rs` + 既存 help テスト自動走査 |
| T2 | ZC-9: watch 文言統一（`watch_line` / `print_watch_start` / `outcome_text`）+ 手動計画の更新 | `src/presentation/renderers/generic.rs`, `src/main.rs`, `src/watch/runner.rs`, `tests/user-test-plan.csv` | C2-1〜C2-4 | 新規 `tests/v080_zero_config_finish_test.rs`（library 直呼び）+ 実機ログ |
| T3 | ZC-10: README Quick Start 再構成 + CHANGELOG `[Unreleased]` | `README.md`, `CHANGELOG.md` | C3-1〜C3-4 | `tests/v080_zero_config_finish_test.rs`（冒頭・アンカー・情報保持） |
| T4 | 品質ゲート: fmt / clippy / test / `quality_check.sh` OVERALL PASS、検収（Plan Architect / Reviewer / QA issue） | `documents/plans/dev/issues/` | C4-1〜C4-4 | 実測 |

### 品質ゲート（C4）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C4-1 | テスト | 既存 428 + 新規が全緑。**テスト数を減らさない** |
| C4-2 | 複雑度 | 新規 CC16+ なし、既存最大 CC 15 を悪化させない |
| C4-3 | 依存 | 追加クレート・追加依存なし（テストは std + 既存 dev 依存 `serde_json` のみ） |
| C4-4 | 安全 | テストは隔離 HOME・一時ディレクトリ・注入経路のみ。実 Keychain・実ジャーナル・実補完ディレクトリを読み書きしない |

## 5. 受け入れ基準（実測で示す）

原典 §7 の受け入れ基準 1〜7 をそのまま満たす（ドライランで削除ゼロ / `--execute` で補完のみ / `--purge --execute` で設定・ジャーナル / `--remove-key --yes` / `env` モードの Keychain 不接触 / 冪等 / 移動済みファイルと他ディレクトリ旧ジャーナルの不可侵）。加えて:

1. `kaseifu uninstall` のドライラン / `--execute` / `--purge --execute` / `--remove-key --yes` / 冪等の各出力（テキストと JSON）が報告に含まれる。
2. `watch` の文言変更前後（開始行・移動行）と README 冒頭の変更前後が報告に含まれる。
3. `cargo fmt --check` / `cargo clippy -D warnings` / `cargo test --all-targets --all-features` / `bash scripts/quality_check.sh` OVERALL PASS。

## 6. リスクと対策

| リスク | 対策 |
|---|---|
| uninstall のテストが実環境の補完・設定・Keychain に触れる | 全 CLI テストで隔離 HOME + `KASEIFU_COMPLETION_DIR` + `KASEIFU_KEY_SOURCE=env` + `KASEIFU_NO_COMPLETION_UPDATE=1` + CWD フィクスチャを固定。Keychain 削除経路は `MemoryStore` / spy 注入の library テストのみ |
| uninstall 実行中に補完が自動再生成されて「削除したのに残る」 | main の自動更新除外に `Uninstall` を追加。案内文で「バイナリが残ると次回実行で再生成される」「`cargo uninstall kaseifu` で本体も消す」を明示 |
| 旧ジャーナルの誤削除 | 削除は CWD の `data/kaseifu.db` のみ・`--execute --purge` のみ・読み取り成功時のみ。他ディレクトリは探索しない |
| `--purge` による undo 履歴の喪失 | ドライラン表示の時点で `--purge deletes it and the undo history is lost` を出し、実行時も削除行に明示 |
| watch 文言変更で既存の手動テスト計画が陳腐化 | `tests/user-test-plan.csv` T55/T56 を新語彙へ更新（意図的差分として記録） |
| README のアンカー破壊 | `](#...)` リンクを全件解決するテストを追加。見出しの改名は既存 P1 テストの対象節に触れない範囲で行う |
| 新規コードの CC16+ | plan / execute / renderer を小さな関数に分割（対象ごとの entry 生成、行・JSON の生成を分離）。`quality_check.sh` の baseline-gate で実測 |

## 7. 設計判断の記録（Reviewer 確認対象）

1. **uninstall の exit code は常に 0**（失敗は `FAIL` 行 + 部分削除の明示で伝える。`doctor` と同じレポート型。原典は exit code を規定していない）。
2. **非TTY + `--remove-key`（`--yes` なし）は削除前にエラー**（partial state を作らない。`--yes` を案内）。
3. **旧ジャーナルは読めた場合のみ削除**（読めない履歴は保全。`--execute --purge` かつ CWD のみ）。
4. **Keychain 削除の確認は削除ステップの最後（Keychain 段）で行う**（順序要件どおり。拒否は `cancelled` で他は巻き戻さない）。
5. **`env` モードは plan 段で action=Keep に固定**（execute 段のゲートと 2 重。spy テストで 0 呼び出しを証明）。
6. **watch の非 Rich 開始行は Rich と同文字列**に統一（`format` は色だけに影響する）。
7. **`log_outcome` の文言は `outcome_text` へ抽出**（挙動不変、テスト可能化）。
8. **README の旧 `### Install` / `### Quick Start` 見出しは削除**し、内容は冒頭 `## Quick Start` へ移動（アンカーは `## Quick Start` に引き継がれる）。

## 8. 検収記録

| 工程 | 成果物 | 判定 |
|---|---|---|
| Plan Architect | `issues/20261016-plan-architect-zero-config-finish.md` | 承認（F1〜F8 を計画に反映済み） |
| Reviewer | `issues/20261016-reviewer-zero-config-finish.md` | APPROVE（blocking 0。MINOR/NIT は記録のみ） |
| QA | `issues/20261016-qa-zero-config-finish.md` | PASS（quality_check.sh OVERALL PASS / 450 passed / CC>=16 0 / 新規依存 0 / 隔離監査 OK） |

### 完了時の実測サマリ（2026-10-16）

- テスト: 既存 428 → **450 passed / 0 failed**（新規 22: uninstall 17 / finish 5）。
- `bash scripts/quality_check.sh`: **OVERALL: PASS**（rustfmt 0 / clippy 0 / test 450 / complexity 725 関数 / baseline-gate PASS / coverage 85.26% report-only）。
- ZC-11: ドライラン（1 target・64 B・削除 0）、`--execute`（補完のみ）、`--purge --execute`（4 対象 + 他ディレクトリ旧ジャーナル/移動済みファイル不可侵）、`env` の Keychain 不接触（spy 0 回 + 実表示）、冪等（`nothing to remove`）、非TTY `--remove-key` は削除前拒否。
- ZC-9: 変更前 `watching <path> (dry-run: true)` → 変更後 `watch <path> start (dry run (no files moved))` / `(execute mode)`。移動ログは `would move <path> -> <dest> (dry run (no files moved))`。
- ZC-10: README 冒頭に `## Quick Start`（インストール → 1 コマンド）。詳細は後段に移動のみで情報削除なし。`](#...)` リンク全件解決をテスト固定。
- 新規依存 0。P0/P1 の既存挙動・既存テストに回帰なし（`v052` の列挙リスト追加 1 件は意図的更新）。

## 9. 実測エビデンス（追記式）

### T1: ZC-11（2026-10-16）

- `cargo test --test v080_zero_config_uninstall_test` = **17 passed / 0 failed**
  （library 9: plan 読み取りのみ・report 行・execute 補完のみ・purge 確認拒否/承認・env spy 非接触・削除失敗の続行・冪等。CLI 8: ドライラン・execute・purge・env keychain 不接触・非TTY 拒否・冪等・JSON・副作用なし・help）
- CLI 実測（隔離 HOME + 一時 `KASEIFU_COMPLETION_DIR` + `env` モード + 一時 CWD。以下は実出力の抜粋）:

（a）ドライラン（text、exit 0、削除 0）:
```text
1 target(s), 64 B total (dry run; nothing was deleted)
remove  Completion: /tmp/kaseifu-uninstall-demo2/completion/_kaseifu (64 B; regenerated on the next run while the binary stays)
keep    Config: .../config.toml (27 B; --purge deletes it)
keep    Journal: .../kaseifu.db (12.0 KB; --purge deletes it; the undo history is lost)
keep    Legacy Journal: data/kaseifu.db (0 operation(s); --purge --execute deletes this one only; journals in other directories are never touched)
keep    Keychain: not touched in env mode (KASEIFU_KEY_SOURCE=env); nothing is read or deleted
keep    Binary: <path> is not removed here; run `cargo uninstall kaseifu`
keep    Moved files: kaseifu never touches files it moved; run `kaseifu undo` to bring them back
```

（c）`--execute`（補完のみ削除、config/journal/旧ジャーナル残存）:
```text
removed 1 target(s), 64 B total
removed Completion: /tmp/kaseifu-uninstall-demo2/completion/_kaseifu (64 B; the binary regenerates it on the next run; run `cargo uninstall kaseifu` to remove kaseifu itself)
keep    Config: ... (27 B; --purge deletes it)
keep    Journal: ... (12.0 KB; --purge deletes it; the undo history is lost)
keep    Legacy Journal: data/kaseifu.db (0 operation(s); --purge --execute deletes this one only; ...)
keep    Keychain: not touched in env mode (...)
...
```

（d）`--execute --purge --remove-key --yes`（env モード: Keychain 行は keep のまま）:
```text
removed 4 target(s), 24.1 KB total
removed Completion: ... | removed Config: ... | removed Journal: ... (the undo history is gone)
removed Legacy Journal: data/kaseifu.db (0 operation(s); deleted here; journals in other directories are never touched)
keep    Keychain: not touched in env mode (KASEIFU_KEY_SOURCE=env); nothing is read or deleted
```
状態確認: `other/data/kaseifu.db`（他ディレクトリ旧ジャーナル）= present、`MovedPhotos/photo.jpg`（移動済み相当）= present。

（e）同一コマンド 2 回目（冪等）: `nothing to remove (already clean)` + 全行 `none`/`keep`、exit 0。

（b）非TTY + `KASEIFU_KEY_SOURCE=keychain` + `--remove-key`（`--yes` なし、exit 1、削除前に停止）:
```text
Error: deleting the Keychain API key asks for confirmation, and that needs a terminal (stdin is not a TTY)
pass --yes to delete it without asking, or drop --remove-key/--purge to keep it
```

（h）ドライラン + `--purge --remove-key`（keychain モード、exit 0。dry run は Keychain を読まない）:
```text
2 target(s), 47 B total (dry run; nothing was deleted)
remove  Completion: ...
remove  Keychain: service com.routeflags.kaseifu.jev (asks for confirmation; --yes skips the prompt)
...
```

- JSON（dry run、抜粋）: `{"key_source_default":false,"key_source_mode":"env","mode":"dry_run","purge":false,"remove_key":false,"targets":[{"action":"remove","kind":"completion","path":".../_kaseifu","size_bytes":64,"status":"would_remove"}, {"action":"keep","kind":"keychain","service":"com.routeflags.kaseifu.jev","status":"kept"}, ...],"totals":{"remove_bytes":64,"remove_count":1}}`
- JSON（`--execute --purge`、抜粋）: `"mode":"execute"`、completion/config/journal が `"status":"removed"`、keychain は `"status":"kept"`（env）、`"totals":{"remove_bytes":12362,"remove_count":3}`。
- env モードの Keychain 非接触は library テスト（spy の get/delete 0 回 + confirm 未発火）と CLI 表示の両方で固定。

### T2: ZC-9

- `cargo test --test v080_zero_config_finish_test` = 5 passed（うち watch 3）。
- 変更前（ベースライン `d436451` の実バイナリを分離ビルドして実行）:
  ```text
  watching /tmp/kaseifu-watch-demo/inbox (dry-run: true)
  ```
  ベースラインコード: `src/main.rs:778` = `println!("watching {roots} (dry-run: {})", !actually_move);`、`src/watch/runner.rs:150` = `"dry-run {} (would move to {})"`。
- 変更後（worktree の実バイナリ）:
  ```text
  watch /tmp/kaseifu-watch-demo/inbox start (dry run (no files moved))
  watch /tmp/kaseifu-watch-demo/inbox start (execute mode)
  ```
  移動ログ: `would move <path> -> <destination> (dry run (no files moved))`（`outcome_text` テストで固定）。
- `tests/user-test-plan.csv` T55/T56 を新語彙に更新（意図的差分）。

### T3: ZC-10

- 変更前（README 冒頭は `## How it works`、インストールは 95 行目以降の `### Install` / `### Quick Start`）。
- 変更後（冒頭、実ファイル抜粋）:
  ```markdown
  ## Quick Start

  Install, then run one command:

  ```bash
  cargo install kaseifu
  kaseifu classify ~/Desktop/example.pdf
  ```

  `classify` decides where the file belongs and moves nothing. ...
  ```
- `cargo test` の `readme_quick_start_comes_first_and_covers_the_one_command_flow` と `readme_keeps_the_detail_sections_and_resolves_its_links` が冒頭性・情報保持・アンカー解決を固定。

### T4: 品質ゲート

- `bash scripts/quality_check.sh` = **OVERALL: PASS**（rustfmt 0 / clippy 0 / cargo-test 450 passed, 0 failed / complexity PASS / baseline-gate PASS / report-tests PASS / coverage line 85.26% report-only）。
- CC: 725 関数、CC<=10 = 721（**99.4%**）、CC 11-15 = 4、CC>=16 = **0**、最大 **15**（ベースライン不悪化）。新規 uninstall 系 37 関数の最大 CC 8。
- 新規テスト 22 件（428 → 450）。新規依存 0。

