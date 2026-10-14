# 残タスク: 本家チェックアウトの更新（`~/OS/home/Codes/github.com/smapira/kaseifu`）

- status: open（**ユーザーの作業**）
- 出典: 本スレッドで確認した事実
- 優先度: 低（開発は Delta の worktree で行えており、実害はない）

## 現状

- `~/OS/home/Codes/github.com/smapira/kaseifu` は古いまま（v0.5.0 相当と確認済み）。
- 以降の変更はすべて `origin` に push 済み（HEAD `f519188`）。

## やること

- その checkout を `git pull` して揃える（または、そこで作業しないなら放置する）。
- 未コミットの変更があれば、先に退避するか確認する。

## 受け入れ条件

- その checkout が `origin/main` と一致している、または一致させる必要がないと決めている。

## 制約

- Delta worktree の外にあるため、**私は直接編集しない**。pull はユーザーが行うか、指示があれば代行する。
- 先に `git status` を見て、未コミットの変更を上書きしない。

## 検証

- `git -C <path> log --oneline -1` と `git -C <path> status --short` を確認。
