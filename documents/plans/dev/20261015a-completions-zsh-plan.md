# kaseifu zsh 補完の実装と配布（v0.4.3 予定 / BDD 計画）

- 起票: 2026-10-15
- 対象: `src/completion/`（新規）, `src/main.rs`, `src/doctor.rs`, `tests/completion_test.rs`（新規）,
  `README.md`, `documents/development.md`, `CHANGELOG.md`
- 制約: `.zshrc` 変更禁止 / Prezto 併用 / 新規依存は `clap_complete` のみ / commit・tag・push は親が実施

## 1. 背景と環境実測

| 項目 | 実測結果（2026-10-15） |
|---|---|
| `/opt/homebrew/share/zsh/site-functions` | 存在・ユーザー所有（`bookair18:admin`）・書込可・fpath に含まれる |
| `_kaseifu` | 未配置 |
| `/usr/local/share/zsh/site-functions` | 存在しない（fpath には載るが走査対象外） |
| `~/.zprezto/modules/completion/external/src` | 存在・fpath に含まれる |
| zsh | 5.9 (arm64-apple-darwin25.0) |
| clap / clap_complete | clap 4.6.7（lockfile） / clap_complete 4.6.11（clap ^4.6.6 要求・整合） |
| `~/.zshrc` 変更前ハッシュ | `bb535b0d…84ceae`（作業後に再確認し不変を証明する） |

### リサーチ結果（コードベース・上流の実測）

1. **clap_complete は隠しサブコマンドを出力する**（zsh/bash/fish/elvish の全 AOT シェルで実測）。
   `clap_builder` 4.6.7 には「`Command` からサブコマンドを除去する API」が存在しない
   （`subcommand()` 追加・`mut_subcommands()` は 1:1 変換のみ・`get_subcommands(_mut)` は参照のみ）。
   上流では clap-rs/clap#1335 として既知で、AOT ジェネレータは未対応（maintainer 回答: discussion #5214）。
   → **zsh スクリプト生成後に隠しコマンドの痕跡を除去するフィルタを自前で持つ**（他シェルは
   `clap_complete` の出力をそのまま流用し、zsh のみ除外を保証する。理由は §4）。
2. **compinit はファイルの 1 行目が `#compdef` であることを要求**（実測: 1 行目が `#compdef myfoo` なら
   `_comps[myfoo]` に登録、バージョン印が 1 行目だと登録されない）。
   → バージョン印は `#compdef kaseifu` の**次行**に挿入する。
3. `cargo test --all-targets --all-features` のベースラインは **279 passed / 0 failed**。
4. `tests/v33_config_auth_test.rs:483` が `report.checks.len() == 6` を固定しており、doctor に 1 行足すと
   この 1 箇所の更新が必要（テスト意図は「ネットワーク無しでローカルチェックが走る」で、件数は付随情報）。

## 2. 振る舞い（BDD シナリオ / 具体例）

### C1. `kaseifu completions zsh`（純粋・書込なし）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C1-1 | 正常系 | stdout に有効な zsh スクリプトを出力、終了コード 0、ファイル書込なし |
| C1-2 | 構文 | `zsh -n` でエラーなし。1 行目 `#compdef kaseifu`、2 行目 `# kaseifu completions v0.4.2` |
| C1-3 | 内容 | 全トップレベル（inspect/scan/organize/undo/classify/watch/setup/auth/config/doctor/completions）を含む |
| C1-4 | ネスト | `auth` の set-key/status/remove-key、`config` の show/set を含む |
| C1-5 | 隠し | `discover` の痕跡（case 腕・候補リスト・関数定義）が一切無い |
| C1-6 | 純粋性 | `KASEIFU_COMPLETION_DIR=<tmp>` を設定して実行しても 1 バイトも書き込まない（自動更新は completions では走らない） |
| C1-7 | 他シェル | `bash` / `fish` / `elvish` / `powershell` でも出力（除外フィルタは zsh のみ。README に明記） |
| C1-8 | 異常系 | 未対応シェル名は clap の usage エラー（終了コード 2） |

### C2. `kaseifu completions zsh --install`（冪等な配布）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C2-1 | 正常系 | 検出ディレクトリに `_kaseifu` を作成し、書込先パスを 1 行で表示（`Installed zsh completions: <path>`） |
| C2-2 | 冪等 | 再実行時は内容同一なら書き換えず `unchanged` と表示（mtime 不変） |
| C2-3 | 明示指定 | `--dir <PATH>` を最優先。存在しない/書込不可なら**エラー終了（1）**し書込なし |
| C2-4 | 環境変数 | `KASEIFU_COMPLETION_DIR` は `--dir` の次に優先。設定されている場合は**フォールバックしない** |
| C2-5 | 検出順 | homebrew → `/usr/local/share/zsh/site-functions` → zprezto → `${XDG_DATA_HOME:-~/.local/share}/zsh/site-functions` → `~/.zsh/completions`。最初の「存在 ∧ 書込可」を採用 |
| C2-6 | 異常系 | 書込可能な候補が無ければエラー終了（1）。`--install` は zsh のみ（他シェルはエラー） |
| C2-7 | JSON | `--format json` では `{"shell":"zsh","path":"…","status":"installed"}`（秘密情報なし） |

### C3. 実行時自動更新（要件 5）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C3-1 | ファイルなし | 検出ディレクトリに生成・設置（ベストエフォート） |
| C3-2 | 印が古い | マーカー行が現行バージョンと不一致なら再生成（例: `v0.0.1` に改竄 → 実行後に `v0.4.2` へ戻る） |
| C3-3 | 最新 | マーカー一致なら何もしない（内容比較のうえで書換なし） |
| C3-4 | 無効化 | `KASEIFU_NO_COMPLETION_UPDATE` が非空なら一切触らない。`KASEIFU_COMPLETION_DIR` が空文字なら未設定扱い |
| C3-5 | 失敗時 | 候補なし・書込不可・rename 失敗などは**すべて無視**。終了コード・stdout・stderr を変えない（stderr は `KASEIFU_DEBUG=1` のときだけ `[debug] …`） |
| C3-6 | JSON 不変 | `inspect`/`classify`/`discover` の `--format json` はバイト単位で不変 |
| C3-7 | 原子性 | 同一ディレクトリ内のドット付き一時ファイル + `rename`。検出ディレクトリの外へは書かない |
| C3-8 | 除外 | `completions`（純粋性）と `doctor`（診断は実状態を映す）では自動更新を走らせない |

### C4. `doctor` の補完状態（1 行追加）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C4-1 | installed | `ok Completion: zsh completions installed (<path>)` |
| C4-2 | stale | `FAIL Completion: zsh completions stale (<dir>; run kaseifu completions zsh --install)` |
| C4-3 | missing | `FAIL Completion: zsh completions missing (run kaseifu completions zsh --install)` |
| C4-4 | JSON | `checks[]` に 1 要素追加（既存キーは不変） |
| C4-5 | 診断副作用なし | doctor 自身は書込を起こさない |

### C5. 品質ゲート

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C5-1 | テスト | 既存 279 + 新規が全緑（`assert_eq!(checks.len(), 6)` のみ 7 へ更新） |
| C5-2 | 複雑度 | 新規 CC ≥ 16 を作らない |
| C5-3 | 依存 | 追加は `clap_complete` のみ（`Cargo.toml` + `Cargo.lock`） |
| C5-4 | 品質 | `bash scripts/quality_check.sh` が OVERALL PASS |
| C5-5 | 無干渉 | `.zshrc` 不変（ハッシュ照合）、`discover` はデバッグ専用のまま、新規ファイルは `git add` 済み |

## 3. タスク一覧（実装順）

| # | タスク | 主なファイル | 期待する振る舞い（受け入れ） | テスト |
|---|---|---|---|---|
| T1 | `clap_complete` 導入（`Shell` のみ・features 既定） | `Cargo.toml`, `Cargo.lock` | ビルド可、`cargo tree` に追加依存なし | `cargo build` |
| T2 | 隠しコマンド痕跡のフィルタ（case 腕/候補リスト/関数定義の 3 規則、入れ子 depth 追跡） | `src/completion/zsh.rs` | 合成 `Command`（隠し葉・隠し親+子・ネスト隠し）で痕跡ゼロ・可視命令は不変 | 単体（合成木） |
| T3 | マーカー挿入（`#compdef` 次行）と `render(shell, cmd)` | `src/completion/mod.rs` | C1-1〜C1-5 | 単体 + `zsh -n` |
| T4 | 候補列挙（純関数）と検出（存在 ∧ 書込可） | `src/completion/mod.rs` | C2-3〜C2-6 | tmp ディレクトリで検証 |
| T5 | `write_if_changed`（バイト比較 + 一時ファイル + rename） | `src/completion/mod.rs` | C2-1, C2-2, C3-7 | tmp ディレクトリで検証 |
| T6 | `refresh_best_effort`（無効化 env・失敗の完全無視・debug のみ stderr） | `src/completion/mod.rs` | C3-1〜C3-8 | 注入 API + 実バイナリ起動（後述） |
| T7 | `completions` サブコマンド（`--install`/`--dir`、出力整形） | `src/main.rs` | C1, C2 | 統合（実バイナリ） |
| T8 | `main()` へ自動更新フック（completions/doctor を除外） | `src/main.rs` | C3-8 | 統合（実バイナリ） |
| T9 | `doctor` に 1 行追加 + 既存件数アサーション更新 | `src/doctor.rs`, `src/main.rs`, `tests/v33_config_auth_test.rs` | C4 | 統合 |
| T10 | ドキュメント（README/development.md/CHANGELOG） | 3 ファイル | C1-7・検出順・無効化 env・doctor の読み方を記載 | 目視 + 品質ゲート |
| T11 | 品質ゲート実行と証跡化 | `reports/quality/`（生成物は git 管理外） | C5-1〜C5-4 | `bash scripts/quality_check.sh` |

### 「失敗しても本体を妨げない」の具体化（T6/T8）

- 呼び出しは `let _ = completion::refresh(...)` 形式（戻り値は捨てる）。内部の全エラーは `debug!` のみ。
- フックは `Cli::parse()` 成功後・dispatch 前。`--version`/`--help` は clap が先に終了するため対象外。
- 実バイナリ統合テスト（`env!("CARGO_BIN_EXE_kaseifu")` + `KASEIFU_COMPLETION_DIR=<tmp>` + `inspect <存在しないパス>`）:
  - 印を `v0.0.1` に改竄 → 実行後に `v0.4.2` へ再生成され、終了コード/stdout/stderr は改竄前と同一。
  - `KASEIFU_NO_COMPLETION_UPDATE=1` → 改竄されたまま。
  - 書込不可ディレクトリ（パーミッション 0555 の tmp）→ 無変化・出力不変。

## 4. 設計判断と代替案

| 論点 | 採用 | 理由 / 却下した案 |
|---|---|---|
| 隠しコマンドの除外 | zsh 生成物の後処理フィルタ（3 規則） | clap に除去 API が無く（上流 #1335）、`Command` 木の再構築は設定コピー漏れで可視部分の出力が変わる。フィルタは上流修正後は自然に no-op になり削除可能 |
| 他シェル | `clap_complete` の出力をそのまま（zsh のみ除外保証） | 各シェルの痕跡形式は別物（bash: `opts=`/case 腕、fish: `complete -c`、elvish: `cand`）で、除外を全シェル実装する価値は要件（zsh 必須）に対して過大 |
| `discover` を実行時注入する案 | 却下 | 完了条件は満たせるが、CLI 定義の単一情報源が 2 箇所に割れ、parse 経路も変わる。補完都合で CLI 構造を変えるリスクを避ける |
| 自動更新の場所 | `main()` の dispatch 前 1 箇所 | 各コマンドへ分散させると漏れが生じる。`completions`/`doctor` のみ明示的に除外 |
| 印の位置 | `#compdef` の次行 | compinit は 1 行目 `#compdef` 必須（実測）。行頭マーカーは grep 可能 |
| 書込先の決定 | `--dir` > env > 既定候補（存在 ∧ 書込可） | env 指定時はフォールバックしない（ユーザーの明示意思を尊重し、AC3 の「書込不可なら無変化」を保証） |

## 5. 検証（受け入れ基準 → 証跡）

| AC | 証跡 |
|---|---|
| 1 | `kaseifu completions zsh > /tmp/_kaseifu && zsh -n /tmp/_kaseifu` |
| 2 | 実機で `/opt/homebrew/share/zsh/site-functions/_kaseifu` を作成 → 再実行で mtime/内容不変 |
| 3 | マーカー改竄 → 実行 → 再生成を `grep` で確認。書込不可ケースは終了コード/stdout/stderr の差分ゼロ |
| 4 | `grep -c develop` ではなく全コマンド名の出現を assertion（`discover` は 0 件） |
| 5 | `kaseifu doctor` の出力 1 行 + `--format json` の checks |
| 6 | `zsh -f` + `fpath` 明示 + `compinit -u -D` で `_comps[kaseifu]` 登録を確認 |
| 7 | `bash scripts/quality_check.sh`（OVERALL PASS）、テスト全緑 |
| 8 | `shasum -a 256 ~/.zshrc` が作業前後で一致 |

## 7. Plan Architect レビュー反映（`issues/20261015-plan-architect-completions-review.md`）

| # | 指摘 | 反映 |
|---|---|---|
| A1 | 🔴 テストの env 汚染 | コアを注入 API（`RefreshPlan { enabled, dir, script }`）に分離。テストは注入 API + 実バイナリの子プロセス env の 2 層（T6/T9） |
| A2 | 🟡 ホーム / XDG | ホームは `dirs::home_dir()`、XDG は `XDG_DATA_HOME` を直接読む（`dirs::data_dir()` は macOS で `~/Library/Application Support` を返すため不可）（T4） |
| A3 | 🟡 原子書込 | `src/config/loader.rs:197-227` の規約を踏襲（`._kaseifu.tmp-<pid>` + rename）。共有化はしない（T5） |
| A4 | 🟡 doctor 行追加 | `check_storage()` の次に `check_completion()` を追加し、`tests/v33_config_auth_test.rs:483` の件数のみ 7 へ更新（T9） |
| A5 | 🟡 フィルタ CC | `strip_case_arm` / `strip_entry` / `strip_function` / `case_depth` に分割、メインループは早期 continue（T2） |
| A6 | 🟡 出力整形 | `emit_report` を使う。JSON は `{"shell","path","status"}` に固定。非 `--install` はスクリプトそのもの（T7） |
| A7 | 🟢 根拠と削除手順 | モジュール doc に clap-rs/clap#1335 と削除条件を記載。テストで痕跡ゼロを固定（T2/T3） |

## 6. リスク

| リスク | 影響 | 緩和 |
|---|---|---|
| clap_complete の出力形式変更でフィルタが無効化 | zsh 補完に `discover` が再出現 | テストが「痕跡ゼロ」を固定（無音で壊れない）。依存は lockfile で固定 |
| 自動更新がユーザー領域へ書く | 予期せぬ書込 | 検出ディレクトリ内のみ・ドット一時ファイル・rename・失敗は完全無視・無効化 env |
| テストが実 Homebrew ディレクトリへ書く | 環境汚染 | テストは常に `KASEIFU_COMPLETION_DIR=<tmp>` か注入 API を使用（実パスへの書込テストは禁止） |
| doctor の件数固定テスト | 既存テスト赤 | `v33_doctor_local_checks_without_network` の件数のみ 7 に更新（意図は不変） |
