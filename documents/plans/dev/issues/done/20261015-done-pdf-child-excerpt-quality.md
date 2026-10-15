# 残タスク: フォルダ命名の品質（子が PDF のとき）

- status: done (moved from `open/` 2026-10-15)
- milestone: M2 - Media quality
- next owner: none (verify at merge time)
- blocker: actionable
- 出典: `documents/plans/dev/20261015c-ai-rename-p7.md` §9.3 / §10、`20261015-reviewer-ai-rename-p8.md` R3
- 優先度: 中（実利用で最も多い「書類フォルダ」で題名が弱い）

## 背景（実測）

- フォルダの題名は「子の本文抜粋（最大 3 子 × 32 KiB、長い順に上位 4 件/子）」と「子の名前・種別分布」から作る。
- 子が**非圧縮 PDF** のとき、Phase 4 のバイト走査が辞書・xref を抜粋として拾う（実測: `T:%PDF-1.4 1 0 obj << /Type /Catalog ...`）。実 PDF は内容ストリームが圧縮されており、抽出できる本文はさらに少ない。
- 結果として PDF だけのフォルダでは題名が「種別の羅列」に寄る（実測の例: `合同` / `pdfファイル群`）。テキスト・Markdown の子では良好に命名できる（実測: `請求・契約関連書類` 相当）。

## やること（候補、実測で選ぶ）

- PDF の text オペレータ（`BT`/`Tj`/`TJ`）と FlateDecode ストリームの最小限の扱いを検討し、辞書語彙より本文を優先する。新規 crate は追加しない前提で、どこまでできるかをまず実測する。
- あるいは「PDF は子の名前と種別を主な証拠にする」と決めて、題名が種別羅列になるのを避けるプロンプト指示を入れる。
- どちらを採るかは合成 PDF と（可能なら実 PDF のコピーで）before/after を取って決める。

## 受け入れ条件

- PDF 主体のフォルダで、題名が種別羅列（`pdfファイル群` 等）に退化しないことの before/after 証跡。
- 既存の文書・画像・動画経路が不変。

## 制約

- 新規 crate 依存なし。CC 16 以上を作らない。読み取りのみ（PDF を変更しない）。

## 検証

- 合成フォルダ 3 種で複数回実行し、題名の分布を記録（モデルは非決定的なので 1 回では判断しない）。

## Verification (2026-10-15, worktree `kaseifu`)

Chose the extraction fix (no new crate, so FlateDecode streams stay opaque;
recorded limitation), not the prompt-instruction fallback.

- `extract_signals(Pdf, _)` now decodes `(...)` literals (nesting, backslash
  escapes incl. octal and line continuations, UTF-16BE BOM) and `<...>` hex
  strings, strips `xref`...`%%EOF` sections (trailer ID hex), and falls back
  to printable runs only when no string decodes. Dictionary keywords, stream
  operators and object wrappers never sit inside strings, so they contribute
  nothing. `strongest_signals` doc comment updated.
- Signals before (same 3 fixtures, old code): 6 noise signals per file
  (`T:%PDF-1.4 1 0 obj << /Type /Catalog ...`, xref rows, trailer IDs).
  After: pdf1 `T:Quarterly Report Q3`; pdf2
  `T:Annual Summary Jane Doe Quarterly Report Q3` (metadata + body);
  compressed pdf3 `T:Compressed Report` (metadata only, no garbage).
- Real-model folder runs (this Mac, 3 synthetic PDFs, 3 runs): all specific
  content titles with body words (`Quarterly_Report_Q3_revenue...` etc.); no
  type-list title in any run (the `model-output-degeneracy` holds stay on as
  backstop). Folder evidence flows through the same `extract_signals`
  (`document_child`), so no folder-code change was needed.
- Tests: 4 new `document.rs` unit tests (body-beats-noise, hex+escapes,
  compressed-opaque, UTF-16BE) + `tests/v101_pdf_excerpt_quality_test.rs`
  (2 pass, file-backed, children do not bleed). Existing
  `pdf_printable_runs_are_scanned` passes unchanged (fallback path).
- `cargo test --lib` document suite 18 passed; clippy `-D warnings` clean;
  `cargo fmt --check` clean.
