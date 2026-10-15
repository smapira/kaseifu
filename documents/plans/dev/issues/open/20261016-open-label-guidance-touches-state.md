# 残タスク: `label` のガイダンス表示が journal DB を作る

- status: open
- milestone: M1 - Reliability and CLI clarity
- next owner: implementer
- blocker: actionable
- 出典: Homebrew Formula の test block 検証（tap 側 Issue #1 のコメント、2026-10-16）
- 優先度: 低（実害は「何もしないコマンドが `~/Library/Application Support/kaseifu/` に空の DB を作る」こと）

## 現状（実測、v0.12.1）

- `kaseifu label`（ターゲットも管理フラグも無し、ガイダンスを出すだけ）を隔離 HOME で実行すると、`Library/Application Support/kaseifu/kaseifu.db` が**作られる**（実測: `find` で 1 ファイル）。
- 原因: `handle_rename` が先頭で `RenameStore::open(journal_path())` を呼び、その後で「ターゲット無しなら guidance」を返す順序になっている。`RenameStore::open` は SQLite DB と親ディレクトリを作る。
- 対照: トップレベルの `kaseifu`（サブコマンド無し）は何も作らない（既存の ZC-2 の設計どおり）。
- 影響: 利用者にとっては「使い方を見ただけ」で状態ファイルが生まれる。`brew test` では Homebrew が HOME を staging に隔離するため実害は無い（Formula 側にもその旨コメント済み）。

## やること

- ガイダンス経路（ターゲットも管理フラグも無い場合）では store を開かない。store が必要なのは `--patterns` / `--list-presets` / `--show-default` / 保存系と、実際の計画・実行。
- 順序を入れ替えるだけで済むか、管理フラグの判定を先に出す必要があるかを確認して最小の変更にする。
- 副作用の有無をテストで固定する（隔離 HOME で `label` 実行後にファイルが増えないこと）。

## 受け入れ条件

- `kaseifu label`（引数なし）が隔離 HOME にファイルを作らない。
- `label --patterns` / `--list-presets` / `--show-default` / 保存系は従来どおり動く（store が必要な経路は開いたまま）。
- 既存テストが無変更で緑（挙動の変更は「作らない」方向のみ）。

## 制約

- CC 16 以上を作らない。新規 crate 依存なし。
- テストは隔離 HOME と一時ディレクトリのみ。
