# zsh 補完計画の Plan Architect レビュー

- 日付: 2026-10-15
- 対象: `documents/plans/dev/20261015a-completions-zsh-plan.md`
- 関連: `src/main.rs`, `src/doctor.rs`, `src/config/loader.rs`, `src/config/storage.rs`, `tests/v33_config_auth_test.rs`
- 検証方法: `grep`/読解で既存規約を確認（`serena`/`context7` は本環境で未使用、`clap_builder`/`clap_complete` は vendored source を直接読解）

## 総評

計画は要件と既存アーキテクチャに整合し、実装可能な粒度に分解されている。
以下の 8 件を反映すれば実装着手を承認する。

## 指摘事項

### A1. 🔴 テストのプロセス環境変数汚染

- **指摘**: `refresh_best_effort` が直接 `std::env::var` を読む設計のままだと、並列実行されるテストが
  プロセスグローバル env を書き換える必要が生じる（既存 v3.3 の規約「no process-environment mutation」に違反）。
- **改善案**: コアを注入 API（`RefreshPlan { enabled, dir, script }` 相当）に分離し、env 読み取りは
  薄いラッパ（`from_env()`）だけに閉じる。テストは (a) 注入 API + tmp ディレクトリ、(b) 実バイナリを
  `std::process::Command::env()`（子プロセスのみ）で起動する絶対パス無し経路、の 2 層で検証する。

### A2. 🟡 ホーム / XDG の解決を既存規約に合わせる

- **指摘**: 候補列に `~/.zprezto/...` と `${XDG_DATA_HOME:-~/.local/share}/zsh/site-functions` がある。
  `dirs::data_dir()` は macOS で `~/Library/Application Support` を返すため流用不可。
- **改善案**: ホームは `dirs::home_dir()`（`src/config/storage.rs:32` と同じ）、XDG は
  `XDG_DATA_HOME` を直接読み、未設定時のみ `dirs::home_dir()/.local/share` を使う。

### A3. 🟡 原子書込の既存パターンを踏襲（新規抽象化はしない）

- **指摘**: `src/config/loader.rs:197-227` に「ドット付き sibling temp + `rename`」の確立した実装がある。
- **改善案**: completion 側も同一規約（`._kaseifu.tmp-<pid>` + rename、失敗時は temp を残すより
  ベストエフォート削除）で実装し、モジュールコメントで参照する。config と completion の共有化は
  無関係なリファクタリングになるため**今回はしない**（AGENTS.md: minimal diffs）。
  ドット接頭辞は compinit の fpath 走査（`*` は dotfile を拾わない）から見えないため必須。

### A4. 🟡 doctor の行追加手順の明文化

- **指摘**: `tests/v33_config_auth_test.rs:483` の `assert_eq!(report.checks.len(), 6)` が赤になる。
  放置すると「既存テスト全緑」の受け入れ条件を満たせない。
- **改善案**: `check_completion()` を `check_storage()` の次に追加し、同時に上記アサーションを 7 に更新
  （テスト意図「ネットワーク無しでローカルチェックが走る」は不変）。更新した事実を完了報告に明記する。
  doctor は診断なので自動更新を走らせない（実状態を映す）ことも計画どおり。

### A5. 🟡 フィルタの CC と構造

- **指摘**: 3 規則（case 腕 / 候補リスト / 関数定義）を 1 関数に詰めると複雑度が上がる。新規 CC ≥ 16 は
  品質ゲートで失格。
- **改善案**: `strip_case_arm` / `strip_entry` / `strip_function` / `case_depth` に分割し、
  メインループは早期 continue のみにする。各規則に合成 `Command` 木の単体テストを付ける。

### A6. 🟡 出力整形は既存の `emit_report` に揃える

- **指摘**: `completions zsh --install` の表示が既存コマンドと異なる形式になると一貫性が崩れる。
- **改善案**: `emit_report(format, "KASEIFU / COMPLETIONS", &lines, &json)` を使い、
  JSON は `{"shell":"zsh","path":"…","status":"installed"|"unchanged"}` に固定。
  非 `--install` の stdout はスクリプトそのもの（`--format` は適用しない）ことを README に明記。

### A7. 🟢 隠しコマンド除外の根拠と将来の削除手順をコードに残す

- **指摘**: フィルタは上流のバグ（clap-rs/clap#1335）への対処であり、将来不要になる。
- **改善案**: モジュール doc に「上流が修正したらフィルタは no-op になるため削除できる」ことを記載し、
  テスト（痕跡ゼロ + 可視コマンド全部あり + `zsh -n`）で挙動を固定する。

### A8. 🟡 lib API に `clap_complete::Shell` を出す点は承認

- **指摘**: `pub fn render(shell: clap_complete::Shell, …)` は lib の公開面に clap_complete を出す。
- **判断**: `clap` は既に lib 依存（`presentation/format.rs` が `ValueEnum` を使用）であり問題なし。
  ただし `Shell` を再エクスポートはしない（依存の露出を最小化）。

## 承認条件

A1〜A7 を計画書へ反映した時点で実装着手を可とする（再レビュー不要）。
