# 残タスク: `is_noise_file` の部分一致による過剰除外

- status: open
- 出典: 本スレッドの Phase 7/8 レビューで検出（`20261015-reviewer-ai-rename-p7.md` の「`is_noise_file` の `ends_with` 過剰一致はテストで固定」の記述）
- 優先度: 中（利用者の普通のフォルダが一括リネームの対象から黙って消える）

## 現状（実測、`f519188`）

- `src/normalize/noise.rs::is_noise_file` は `lower == n || lower.ends_with(n)` で判定する。`n` は `ds_store` / `.localized` / `node_modules` / `target` / `thumbs.db`。
- 実測: `/tmp/issue-work` に `my-target/`（中に `c.txt`）を置いて `kaseifu rename /tmp/issue-work --recursive` を実行 → **`my-target` が項目に出ない**（黙って除外）。
- 同じ関数はフォルダの内容読み（`context::directory::read_directory_facts`）でも使われるため、**フォルダ命名の証拠からも消える**。
- ファイル名の `F:` 信号にも影響しうる（`normalize` 経由かは要確認）。

## やること

- ノイズ判定を「ディレクトリ名の完全一致（大小無視）」に限定し、`ends_with` をやめる。
- `target` を除外したいのはビルド出力なので、**中身がビルド出力のとき**（またはパス上の祖先が既知のツリーのとき）だけにする。
- 除外したものを一括処理で**なぜ除外したかを報告**できるようにするか検討する（黙って消さない）。

## 受け入れ条件

- `my-target/`、`dist-target/`、`my-ds_store-backup/` などの利用者名が普通に扱われる。
- `target/`（完全一致）は従来どおり除外される。
- 既存の `normalize` / `discovery` のテストが無変更で通過するか、変更理由が記録されている。

## 制約

- CC 16 以上を作らない。

## 検証

- 単体テストで境界（完全一致は除外、部分一致は除外しない）を固定。
- 実機: `my-target/` を含むツリーで一括のプレビューに現れること。
