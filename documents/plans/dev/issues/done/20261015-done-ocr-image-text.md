# 残タスク: 画像内文字の認識（OCR）を `text` 配列として追加

- status: done (moved from `open/` 2026-10-15)
- milestone: M2 - Media quality
- next owner: none (verify at merge time)
- blocker: actionable
- 出典: `documents/plans/dev/20261014k-ai-rename-p5b-vision.md` D5（次回条件）
- 優先度: 中（原典 §5.2 の「必要時」条件。看板・書類・スクリーンショットの題名が伸びる）

## 背景

- p5b は OCR（`VNRecognizeTextRequest`）を「分類ラベルの配線・縮退・bound が固まってから、需要と注入対策をセットで設計する」として先送りした。**その条件は満たされた**（Phase 5b〜6 で配線は着地）。
- 実測（p5b 時点）: 合成の看板風画像で 0.397 秒で `OPEN` / `9:00 - 18:00` を取得。動く。
- 原典は OCR を「不要な OCR を実行しない」（§11.3）と条件付きにしているため、常時ではなく必要なときだけ走らせる設計が要る。

## やること

- `kaseifu_vision_perceive` の応答に `text` 配列を足す互換拡張（`schema_version` は据え置き、既存キーは不変）。
- プロンプトに渡すのは bound 済みの断片のみ（件数・文字数を既存の label 予算と同じ考え方で制限）。
- **注入面が 1 つ増える**ことを前提に設計する（画像に書かれた指示文はファイル名と同じ untrusted 入力。既存の `contains_prompt_injection` と「指示文があれば保留」の扱いを、テキスト断片にも適用するか決める）。
- 走らせる条件（対象種別、文字が写っていそうなときだけ、コスト上限）を決めて記録する。

## 受け入れ条件

- `text` が `--format json` に現れ、既存キーが削らない（追加のみ）。
- テキスト入りの合成画像で題名が改善することの before/after 証跡。
- 注入文を画像に書いた場合に、モデルへ渡る前に保留される（または安全に扱われる）ことのテスト。
- `cargo test --all-targets --all-features` と `bash scripts/quality_check.sh` が緑。

## 制約

- 新規 crate 依存を追加しない。新規設定ファイル・別 DB・独自キャッシュ・外部プロセスなし。
- CC 16 以上を作らない。出力は ASCII、新規コードに em dash を書かない。
- テストは隔離 HOME と一時ディレクトリのみ。実写真を使わない。

## 検証

- 単体: 応答のパースと bound（既存 `semantic::perception` のテストに追加）。
- 実機: 合成画像 2 種（文字あり・なし）で `kaseifu rename --format json` を実行し、`text` と題名の差を記録。

## Verification (2026-10-15, worktree `kaseifu`)

- Swift `kfPerceive` runs `VNRecognizeTextRequest` (`.fast`, no language
  correction) on the same bounded thumbnail and answers `"text": [...]`
  (additive, `schema_version` unchanged, empty on any OCR failure so OCR never
  fails perception). Video frames intentionally excluded (per-frame cost
  multiplier; recorded).
- Rust: `PerceptionInfo.text` (`#[serde(default)]`, always serialized),
  `bound_texts` (trim, printable-ASCII-only, 8 x 120 chars, case-insensitive
  dedupe), `FmPerception.text` (rebounded at the wire), Swift prompt
  `image_text:` data line marked untrusted, `PerceptionReport::Labels {
  labels, text }`, `rename --format json` gains additive `"text"`.
- Injection: OCR fragments go through `contains_prompt_injection` in
  `plan_image_rename`; a hit withholds the proposal with the existing reason
  before any inference (`bridge.requests()` stays empty).
- Tests: `perception.rs` unit (parse alongside labels, legacy payloads,
  trim/ASCII/bound/dedupe), `contract.rs` (text travels, rebounded),
  `tests/v100_ocr_image_text_test.rs` (3 pass + 1 manual `#[ignore]`).
  Necessary adaptations only in v094/v095 asserts (struct variant).
- Real device (this Mac, PIL fixtures in `/tmp`, nothing committed): sign
  image yields 3 fragments in 2.672 s (fast-level misreads recorded); blank
  image yields 12 labels and 0 fragments with perception succeeding. Invoice
  fixture: 3/3 runs read clean text and title `2026-10-15_インボイス.png`;
  textless twins yield generic/wrong titles (`印刷されたページのイラスト` /
  `画像` / `手書きのアート作品`). Titles move from generic to specific.
- `cargo test --lib` 223 passed; v094 16 passed; v095 24 passed; v100 passed.
  clippy `-D warnings` clean, `cargo fmt --check` clean.
