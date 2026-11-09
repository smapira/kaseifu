# 残タスク: 直下がディレクトリだけのとき `cluster` が理由も件数も出さずに失敗する

- status: open
- milestone: M3 - Workflow semantics
- next owner: implementer
- blocker: actionable
- 出典: シナリオA実測（`/Users/bookair18/Desktop/stash`、2026-11-09）。オーナー判断により今回のリリース必須修正にはしない
- 優先度: 低（実データを変更せず、失敗は安全側）

## 現状

`cluster <dir>` は「直下に regular file がない」と即エラー終了する。

```text
Error: no files to cluster directly under /Users/bookair18/Desktop/stash
（exit 1）
```

このとき **`directories left alone` も `skipped` も出ない**。
そのため「整理対象のファイルは無いか、あっても `.DS_Store` だけだったのか」「下位にどれだけフォルダがあるか」が利用者に伝わらない。

シナリオAで実際に踏んだケース:

- `stash` 直下: 28 ディレクトリ + `.DS_Store` 1 件 → エラー、28 件は報告されない。
- `github-issue` / `skill` / `WIP_searpbear`: 直下のファイルは `.DS_Store` のみ → スキップ後の 0 件で同じエラー（`.DS_Store` が skip されたことも見えない）。
- `code` / `sound` / `url`: 直下が空 → 同じエラー。

## やること（候補）

- 「ファイル 0 件」をエラーではなく成功として扱い、既存の要約（`directories left alone:` / `skipped:` / `ungrouped:`）を出す。
- またはエラーのままでも、少なくとも「directories: 28, skipped: 1 (.DS_Store)」を stderr に添える。
- 終了コードの意味を決める（何もしなかった = 0 か、入力不正 = 1 か）。`--format json` も同じ形で揃える。
- `.DS_Store` だけのディレクトリは「空」と同じ扱いで良いかを確認する。

## 受け入れ条件

- 直下がディレクトリのみ / `.DS_Store` のみ / 空の 3 ケースで、件数と理由が読める出力になり、終了コードが意図どおり。
- 実際にファイルがあるディレクトリの現行出力が変わらない（既存テストはそのまま通る）。

## 制約

- リリース必須にはしない（オーナー判断 2026-11-09）。実データ無変更、検証は `target/` 配下のサンドボックスで行う。
