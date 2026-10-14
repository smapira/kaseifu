# Plan Architect レビュー: kaseifu TREC Benchmark（D2D Hit@1 評価基盤）実装計画

- 日付: 2026-10-14
- 対象: `documents/plans/dev/20261014g-trec-benchmark-d2d.md`
- 関連コード: `src/discovery/{mod,similarity,scoring,candidates,destinations,search_terms}.rs`, `src/models/similar_file.rs`,
  `src/models/file_context.rs`, `src/platform/macos/spotlight.rs`, `src/classifier/pipeline.rs`, `src/normalize/mod.rs`,
  `src/context/image_metadata.rs`, `src/lib.rs`, `Cargo.toml`, `.gitignore`, `scripts/quality_check.sh`
- 確認手段: 直接読解 + `grep` + 実機 `mdfind` / `curl` / `awk`（serena / context7 / codebase-memory-mcp / graphify は本環境で未接続）

## 検証したこと（計画の前提が実コードと一致するか）

- `discover_with`（`src/discovery/mod.rs:60-129`）の内部 `similar_files`（同 78-85）が文書単位の `SimilarFile` を持ち、
  `similarity_score` は `scoring::score_file`（`src/discovery/scoring.rs:175-201`）の 3 項（0.60/0.25/0.15）で決まる。
  **フォルダ集約（`aggregate_by_directory_with_exclusions`）はその後の別層**であり、文書層の出口で止める計画は実装可能。
- `pub use`（`src/discovery/mod.rs:19-20`）は `find_similar_files` のみで `find_similar_files_with` を含まない。
  注入経路を使うには **re-export 1 行の追加が必要**（予定どおり）。`find_similar_files_with` 自身は `pub fn`（`src/discovery/similarity.rs:24`）。
- `is_excluded_destination` は先頭 3 コンポーネント内の `Library` を除外（`src/discovery/destinations.rs:30-33,112-117`）し、
  既存テストが `/Users/test/Library/Application Support/Anki2/...` を除外例として固定（同 234）。
  ヒット段（`src/discovery/similarity.rs:70`）と集約段（`src/discovery/candidates.rs:34`）の両方で呼ばれる。
- 集約段は入力自身の親ディレクトリを除外（`src/discovery/mod.rs:106`、`src/discovery/candidates.rs:28-30`）。
- `MAX_SPOTLIGHT_HITS = 10_000`（`src/discovery/mod.rs:31`）は `find_similar_files_with` 内で検索境界の引数として渡る（`src/discovery/similarity.rs:46`）。
- `spotlight::search` は `mdfind <query>` を実行し `take(limit)`（`src/platform/macos/spotlight.rs:119-132`）。`-onlyin` はコードベースに **0 件**。
- `SearchFn` は `Fn(&str, usize) -> Result<Vec<String>>`（`src/discovery/mod.rs:65`、`src/classifier/pipeline.rs:224`）で、ベンチ側のクロージャを渡せる。
- `NormalizedFileContext` の `target` / `semantic_signals` / `parent_directory` は `pub`（`src/models/file_context.rs:68-81`）。
  `SimilarFile` の全フィールドも `pub`（`src/models/similar_file.rs:6-28`）。
- `MetadataOptions` は `Default` 実装済み（`src/context/image_metadata.rs:22-29`）で、既定は spotlight=true / exif_fallback=true。
  `context::builder::build` と `normalize::normalize_with_options` は `pub`（`src/classifier/pipeline.rs:226-227` と同型）。
- 実機実測: `~/Documents/kaseifu-benchmark/` と `~/Library/Application Support/kaseifu/benchmark/` は
  **どちらも `mdfind` で発見できる**（name / content 両クエリ）。`/tmp` は `mdfind -onlyin /tmp` = 0 件。
  つまり原典 §5 のパスが使えない理由は「索引されないから」ではなく **kaseifu の除外規則に当たるから**（計画 P0-5 の根拠がこれで正しい）。
- 実 qrels: 6,543,541 B / 311,410 行 / 249 トピック（**672 が欠番**）/ `REL ∈ {0,1,2}` で実測分布は {0: 293,998, 1: 16,381, 2: 1,031} /
  正判定 17,412 行 / 正判定文書 15,940 種。正判定 docno に **FR が 1,147 件含まれる**。
- `scripts/quality_check.sh` は `rust-code-analysis-cli -m -p src`（**src のみ**）+ `complexity_report.py check` で
  「新規 CC>=16 / 既存の悪化」をゲートする。テスト側の CC はゲート対象外だが、`src/benchmark/**` は全関数が対象。

## 指摘事項（実装に反映済み）

### 🔴 F1. コーパス根が `~/Library` 配下だと kaseifu は構造的に全滅する

原典 §5 の `~/Library/Application Support/kaseifu/benchmark/` をそのまま使うと、`is_excluded_destination`（`destinations.rs:112-117`）が
`/Users/<user>/Library` を除外するため、ヒット段（`similarity.rs:70`）で全候補が落ち、**kaseifu だけでなく検索結果が常に 0 件**になる。
索引の問題ではない（実機で `mdfind` は発見できることを確認済み）。

**反映**: 作業領域を `~/Documents/kaseifu-benchmark/` にする（計画 §3-3）。原典との差異は報告に明記。
`validate` と `check-index` は「コーパス根が除外規則に当たる」を **exit 1 の FAIL** にする（黙って 0 点を量らないため）。

### 🔴 F2. 文書層で止めることを回帰テストで固定する

集約段で止めると、`candidates.rs:28-30` の「入力自身の親ディレクトリを除外」により、原典 §15.5 の **Flat（単一ディレクトリ）条件では候補が常に 0 件**になる
（全文書の親が入力の親と一致するため）。文書層で止める限りこの規則は継承されない。
これは設計の要なので、**Flat 配置のフィクスチャで候補が 0 件でないこと**をテストで固定し、将来の配線ミスでこの性質が黙って壊れないようにする。

**反映**: 計画 §3-1 / C3-1 / T5、テスト v090 の「flat でも候補が出る」ケース。

### 🔴 F3. 検索語はファイル名とメタデータからしか作られない（本文を読む経路が無い）

`extract_terms` の入力は `semantic_signals` だけ（`src/discovery/search_terms.rs:12-47`）で、その実体は
`F:`（ファイル名ステム、`src/normalize/mod.rs:51-68`）と `M:`/`K:`（Spotlight メタデータ）と `X:`（xattr）。
`grep -rn 'read_to_string|File::open' src/` は `src/config/loader.rs` と `src/completion/mod.rs` 以外にヒットしない。
したがって TREC コーパスのようにファイル名が内容語を持たない条件では、**検索語が内容に当たらず `coverage` が 0 に近づく可能性が高い**。

これは評価基盤の欠陥ではなく**現行 Discovery の能力境界**であり、直すには検索語生成のロジック変更が必要（原典 §8 が禁止、かつユーザー判断が要る論点）。

**反映**: 計画 §2.3（予測であることを明記）・§8 ⑤・出力 JSON の `diagnostics`（`inputs_with_empty_terms` / `inputs_with_zero_candidates`）と `constraints`。
報告にも最優先の論点として書く。

### 🔴 F4. `--onlyin` だけでは隔離として不十分（内含検査が必須）

`-onlyin` は入力パスの下を検索するが、結果は symlink / 別名経路で外を指し得る。
原典 §6 は「パスの正規化後に検証」を要求している。

**反映**: 計画 §3-2。`canonicalize` 優先 + 失敗時は字句正規化（`.`/`..` 解決）で内含を判定し、外の候補は捨てる。
入力文書も内含を要求し、外なら入力を拒否（§12-6 のテスト対象）。

### 🟡 F5. mdfind に timeout が無いという事実を分離する

既存 `spotlight::search` は `Command::output()` で無制限待ち（`spotlight.rs:119-132`）。ベンチ側で打ち切るには
`spawn` + `try_wait` + `kill` が要る。**既存 `search` は絶対に変えない**（通常利用の挙動変更になる）。

**反映**: 新関数 `search_onlyin` は `-onlyin` 版のみ。timeout ラッパは `src/benchmark` 側に置く（計画 §3-7）。
引数順は実機で `mdfind -onlyin DIR QUERY` が動くことを確認済み。

### 🟡 F6. manifest の件数表記は実データに合わせる（250 ではなく 249、672 欠番）

原典 §7 の 250 トピックという前提をそのまま書くと、生成物が実データと食い違う。

**反映**: `dataset.topics_total = 249` と `topics_missing = [672]` を実測値として保存する（計画 §6）。`REL` は {0,1,2} のみなので
「段階的評価」は 2 段階の正例しか持たない事実も `derivation.graded_values_saved` の説明に含める。

### 🟡 F7. FR 欠落時の分母の扱い

正判定に FR が 1,147 件あるため、472,525 文書版（FR 除外）を使うと入力文書が実在しない可能性がある。

**反映**: `scatter` は配置できなかった doc_id を `missing_doc_ids` として manifest に記録。`run` は入力不在を
`skipped_missing_input` として数え、**`evaluated` に混ぜない**（計画 C5-9 / P0-10）。

### 🟡 F8. ゼロ除算・浮動小数の決定性

`evaluated == 0` のとき `hit_at_1 = hits / evaluated` は NaN になる。原典 §4 は「結果が無い場合は 0 として分母から除外しない」だが、
これは**入力がある場合**の話であり、入力 0 件は評価不成立である。

**反映**: 指標は `evaluated == 0` ならすべて 0.0 を返す純関数にする（NaN / -0.0 を出さない）。入力 0 件の `run` は
「評価対象が 0 件（manifest の入力が実在しない）」として**エラー終了**し、0 点の run を作らない。

### 🟢 F9. 既存挙動の非破壊性

- `MAX_SPOTLIGHT_HITS` はベンチ側で再定義せず `discovery::MAX_SPOTLIGHT_HITS` を渡す（候補集合を製品と同じ打ち切りにする）。
- `MetadataOptions` は `Default` を使う（ユーザー config / Keychain を読まない = 隔離と再現性の両方に必要）。設定は JSON に記録。
- `search_onlyin` の追加は既存 `search` の呼び出し元に影響しない（`grep` で `spotlight::search` の利用は 2 箇所、どちらも `search` のまま）。

### 🟢 F10. 品質ゲートへの適合

`cargo clippy --all-targets --all-features -- -D warnings` は新バイナリ・新テストも対象。
`rust-code-analysis-cli` は `src` のみを解析するので、`src/benchmark/**` の全関数を **CC <= 15** に抑える（目標 <= 10）。
長い `match` はハンドラ関数に分け、`--max-inputs 0` などの分岐は早期 return で書く。
テストは `tests/v090_*` に置き、CI（データ・ネットワーク無し）で隔離 HOME / 一時ディレクトリ / 注入経路のみを使う。

## 備考

- F1〜F10 は計画 §2--§9 に反映済み。上記をもって計画を**承認**し、Implementer へ実装を依頼する。
- 承認の条件: F3 の帰結（kaseifu の coverage が 0 になりうる）を **実測と混同しない**こと。実コーパスが無い今回の成果物では、
  これは予測としてのみ報告する。
