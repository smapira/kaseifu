# QA 品質確認: kaseifu TREC Benchmark（D2D Hit@1 評価基盤）

- 日付: 2026-10-14
- 対象: `documents/plans/dev/20261014g-trec-benchmark-d2d.md` の実装（HEAD `9be5f71` + 未コミット差分）
- 環境: macOS 26.4.1（build 25E253）、arm64、rustc 1.98.1、Spotlight 有効
- 確認手段: `bash scripts/quality_check.sh`（= CI と同じコマンド）、`cargo test` / `cargo clippy` 直接実行、
  `kaseifu-bench` の実機実行、実 qrels での決定性実測、Python による独立再導出

## 1. 品質ゲート（実測出力）

```
KASEIFU / QUALITY REPORT

Cyclomatic Complexity
  Functions analyzed     935
  CC <= 10               926          99.0%
  CC 11-15               9
  CC >= 16               0
  Maximum CC             15
  Average CC             2.87
  Cognitive >= 21        1 (warn-only)
  Lines >= 101           3 (warn-only)
  Unmeasurable           0 (excluded, never CC 0)

  rustfmt|PASS|violations 0
  clippy|PASS|warnings 0
  cargo-test|PASS|passed 512, failed 0
  complexity|PASS|analyzer 0.0.25
  baseline-gate|PASS|no new/worsened CC>=16
  report-tests|PASS|complexity_report self-tests green
  coverage|PASS|report-only (line 81.59%)

OVERALL: PASS
```

- **CC >= 16 = 0、最大 CC 15 は既存ベースラインと同一**（新規関数はすべて CC <= 15 に収めた。計画 §5 の T1--T14 を小さな関数に分割）。
- `baseline-gate` は「新規 CC>=16 / 既存の悪化」なしで PASS。
- coverage は report-only（ゲート外）。85.26% → 81.59% に低下した。原因は新規モジュールのうち
  CLI ハンドラ層（実ディレクトリを要求するため自動テストが薄い）が分母に加わったため。純関数層
  （qrels / 指標 / 散乱 / システム / 経路検査）は単体テストで網羅している。

## 2. テスト

```
cargo test --all-targets --all-features
  passed 512 / failed 0（ベースライン 450 + 新規 62）
  新規テスト関数 62 個 = src/benchmark/** 45 + tests/v090_trec_benchmark_test.rs 17
```

- CI と同じ条件（ネットワーク・実 qrels・実コーパス無し）で全テストが通る。テストは隔離ディレクトリ
  （`CARGO_TARGET_TMPDIR`）と注入検索のみを使い、実ユーザーの `~/Library/Application Support/kaseifu/`、`~/Desktop/`、
  実 qrels・実コーパスを読み書きしない（`grep` で実パス参照が無いことを確認）。
- 新規バイナリは `cargo test --all-targets` の対象に含まれ、コンパイル時データ要求は無い（CI 安全）。
- 既存のヘルプ走査（`tests/v070_zero_config_p1_help_test.rs`）と補完整合（`tests/completion_test.rs`）は無修正で通過
  （ベンチは別バイナリのため）。

## 3. 手動確認（実データ・実機）

| 確認項目 | 結果 |
|---|---|
| 実 qrels の validate | PASS（6,543,541 B、sha256 `f8f2c972...f4c3`、311,410 行、トピック 249、欠番 [672]、正判定 17,412、判定値 {0: 293998, 1: 16381, 2: 1031}） |
| `prepare` の決定性（2 回） | `manifest_sha256 = 6eb758ca...f4920`、ファイルは**バイト単位で同一** |
| seed / 件数感度 | seed 42→`802e5031...`（100 件）/ `6eb758ca...`（200 件）/ seed 43→`720b5540...`（200 件）と別物 |
| 全件 manifest | 15,940 件 / 0 除外 / 38.8 MB / 1.54 秒 / sha256 `86b2b073...` |
| 独立再導出 | Python で qrels から再計算した 120 件が size / sha256 / 埋め込み id / 自己除外で一致 |
| `check-index`（コーパス不在） | exit 1。macOS 26.4.1 / 25E253 / aarch64、`/` と `/System/Volumes/Data` ともに Indexing enabled、理由を明示 |
| `run`（コーパス不在） | exit 1、**検索を実行しない**（scatter を先に要求） |
| `compare`（run 無し） | exit 1、`no runs found in .../runs` |
| `validate` の除外規則検出 | `~/Library/Application Support/...` をコーパス根にすると FAIL（理由と `destinations.rs:112` を表示） |
| 読み取り専用 | 評価前後でコーパスのサイズ / sha256 / mtime が不変（テストで固定） |
| 出力の ASCII 性 | 全出力・全 JSON / CSV が ASCII（テストで `is_ascii` と 5 列を固定） |

## 4. 検証できなかったこと（正直な限界）

1. **実コーパスでの本評価と Hit@1 の数値は未測定**。原典 §2 に従い代用データを作らないため。
   したがって §13 の「TREC Robust04 の実データを使用できる」「BM25 との比較結果」は未達（環境要因）。
2. **528,155 文書規模の BM25 索引のメモリ量は未測定**（1 パスで索引を作る実装。上限は `--bm25-max-docs` で指定可能）。
3. **Spotlight が数百千文書の索引を完了するまでの時間と、`mdfind` の 1 回あたり所要時間は未測定**
   （打ち切りは `--timeout-ms`、超過は `timeouts` に記録）。
4. 専用 macOS ユーザーの分離（原典 §6 の「可能であれば」）は未実施。`sandbox-exec` は原典どおり必須要件にしていない。

## 5. 判定

- ゲートは **OVERALL: PASS**（rustfmt 0 / clippy 0 / 512 passed 0 failed / CC>=16 = 0 / 最大 CC 現状維持）。
- 実データ（公開 qrels）での決定性と、実機での索引ゲート・拒否経路を実測で確認した。
- 残りはコーパス依存（上記 4-1）のみで、実装の欠落ではない。
- **品質確認を承認**。コミット / push / tag は親スレッドの判定に従う（本スレッドでは未実施）。
