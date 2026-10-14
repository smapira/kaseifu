# 残タスク: ZC-14 `--to <dir>`（明示した移動先を指定する）

- status: open
- 出典: `documents/plans/user/20261013d.md`（ゼロコンフィグ化 改善計画 v1.0）ZC-14
- 優先度: 中（非 TTY の案内文が「将来の `--to <dir>`」を名指ししており、未実装のまま利用者に見えている）

## 現状（実測、`f519188`）

- `src/main.rs` に `--to` は 0 件。`kaseifu rename --help` にも出ない。
- `organize --select` の非 TTY 案内が代替手段として `--format json` と将来の `--to <dir>` を挙げている（原典 §5）。

## やること

- `organize --to <dir>`（または `--select --to`）を実装し、対話なしで移動先を指定できるようにする。
- 移動先の検証は既存の除外規則（`discovery::destinations::is_excluded_destination`）と「存在するディレクトリであること」を再利用する。
- 自己の親ディレクトリなど no-op になる指定を拒否する。

## 受け入れ条件

- 非 TTY でも対話なしで移動できる。`--format json` と併用可能。
- 除外規則に落ちる移動先は理由付きで拒否される（上書きしない）。
- journal / undo は既存経路のまま動く。
- help に内部 Phase 語を出さない（`tests/v070_zero_config_p1_help_test.rs` が全 help を走査）。

## 制約

- 新しい設定ファイル・別 DB・独自キャッシュ・外部プロセスなし。CC 16 以上を作らない。
- テストは隔離 HOME と一時ディレクトリのみ。実ファイルを動かさない。

## 検証

- 実機: 隔離 HOME の一時ツリーで `--to` による移動 → `undo` 一巡。
- 非 TTY の実行でハングしないこと（`</dev/null`）。
