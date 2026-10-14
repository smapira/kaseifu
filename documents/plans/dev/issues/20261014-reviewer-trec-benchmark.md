# Reviewer 検収: kaseifu TREC Benchmark（D2D Hit@1 評価基盤）

- 日付: 2026-10-14
- 対象: `documents/plans/dev/20261014g-trec-benchmark-d2d.md` とその実装
- 検収対象ファイル: `src/benchmark/{mod,cli,qrels,scatter,gate,search,systems,run}.rs`、`src/bin/kaseifu_bench.rs`、
  `tests/v090_trec_benchmark_test.rs`、差分 `src/lib.rs` / `Cargo.toml` / `.gitignore` / `src/discovery/mod.rs`
- 確認手段: 実コード読解、`cargo test`、`cargo clippy -D warnings`、`bash scripts/quality_check.sh`、
  `kaseifu-bench` の実機実行（実 qrels・実機 macOS 26.4.1）、原典 `documents/plans/user/20261014b.md` との突き合わせ

## 検収結果: 承認（条件付き）

差分は計画 §1--§9 と一致し、原典の禁止事項に違反していない。以下の検証を実測で確認した。

## 検証したこと（原典の要件単位）

| # | 要件（原典） | 検証方法 | 結果 |
|---|---|---|---|
| 1 | §3 正解集合は共通トピック由来 | Python で実 qrels から独立に再導出し manifest と突き合わせ（各 manifest 60 件） | **一致**（size / sha256 / 埋め込み id / 自己除外） |
| 2 | §3 正の判定のみ・未判定を否定しない | `Qrels::parse` は `rel > 0` だけを格納。実データの `rel` 分布 {0: 293998, 1: 16381, 2: 1031} を記録 | **適合** |
| 3 | §3 評価対象の固定 | 同一設定の `prepare` 2 回で manifest が**バイト単位で同一**（sha256 `6eb758ca...`） | **適合** |
| 4 | §3 ランダムサンプリングはシード固定 | `select_inputs` = splitmix64-ranked-selection-v1。seed 42/43 で別 sha、同一 seed で同一 | **適合** |
| 5 | §4 Hit@1 と補助指標 | `run_results` / `auxiliary_of` の純関数テスト（2/3 の検算、p95 = 95/100、median） | **適合** |
| 6 | §4 結果なしは 0 で分母維持 | 候補 0 件の 4 入力を分母に残し coverage 0 / no_result_rate 1.0 を固定 | **適合** |
| 7 | §5--6 通常のユーザーファイルを対象にしない | 検索は `mdfind -onlyin <corpus>` のみ。ヒットは正規化後の内含検査を通らない限り捨てる。
  コーパス外の文書を混ぜたフィクスチャで候補数が減ることを固定 | **適合** |
| 8 | §6 symlink 脱出の拒否 | symlink 経由のパスが `is_inside` で false になることをテスト | **適合** |
| 9 | §6 ファイル操作禁止 | 評価前後でコーパスの集合 / サイズ / sha256 / mtime が不変。`cleanup` は dry-run で 0 件削除、`scatter` は既存ファイルを上書きせずエラー | **適合** |
| 10 | §7 索引未完了なら中止 | `run` は `gate.ready == false` なら **検索を 1 回も実行せず**停止（テストで検索回数 0 を固定）。実機 `check-index` は exit 1 | **適合** |
| 11 | §7 macOS 版と索引状態の記録 | `check-index` / report に `sw_vers`（26.4.1 / 25E253）と `mdutil` の生出力を記録 | **適合** |
| 12 | §8 文書レベルのランキングを取り出す | `extract_terms` → `normalize_terms` → `find_similar_files_with` の**同一関数**を通ることを、
  製品経路（`discover_with`）とベンチ経路の送信クエリを記録して比較（語の集合一致） | **適合** |
| 13 | §8 フォルダランキングを D2D スコアにしない | `aggregate_by_directory*` を一切呼ばない（`grep` = 0）。Flat フィクスチャで兄弟文書が候補になることを固定 | **適合** |
| 14 | §8 検索・ランキングのロジックを変更しない | `src/discovery/**` の差分は re-export 1 行のみ（`git diff` で確認）。スコア再計算なし（恒等式をテストで固定） | **適合** |
| 15 | §9 3 ベースライン | kaseifu / bm25 / random が `--system` で切替可能。BM25 は本文、Random は seed 由来 | **適合** |
| 16 | §10 JSON / CSV の形 | JSON に `benchmark` / `metric` / `system` / `results{evaluated,hits,hit_at_1,coverage}` + `auxiliary` +
  `scattering` + `reproducibility` + `constraints`。CSV は 5 列固定（テストで列数固定） | **適合** |
| 17 | §12 の 10 テスト | `tests/v090_trec_benchmark_test.rs` の 17 テストで 10 項目すべてをカバー（下表） | **適合** |
| 18 | §14 の 3 制約の明記 | `CONSTRAINT_LINES`（5 行 = 原典 3 + 情報源差 + 検索語の限界）が全 report に入ることを固定 | **適合** |

### §12 の 10 項目とテストの対応

| 原典 §12 | テスト |
|---|---|
| 1 qrels の読み込み | `v090_qrels_loads_the_official_four_column_format` |
| 2 多対多からの正解集合生成 | `v090_many_to_many_shared_topics_become_the_ground_truth` |
| 3 自己文書の除外 | `v090_the_input_document_is_never_its_own_ground_truth` |
| 4 Hit@1 の計算 | `v090_hit_at_1_matches_the_worked_example`（分母・分子・整合性） |
| 5 検索結果なし | `v090_a_missing_result_is_a_miss_and_is_not_removed_from_the_denominator` |
| 6 評価対象外パスの拒否 | `v090_paths_outside_the_corpus_are_rejected` |
| 7 ファイル移動・削除なし | `v090_an_evaluation_leaves_every_corpus_file_untouched` |
| 8 同一入力→同一 manifest | `v090_the_same_inputs_produce_an_identical_manifest`（バイト比較も含む） |
| 9 索引未準備で開始しない | `v090_an_evaluation_never_starts_on_an_unready_index` |
| 10 macOS 実環境の統合 | `v090_macos_integration_scopes_the_real_mdfind_and_never_leaves_the_corpus`（実 `mdfind`） |

## 指摘事項

### 🔴 R1. `normalize_path` の混在（検出済み・修正済み）

含検査で「存在する根（`canonicalize` 済み）」と「まだ存在しない対象（字句正規化）」を比較すると、macOS の
`/var` → `/private/var` シンボリックリンクの下で**内部のパスが外部と判定される**。
テストが実際に検出し、`normalize_path` を「存在する最長の祖先を解決して残りを連結」に修正した（単体テストで固定）。

**判定**: 修正済み。このバグは散乱（`scatter`）でファイル作成**前**に含検査を行うため、実作業でも踏むものだった。

### 🔴 R2. テストフィクスチャの置き場所が製品の除外規則に当たっていた（検出済み・修正済み）

`std::env::temp_dir()`（macOS は `/var/folders/...`）にフィクスチャを置くと、先頭コンポーネント `var` が
`SYSTEM_ROOT_COMPONENTS`（`src/discovery/destinations.rs:61`）に該当し、kaseifu のヒットフィルタで全候補が消える。
フィクスチャを `CARGO_TARGET_TMPDIR`（リポジトリの `target/tmp`、git 管理外）へ移した。

**判定**: 修正済み。かつ**ユーザー向けの意味のある事実**として記録: コーパスを `/tmp` や `$TMPDIR` に置くと
評価は構造的に 0 点になる。`validate` / `check-index` が同じ理由で FAIL することを実機で確認済み。

### 🔴 R3. 「フォルダ集約を継承しない」ことの固定（検証済み）

集約層に戻すと Flat（単一ディレクトリ）条件で候補が常に 0 になる（`src/discovery/candidates.rs:28-30` が入力の親を除外するため）。
実装は文書層で止まり、Flat フィクスチャで候補 3 件が出ることをテストで固定した。

### 🟡 R4. `extract_terms` の語順が安定しない（検出、製品は変更せず）

`src/discovery/search_terms.rs:13` の `HashSet` を `into_iter()` するため、同一入力でも語順が変わりうる
（実測: `"1a6ac521 Text Plain"` / `"1a6ac521 Plain Text"`）。mdfind の語クエリは AND なので**測定結果には影響しない**が、
クエリ文字列を証跡にする用途では信用できない。

**判定**: 製品コードは変更しない（原典 §8 のロジック不変更）。ベンチ側は語の集合のみに依存し、テストも集合比較にした。
計画 §11.4-1 に記録。

### 🟡 R5. `cleanup --execute` は削除を行う（スコープ確認）

原典 §6 は評価中のファイル操作を禁じ、§15.1-6 は「ベンチマーク終了後に安全に初期状態へ戻せる」ことを要求する。
実装は (a) scatter manifest に載っているファイルだけ、(b) `--execute --yes` の明示時のみ、(c) 常に内含検査付き、
(d) 元データには一切触れない。評価経路（`evaluate`）には削除経路が無い。

**判定**: 適合。FAQ 的な誤用（原典の元データを消す）は構造的に起こらない。

### 🟡 R6. 隔離検索の置き場所を計画から変更（承認）

計画は `src/platform/macos/spotlight.rs` に関数を足すとしていたが、実装は `src/benchmark/search.rs` に置いた。
理由: 共有ファイルの編集を減らす（並行作業との衝突回避）かつ製品側に未使用の公開 API を増やさない。
製品の `spotlight::search` は無変更で、呼び出し元 2 箇所もそのまま。

**判定**: 承認（計画より良い）。計画 §1 の対象リストを更新した。

### 🟢 R7. `run` が読む manifest の選び方

`newest_manifest` はファイル名の辭書順で最新を選ぶため、`d2d-seed43-200.json` は `d2d-seed42-200.json` より新しいと見なされる。
実機で確認（seed 43 の manifest が選ばれた）。誤用は起きない（report に読んだファイルのパスと sha256 を記録する）が、
複数 manifest を置いた場合の振る舞いは計画に明記すべき。

**判定**: 許容（報告と計画に明記）。改善案として `--manifest <file>` の明示指定を残す。

### 🟢 R8. 完了条件（原典 §13）のうち今回満たせないもの

「TREC Robust04 の実データを使用できる」「BM25 との比較結果を取得できる」は**コーパスが無いため未達**（代用データは作らない）。
qrels 依存部分は実データで検証済み。この未達は環境要因であり、実装の欠落ではない。

**判定**: 条件付き承認。原典 §2 の「データが取得できない場合は停止して報告」に従い、ユーザー判断を仰ぐ。

## 結論

- 原典の禁止事項（ランキング変更・フォルダスコアの代用・ユーザーファイルへの影響・データのコミット）への違反は**なし**。
- 検出した 2 件のバグは修正済みでテストに固定。実装上の妥協は R4（製品コードを変えない）と R7（許容）のみ。
- 実データによる決定性と、実機でのゲート動作を確認済み。**実装を承認**し、QA の品質確認へ進む。
- コーパス取得後の本評価実行（原典 §13 の残り）はユーザーのデータ取得待ち。
