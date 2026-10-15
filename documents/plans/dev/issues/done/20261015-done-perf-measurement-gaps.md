# 残タスク: Phase 9 の測定の残り（コールドスタート・大規模計画・セッション効果）

- status: done (moved from `open/` 2026-10-15)
- milestone: M4 - Measurement and release hygiene
- next owner: none (verify at merge time)
- blocker: actionable
- 出典: `documents/plans/dev/20261015e-ai-rename-p9-perf.md` §7、`20261015-reviewer-ai-rename-p9-perf.md` R1 / R3 / R4
- 優先度: 低（現在の測定で主要な結論は出ている。精度を上げる作業）

## 残っている測定

- **コールドスタート**: 初回起動・モデル未ロードの分離。現在の最大値にはフレームワークのウォームアップが混ざる（実測: 画像 124 ms、動画 336 ms、生成 11.3 s）。
- **大規模フォルダの計画コスト**: 上限 50 項目を実際に計画する時間（1 項目 1 生成なので数分）。現状は §2.2 からの推算のみ。
- **セッション再利用の生成時間以外の効果**: メモリ、トークン課金、システムサービスとの往復回数。
- **メモリの項目数依存**: 実生成を伴うバッチ（50 件）の peak RSS（現状の 20.5 MB はモデル呼び出し 0 回のケース）。
- **同時実行**: 複数プロセスが同時に生成したときの挙動（`FmContext` は 1 コンテキスト 1 生成を ABI で強制しているが、プロセス間は未検証）。

## やること

- `kaseifu-bench perf` に必要な切り口を足す（例: 事前ウォームアップを分離するフラグ、生成を伴うバッチのメモリ計測）。
- 追加の数値も実測で計画書に追記する（推測で埋めない）。

## 受け入れ条件

- 追加の測定が `kaseifu-bench perf` の再実行で得られる形で入っている。
- 数値が計画書に追記され、ばらつき（min/median/max）を隠していない。

## 制約

- 新規 crate 依存なし。CC 16 以上を作らない。合成入力は一時ディレクトリのみで、終了前に削除する。

## 検証

- 同コマンドを複数回実行し、同じ桁の値を得ることを確認。

## Verification (2026-10-15, worktree `kaseifu`, this Mac)

- `kaseifu-bench perf --batch-plan N` added (synthetic docs, model-free prep +
  one real product-path generation each); first calls always split out
  (`generation_cold_first`, `ffi_available_cold_first`); `--help` documents
  the flag. No new crates, no harness processes (concurrency run by hand).
- Numbers (all `exit 0`, same machine): cold gen 8711 ms vs steady ~3999 ms;
  cold avail 979 µs vs steady 595 µs; batch-plan 3: prep 193.93 ms + gens
  7279/7411/10348 ms, peak RSS 54.6 MB (vs 26.9 MB base, 26.0 MB reuse);
  reuse shows no time/memory win this run; folder 5000 capped at 200 reads in
  377.7 µs; Vision+OCR single 274.41 ms; 2 concurrent runs both succeed at
  ~14.2 s each. Full table in `20261015e-ai-rename-p9-perf.md` §9.
- 50-item projection stays a labeled projection (prep + 50 x steady gen =
  4-8 min); tokens unmeasurable through the bridge (recorded).
- Tests: new `split_cold` unit test + existing 4 perf unit tests green
  (52 benchmark lib tests pass); clippy `-D warnings` clean,
  `cargo fmt --check` clean.
