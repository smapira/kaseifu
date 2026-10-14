# 残タスク: `undo` / `scan` / `watch` の `--format json`

- status: open
- 出典: `documents/plans/dev/20261014d-debug-scenario-design.md` R9（および F-07）、`20261014d-cli-scenarios-C01-C12.md` C07
- 優先度: 中（自動化・CI からの構造化取得ができない。スクリプトが人間向け文言に依存してしまう）

## 現状（実測、`f519188`）

- `kaseifu undo --format json` → `nothing left to undo`（人間向けテキスト。JSON ではない）。
- `kaseifu scan <path> --format json` → `Scanning: <path>`（同上）。
- `watch` は `--format` フラグを受けるが、JSON 経路の有無は未確認（常駐コマンドなので検証が要る）。
- **この制限はすでに `kaseifu --help` に明記されている**（実測）: 「`json` applies to inspect, classify, organize, doctor, setup, auth, config, completions --install and uninstall. `scan`, `undo` and `watch` always render text.」。**黙って無視されるわけではない**ので、これは不具合ではなく「自動化向けの拡張候補」である。
- `scan` はそもそもバナー出力のみ（`--help` にも「scanning is not implemented yet」と出る）なので、JSON 化の前に「scan が何をするコマンドなのか」の整理が必要（W2〜W5 の S21/S26/S27 と同席）。
- 一方で `organize` / `rename` / `discover` / `classify` / `inspect` / `doctor` は JSON を出す（既存の契約: stdout = 人間向けテキスト（JSON 対応コマンドのみ JSON）、stderr = 診断）。

## やること

- `undo` / `scan` / `watch` の JSON 出力を **追加のみ** で定義する（既存キーを削らない・型を変えない）。
- `watch` はイベント行を JSON Lines にするのか、開始・終了で包むのかを決めて記録する。
- C07 の契約（stdout/stderr の分離）を README の該当箇所に反映する。

## 受け入れ条件

- 3 コマンドで `--format json` が単一の（watch は行単位の）パース可能な出力を出す。
- 非 TTY でも同じ。ハングしない。
- 既存のテキスト出力は不変。

## 制約

- 出力は ASCII。新規コードに em dash を書かない。CC 16 以上を作らない。
- テストは隔離 HOME と一時ディレクトリのみ。

## 検証

- 各コマンドで stdout を `json.loads` できることのテスト + 既存キーの不変テスト。
