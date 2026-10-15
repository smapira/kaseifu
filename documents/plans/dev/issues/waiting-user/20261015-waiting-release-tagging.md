# 残タスク: リリースタグ（Phase 6〜9 が未タグ）

- status: waiting-user
- milestone: M4 - Measurement and release hygiene
- waiting on: release/tag decision
- 出典: スレッドのリリース運用（v0.5.0〜v0.9.0 は annotated タグを `origin` へ push 済み）
- 優先度: 中（履歴は push 済みだが、区切りがないと「どの時点の動作か」の参照点がない）

## 現状（実測）

- 最新タグは `v0.9.0`（`440719c`）。
- その後 `origin/main` に 4 コミットが入っている（`4d94c40` Phase 6 動画 / `e6104eb` Phase 7 フォルダ / `ad5b0a4` Phase 8 一括 / `f519188` Phase 9 測定）で、いずれも未タグ。
- ローカルと `origin/main` は `f519188` で一致、作業ツリーは clean。

## やること

- 区切りを付けるか決める。付けるなら **annotated** で打ち、`origin` へ push する（例: `v0.10.0`、Phase 6〜9 をまとめる）。
- タグメッセージに「何が入ったか」を書く（動画・フォルダ・一括リネーム、Phase 9 の測定基盤）。
- 区切らない選択もありうる（その場合はこの issue を close する）。

## 受け入れ条件

- タグが `origin` に存在し、`git show <tag>` で説明が読める。または「打たない」と決めた記録がある。

## 制約

- force push もタグの付け替えもしない（打ったら動かさない）。
- リリース物（バイナリの配布・パッケージ公開）は別の判断（この issue には含めない）。

## 検証

- `git ls-remote --tags origin` と `git tag -n` で確認。
