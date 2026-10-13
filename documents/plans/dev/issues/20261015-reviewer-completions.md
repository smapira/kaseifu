# zsh 補完実装の検収レビュー（Reviewer）

- 日付: 2026-10-15
- 対象: 計画 `documents/plans/dev/20261015a-completions-zsh-plan.md`
- 変更: `src/completion/{mod.rs,zsh.rs}`（新規）、`src/main.rs`、`src/doctor.rs`、`tests/completion_test.rs`（新規）、`Cargo.toml/lock`、`README.md`、`documents/development.md`、`CHANGELOG.md`、`tests/v33_config_auth_test.rs`（1 行）
- 検証環境: macOS / zsh 5.9 / clap 4.6.7 / clap_complete 4.6.11 / 実 Homebrew site-functions

## 1. 仕様トレーサビリティ

| 要件 | 実装 | 検証 |
|---|---|---|
| 要件 1 clap_complete 導入 | `Cargo.toml`（`clap_complete = "4"`） | `cargo tree` 差分は 1 crate のみ |
| 要件 2 `completions zsh` | `handle_completions` → `render` | stdout 24,306 bytes / `zsh -n` OK |
| 要件 3 インストール機能 | `install_dir` + `install`（冪等） | 実機 `/opt/homebrew/share/zsh/site-functions/_kaseifu` 作成・再実行 mtime 不変 |
| 要件 4 `.zshrc` 不変 | 変更なし | `shasum -a 256 ~/.zshrc` が作業前後で同一 |
| 要件 5 自動生成・更新 | `refresh_if_stale`（dispatch 前） | 印を `v0.0.1` に改竄 → 実行後 `v0.4.2` に再生成（テスト + 手動） |
| 要件 6 失敗しても本体に影響なし | 全エラーを `debug!` のみ | 書込不可・欠損で exit/stdout/stderr 差分ゼロ（テスト） |
| 要件 7 ネスト補完 | 生成スクリプトに `auth`/`config` の全サブコマンド | 実 zsh の候補一覧で `set-key/status/remove-key` を確認 |
| 要件 8 環境に応じた検出 | `candidates()` + `detect()` | 検出順の純関数テスト + 実機（Homebrew 優先） |

## 2. 敵対的レビューで見つけた問題と対応

### R1. 🔴 バージョン印の位置（実装中に検出・修正済み）

- **壊し方**: 生成スクリプトの 1 行目に印を置くと compinit がファイルを登録しない（実測: 1 行目が印の場合は `_comps[kaseifu]` に現れない）。
- **対応**: 印は `#compdef kaseifu` の次行に挿入（`zsh::with_marker`）。`completion_installed_file_is_registered_by_compinit` テストが退行を検知する。

### R2. 🟡 ユーザー管理の symlink を自動更新が置換しうる（検出・修正済み）

- **壊し方**: `_kaseifu` を dotfiles へ symlink しているユーザーに、自動更新が `rename` で通常ファイルを被せてリンクを壊す。
- **対応**: `refresh_dir` は対象が symlink なら何もしない（`--install` は明示操作として上書き）。テスト `completion_refresh_never_replaces_a_user_managed_symlink` を追加。

### R3. 🟡 他シェルの出力には隠しコマンドが含まれる

- **指摘**: 上流 clap_complete は全 AOT シェルで `hide = true` を描画する（clap-rs/clap#1335、`clap_builder` 4.6.7 に除去 API なしをソースで確認）。除外フィルタは zsh のみ。
- **判断**: zsh は必須かつ唯一導入対象であり、要件の受け入れ基準も zsh スクリプトを指す。README とモジュール doc に明記済み（「そのまま出力」と分かる形）。
- **残余リスク**: bash/fish ユーザーが `completions bash` を自作導入すると `discover` が候補に出る（デバッグ専用コマンドであり実害は小さい）。

### R4. 🟡 フィルタが clap_complete の出力形式に依存

- **壊し方**: 上流が出力形式を変えると痕跡が残る可能性。
- **対応**: テストが (a) 痕跡ゼロ、(b) `--help` の全コマンドが候補に存在、(c) `zsh -n`、(d) compinit 登録、(e) 実 zsh の候補一覧を固定するため、無音では壊れない。上流修正時はフィルタが no-op になるだけで削除可能（モジュール doc に記載）。

### R5. 🟢 明示指定時のフォールバックなし

- `--dir` / `KASEIFU_COMPLETION_DIR` が使えない場合はエラー（exit 1）で停止し、無言で別ディレクトリに書かないことを実測（存在しないディレクトリ・ファイル・書込不可の 3 ケース）。

### R6. 🟢 秘密情報と表示規約

- 生成スクリプトは環境変数に依存せず、`JEV_API_KEY`/`JEV_API_URL` を設定してもバイト同一（実測）。秘密値・URL の痕跡なし。ASCII のみ・ANSI エスケープなし（`--format json` / `NO_COLOR` / 非 TTY で同一出力）。

### R7. 🟢 並行実行

- 10 並列の自動更新で `_kaseifu` は 1 つ・一時ファイル残留なし・最終ファイルは `zsh -n` OK（pid 付き一時ファイル + rename）。

### R8. 🟢 `discover` の扱い

- `discover` は従来どおり動作（JSON 出力も健在）し、`--help` にも補完にも出ない（実 zsh の `kaseifu <TAB>` 一覧に `discover` なしを確認）。

## 3. テストレビュー

- 新規 18 件はすべて振る舞いを検証（実装詳細のスナップショット比較ではない）。異常系（欠損・書込不可・symlink・無効化 env・非 zsh install）と正常系を網羅。
- 実バイナリ起動テストは必ず `KASEIFU_COMPLETION_DIR=<tmp>` を渡すため、実ユーザーの site-functions を書き換えない。
- `tests/v33_config_auth_test.rs` の `checks.len()` を 6 → 7 に更新した。これは doctor のチェック追加に伴う意図的な更新で、テストの意図（ネットワーク無しでローカルチェックが走る）は不変。

## 4. 結論

**承認**。受け入れ基準 1-8 をすべて実測で確認。R1・R2 は修正済み、R3 は文書化して受容、R4 はテストで固定している。
