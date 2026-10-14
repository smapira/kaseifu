# Reviewer 記録: TREC ベンチマーク基盤 + AI リネーム基盤 Phase 0-2

- 日付: 2026-10-14
- 対象: 未コミット差分（TREC `20261014g`、AI リネーム `20261014f`）
- 実施: reviewer サブエージェント（編集なし・検証のみ）。親が指摘の取捨選択、修正、再検証を実施。
- 判定: 両プログラムとも合格（blocking 0 件）

## 検証（レビュー側の実測）

- `bash scripts/quality_check.sh` = OVERALL: PASS（568 passed / 0 failed）
- `cargo run --example fm_bridge_smoke` = 実推論 8633 ms、title/category 取得
- `cargo test --test v090_fm_bridge_test -- --ignored` = 3 passed
- `kaseifu-bench --help` = ASCII、8 サブコマンド、製品 CLI と分離
- 差分監査: 意図したファイルのみ、秘密情報・TREC データなし

## 指摘と処置（全 8 件）

| # | 重大度 | 内容 | 処置 |
|---|---|---|---|
| 1 | 中 | `--format json run` が `manifest:` 行を stdout に混入 | 修正済み（Text 時のみ印字） |
| 2 | Low | `compare` 表の `mean ms` 列幅ずれ | 修正済み（幅テスト追加） |
| 3 | Low | `newest_manifest` が辞書順最終 | 修正済み（mtime 選択＋テスト） |
| 4 | Low | `run` に `--qrels` がない | 修正済み（validate/prepare と同形） |
| 5 | Low | `reproducibility.timeouts` が CLI 後付け | 修正済み（`RunRequest.timeout_counter` 経由で `evaluate` が転記） |
| 6 | Low | 未使用の公開 API（`eligible_inputs`、`has_positive_topics`、`Prediction.timed_out`、`_hits_window`、`check_index_layout_only` の `qrels` 引数） | 修正済み（削除） |
| 7 | Low | 既定 suite が実機で実推論を実行するとの指摘 | 却下（誤読。`skip_with_a_live_model` ガードは実装済みで、モジュール文書と一致することを実測確認） |
| 8 | Low | run 出力ファイル名が秒単位で衝突し得る | 修正済み（`unique_output_path`＋テスト） |

## 再検証（親の実測）

- `bash scripts/quality_check.sh` = OVERALL: PASS（600 passed / 0 failed、CC>=16 = 0）
- 修正適用後のゲートで E0308（`run_command` 呼び出しの借用）、`cargo fmt` 違反 1 件、新規テストの時刻ラベル誤り 1 件を検出。いずれも修正してゲートを再通過させた。
