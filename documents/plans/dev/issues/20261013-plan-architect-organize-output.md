# Plan Architect レビュー: organize 出力改善 4件

## 優先度
🔴 高（2件） / 🟡 中（4件） / 🟢 低（2件）

## 対象
- 計画書: `documents/plans/dev/20261013d-organize-output-plan.md`
- 関連ファイル: `src/main.rs:283-361`、`src/operations/organize.rs`、`src/presentation/renderers/generic.rs`、`src/watch/runner.rs`、`tests/user-test-plan.csv`、`scripts/complexity_report.py`

## 検証（コード・実測に基づく）

- `read_file: src/main.rs:283-362` — 表示は `handle_organize` + `print_organize_outcome` に閉じており、ライブラリ renderer へ移す方針でテスト可能になる（main.rs の表示は lib テストから触れないため）
- `grep: Refused` + `git log -S Refused -- src/operations/organize.rs` — **`OrganizeOutcome::Refused` は未構築**。policy 拒否（Review/Leave/NONE）は `Unplanable(PlanError::NotAutoMove)` として返る。したがって「policy refused」表示は NotAutoMove 分岐に実装しないと T39/T40 を満たせない（計画はこれを織り込み済み）
- `read_file: scripts/complexity_report.py:262-330` — baseline 比較は (file, function) キーで、freed name の一致を moved/renamed として扱う。`policy_view` の classify.rs → generic.rs 移動は **gate 失敗にならない**（CC 不変なら note 扱い）
- `read_file: tests/presentation_test.rs:80-156` — パス系テストの既存流儀（`$HOME` fixture、`COLUMNS` 変更は 1 テスト限定・復元必須）を確認
- `terminal: organize --format rich/json` 実測 — `--format json` がプレーン行になる事実を再確認（計画 §2.2 の記載と一致）

## 指摘事項

### 🔴 HIGH-1: 「`--format json` の出力が変化しない」は文字列不変を意味しない（受入基準2の解釈確定が必要）

受入基準2は文言のバイト不変では成立しない。`--format json` は通常のプレーン行と同一の print 経路（`src/main.rs:307-321`）を使っており、項目3で `NotAutoMove` を消す以上、この経路の文字列は必ず変わる。

**改善案**: (a) `resolve_mode`/分岐構造には一切触れない（JSON 化もエラー化もしない）、(b) 修正前後の `--format json` 全文を最終報告に並べて「分岐挙動は不変・文字列はプレーン改善の対象」と明記、(c) 解釈が違う場合に親が判断できるよう §7 の選択肢を維持。計画 T6 に (b) を明記した。

### 🔴 HIGH-2: policy 拒否の実経路は NotAutoMove（Refused は死に分岐）

未使用の `Refused` を残すか削除するかは挙動判断に踏み込むため、**表示のみ**で両方人間向け文言に写像し、削除はしない。T39/T40 の MUST 文言「policy refused」は NotAutoMove 表示で担保する。テストは「Review 相当（0.7/0.1）」「Leave 相当（0.2/0.05）」の plan 失敗×両方で `policy refused` を検証する。

### 🟡 MED-1: dry_run_banner 以外の watch / runner 文言は対象外であることを明記

`print_watch_start`（`watching ... (dry-run: true)`）と `watch/runner.rs`（`dry-run {path} ...`）は helper を使わない別文言で、矛盾する「executed」は含まない。今回は変更しない旨を計画の残課題に記録（レビュー時の「不整合」指摘を予防）。

### 🟡 MED-2: MoveFailed の文字列は依然 Rust Debug 風（`Io(Os { ... })`）

これは `operations/organize.rs:123` が `format!("{error:?}")` で作る**データ**で、watch の `ProcessOutcome::Failed` にも流れる。表示タスクで lib の文字列生成を変えると watch ログという別面の回帰になるため対象外とし、残課題に記録する（organize renderer は受け取った文字列をそのまま出す）。

### 🟡 MED-3: プレーンと rich でパス表示が異なる（`relative_display` / `paint_path`）

テストは両方の流儀を固定すること。フィクスチャは $HOME 配下の短いパス（truncation 非依存）とし、`COLUMNS` に依存させない。既存 `presentation_test.rs:143-156` のような環境変数操作が必要になる場合は 1 テストに隔離し復元する。

### 🟡 MED-4: `policy_view` の移動で classify の出力が変わらないこと

`src/presentation/renderers/classify.rs` の既存テスト（`tests/v31_render_test.rs` の `policy_is_real_verdict_and_no_move_to` / `none_row_and_confidence_margin`）が緑のままであること。移動は関数をそのまま移し、出力文字列を変えない（`generic::policy_view` に pub で、classify は委譲）。

### 🟢 LOW-1: プレーン見出しの語彙

`== REVIEW ==` は不変（T37 の MUST）。既存の `== Moved ==` / `== No move ==` は維持。新設は `== Mode ==` / `== Decision ==` / `== Policy ==`。WouldMove の見出しは `== Dry run ==` → `== Would move ==` に変わり、T34 の「出力に Dry run」はモード行 `dry run (no files moved)`（小文字）が担う。**ユーザーテスト計画の文言更新要否は親に報告**する。

### 🟢 LOW-2: テストの不在検証を必須化

「`NotAutoMove`/`UnknownCandidate`/`SourceMissing`/`SourceNotAFile`/`DestinationNotADirectory` の literal 不在」「`executed` 不在」「ANSI なし（プレーン）」「ASCII のみ（パネル）」を双方向で固定する。

## 判定

**APPROVE WITH CHANGES**（HIGH-1/HIGH-2 を計画に反映し、MED-1〜4 を残課題・テスト方針として明記すれば実装開始可）。計画書 §4/§5 を更新した。
