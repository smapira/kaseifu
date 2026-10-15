# 残タスク: 利用者のフォルダ名 `Cache` / `Caches` が改名できない

- status: done
- milestone: M1 - Reliability and CLI clarity
- completed: 2026-10-15 (parent thread)
- 出典: `documents/plans/dev/20261015c-ai-rename-p7.md` D8（「destination 規則は不変」として意図的に残した差分）
- 優先度: 中（理由文は出るので危険ではないが、利用者には理不尽に見える。一括では黙って除外される）

## 現状（実測、`f519188`）

- `kaseifu rename /tmp/issue-work/Cache` →
  `Error: /private/tmp/issue-work/Cache: folders inside a cache or temporary area are not renamed`
- `kaseifu rename /tmp/issue-work --recursive` のプレビューに `Cache` が**現れない**（`is_batch_target` が同じ規則で除外）。
- 根拠の規則は `src/context/directory.rs::is_cache_or_temp` で、**パス上のどこかの成分**が `cache` / `caches` / `temporaryitems` / `*.cache` に一致すれば拒否する。これは**移動先（destination）の除外規則**を改名対象に流用したため。

## やること

- 改名対象には「そのフォルダ自身の名前」が該当するときだけ拒否する（`~/Documents/Cache` を守る目的は果たしつつ、`~/Documents/my-cache/project` は許可）か、destination 規則との差を明文化する。
- どちらにせよ、**黙って除外しない**: 一括では `held: folders inside a cache or temporary area are not renamed` として見えるようにする（`is_batch_target` ではなく計画層で保留にする）。

## 受け入れ条件

- `Cache/` の完全一致の拒否は維持され、単発 rename で理由が出る。
- 一括のプレビューで**除外された項目が理由付きで見える**（または明示的な除外件数が出る）。
- destination の除外規則（`is_excluded_destination`）の判定結果は不変（既存テストがそれを固定している）。

## 制約

- CC 16 以上を作らない。既存の destination テストを書き換えない。

## 検証

- 実機: `Cache/` / `my-cache/project/` / `Library/` を単発と一括の両方で試し、結果を記録。

## Verification (2026-10-15, parent thread)

- New `src/context/directory.rs::is_cache_or_temp_name` (single-name check).
  `refuse_reason` refuses only the folder's own cache-like name, so
  `.../Cache` stays refused while `.../Cache/project` is allowed. The
  destination rule (`is_cache_or_temp`, whole subtree) is unchanged.
- Batch walk lists a cache/temp folder but never descends into it, so the
  plan holds it with its reason instead of dropping it silently.
- Real CLI: preview shows `Cache [held: ... folders inside a cache or
  temporary area are not renamed]`; single `rename` of `Cache` keeps the
  same refusal (dry run, isolated HOME).
- `cargo test --all-targets --all-features`: 873 passed / 0 failed;
  `bash scripts/quality_check.sh`: OVERALL PASS.
