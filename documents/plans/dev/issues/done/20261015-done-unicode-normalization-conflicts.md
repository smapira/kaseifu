# 残タスク: 一括の衝突検出が Unicode 正規化差を畳まない

- status: done (moved from `open/` 2026-10-15)
- milestone: M3 - Workflow semantics
- next owner: none (verify at merge time)
- blocker: actionable
- decision (2026-10-15, owner): adopt option (2), introduce the `unicode-normalization` crate as an explicit exception to the no-new-dependencies rule for this issue only
- 出典: `documents/plans/dev/20261015d-ai-rename-p8.md` D3 と §10、`20261015-reviewer-ai-rename-p8.md` R3
- 優先度: 低（実害は「プレビューで提案として見えるのに適用時に 1 件失敗する」に限られる。上書きは起きない）

## 現状

- 一括の衝突判定（`rename::batch::hold_conflicts`）は **同一ディレクトリ + 小文字化した宛先名** でまとめる。大文字小文字は畳むが、**NFC / NFD の正規化差は畳まない**。
- macOS の既定ボリュームは正規化非依存なので、`é`（NFC）と `é`（NFD）は同じファイルになる。
- 安全性は保たれている: 適用時の項目ごとの再確認が「既に在る」を検出して**上書きせず**、その項目だけを失敗させる（Phase 8 の統合テストで固定）。

## やること（どちらかを選ぶ）

- (1) 正規化表を自前で持つ（新規 crate なしで、必要な範囲の NFC/NFD 畳み込みだけを小さく実装する）。どこまでを対象にするか（結合文字・互換分解の扱い）を決めて記録する。
- (2) ユーザーに依存追加の許可を求め、`unicode-normalization` を使う。→ **この選択はユーザー判断**（「新規 crate 依存を追加しない」という現行ルールに反するため）。
- (3) 何もしないと決めて、プレビュー時点で「正規化差の衝突は適用時に検出される」ことを README に明記する。

## 受け入れ条件

- 選んだ方針が計画書に理由付きで残っている。
- (1)/(2) を選ぶなら、正規化差だけが異なる 2 提案のテストがある。
- (3) を選ぶなら README に限界として書かれている。

## 制約

- 現行ルールでは新規 crate を追加しない。(2) はユーザーの明示的な許可が必要。
- CC 16 以上を作らない。

## 検証

- 正規化差だけで異なる名前を 2 提案させ、一括のプレビューと適用の結果を記録。

## オーナー回答 (2026-10-15)

`unicode-normalization` を導入

Recorded as the decision above: option (2). The crate is allowed for this
issue only; the general no-new-dependencies rule stays in force elsewhere.

## Verification (2026-10-15, worktree `kaseifu`)

- `cargo add unicode-normalization` -> 0.1.25 (+ tinyvec). `Cargo.toml` +1
  line, `Cargo.lock` +16 lines, both purely additive (verified
  `git diff --stat`). No other dependency change in this worktree.
- `folded_name` is now NFC-then-lowercase; `hold_conflicts` doc comment
  updated (it used to say folding needs a table the tool does not ship).
- Tests: `batch.rs` unit `normalization_only_differences_are_conflicts`
  (NFC `caf\u{e9}.pdf` vs NFD `cafe\u{301}.pdf` held, third item ready) +
  `tests/v105_unicode_normalization_test.rs` (3 pass: conflict, ASCII
  behavior unchanged incl. case folding and no-new-holds, no cross-directory
  conflicts).
- On-volume premise verified on this Mac (APFS): writing NFC `café.pdf` then
  opening NFD `café.pdf` with O_EXCL fails with FileExistsError (one file).
  A model-driven preview-vs-apply demo was not forced (would need the model
  to propose a normalization pair on demand); the grouping rule is pinned
  above and the apply-time recheck still refuses occupied names.
- Regression: lib batch suite 11 pass; v097 8/9 with the 1 failure being the
  known parent-owned conflict (`v097_cli_never_waits_for_input_without_a_terminal`:
  the real model echoes current names and the parent's uncommitted
  `model-output-degeneracy` hook holds them, so `status: applied` never
  prints; deterministic 2/2, unrelated to NFC which is identity on ASCII).
  clippy `-D warnings` clean, `cargo fmt --check` clean.
