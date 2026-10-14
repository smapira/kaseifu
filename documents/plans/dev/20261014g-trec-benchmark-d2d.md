# kaseifu TREC Benchmark（D2D 関連ファイル選択の Hit@1 評価基盤）実装計画

- 起票: 2026-10-14
- 原典: `documents/plans/user/20261014b.md`（開発指示書: kaseifu TREC Benchmark、642 行。§2 データ、§3 D2D 構築、§4 指標、§5--7 実行環境、§8 評価エンジン、§9 ベースライン、§10 出力、§11 CLI、§12 テスト、§13 完了条件、§14 制約、§15--16 散乱。**読み取り専用。編集しない**）
- 別プログラム: `documents/plans/user/20261014a.md`（Apple Foundation Models 統合 / AI リネーム）は**スコープ外**。実装も計画もしない
- 前提: HEAD `9be5f71`、worktree clean（`documents/plans/user/*.md` は未追跡のユーザー管理ファイル）
- 対象: 新規 `src/benchmark/**`（`mod.rs` 以外に `qrels.rs` / `scatter.rs` / `gate.rs` / `search.rs` / `systems.rs` / `run.rs` / `cli.rs`）、
  新規 `src/bin/kaseifu_bench.rs`、新規 `tests/v090_trec_benchmark_test.rs`、
  `src/lib.rs`（モジュール宣言 1 行）、`Cargo.toml`（`[[bin]]` 1 ブロック）、`.gitignore`（作業データ保護 3 行）、
  `src/discovery/mod.rs`（re-export 1 行）
  - 実装時に確定した差異: **`src/platform/macos/spotlight.rs` は編集しない**。隔離検索（`mdfind -onlyin` + timeout）は
    ベンチ専用の `src/benchmark/search.rs` に置いた（共有ファイルの編集を 1 件減らし、製品コードに未使用の公開 API を増やさない）
- 対象外（実装しない）: コーパスを用いた本評価の実行とその数値、代用データでの実行、専用 macOS ユーザー作成、`sandbox-exec`、
  macOS VM、既存 Discovery / ランキング / スコアリングのロジック変更、`src/main.rs` の編集、README / CHANGELOG、
  `20261014a.md` 側（Swift / Foundation Models / リネーム）、`src/semantic/**` / `swift/**` / `build.rs` / `schemas/fm-*.json`（並行作業者が担当）
- 版数接頭辞: 新規テストは **`v090_*`**（次期マイナー想定。v070/v080 の前例に倣う）
- 制約: 新規 crate 依存なし・BM25 はリポジトリ内実装・新規設定ファイル/DB/独自キャッシュなし・新規 CC16+ なし・
  既存最大 CC を悪化させない・`cargo fmt --all -- --check` 違反 0・`clippy -D warnings` 警告 0・既存 450 テスト全通過・
  ASCII のみ・em dash なし・TUI なし・秘密情報なし・TREC データを Git にコミットしない・commit / push / tag は親が実施
- ベースライン実測（2026-10-14、HEAD `9be5f71`、clean）: `cargo test --all-targets --all-features` = **450 passed / 0 failed**、
  `bash scripts/quality_check.sh` = **OVERALL: PASS**（rustfmt 0、clippy 0、CC>=16 = 0、最大 CC 15、coverage 85.26% report-only）
- 確認手段: serena / context7 / codebase-memory-mcp / graphify は本環境で未接続のため、LSP 相当の直接読解と
  `grep`（`src/discovery/**`、`src/models/{similar_file,file_context}.rs`、`src/platform/macos/spotlight.rs`、
  `src/classifier/pipeline.rs`、`src/context/**`、`src/normalize/**`、`src/lib.rs`、`src/main.rs`、`Cargo.toml`、
  `scripts/quality_check.sh`、`tests/**`、`.gitignore`）および実機 `mdfind` / `curl` / `awk` で確認した

## 1. 背景と現状実測（変更前）

| ID | 現状（実測・コード根拠） | 本計画での扱い |
|---|---|---|
| P0-1 | Discovery は**文書単位のスコアを既に持つ**が、戻り値はフォルダ集約のみ。`discover_with` の内部 `similar_files`（`src/discovery/mod.rs:78-85`）は `SimilarFile.similarity_score` を持つ文書候補列。`pub` なのは `find_similar_files` / `extract_terms` / `normalize_terms`（`src/discovery/mod.rs:19-20`） | 文書レベルの出口として `find_similar_files_with` を re-export 1 行で公開し、**同じ手順・同じ関数**で評価用に取り出す（§3-1） |
| P0-2 | その文書スコアは**ファイル名と拡張子のみ**から作られる: `score_file` = 0.60*filename_similarity + 0.25*token_jaccard(ファイル名トークン) + 0.15*extension_match（`src/discovery/scoring.rs:3-5,190-192`）。本文は**ランキングに一切入らない** | 原典 §9 の「異なる情報源を利用する手法間の比較」に該当する事実として、出力 JSON と計画に明記。ロジックは変更しない |
| P0-3 | 本文は**検索（再現率）側では使われる**: mdfind の語クエリ（`src/discovery/similarity.rs:143-149`）が Spotlight の全文索引に当たる | kaseifu の「本文利用」は検索のみ・ランキングはファイル名のみ、と切り分けて記録 |
| P0-4 | `mdfind -onlyin` は**コードベースに存在しない**（`grep -rn onlyin src/` = 0 件）。`spotlight::search` は `mdfind <query>` を無制限スコープで実行（`src/platform/macos/spotlight.rs:119-132`） | 新関数 `search_onlyin` を追加（既定 `search` は不変）。ベンチは常に `-onlyin` 経路（§3-2） |
| P0-5 | 原典 §5 が指定する `~/Library/Application Support/kaseifu/benchmark/` は、**kaseifu 自身の除外規則で丸ごと落ちる**: `is_excluded_destination` は先頭 3 コンポーネント内の `Library` を除外（`src/discovery/destinations.rs:30-33,112-117`）、この判定はヒット段（`src/discovery/similarity.rs:70`）と集約段（`src/discovery/candidates.rs:34`）の両方で効く。既存テストも `~/Library/Application Support/...` を除外例として固定（`src/discovery/destinations.rs:234`） | コーパスは `~/Documents/kaseifu-benchmark/` に置く（§3-3）。`check-index` が除外規則違反を機械的に検出 |
| P0-6 | 集約段は**入力自身の親ディレクトリを候補から除外**する（`src/discovery/mod.rs:106` + `src/discovery/candidates.rs:28-30`） | 文書レベル出口では継承しない（移動先ポリシーであって関連性ではない）。Flat 配置が成立する根拠（§3-1） |
| P0-7 | 生ヒットは `MAX_SPOTLIGHT_HITS = 10_000` で打ち切られ（`src/discovery/similarity.rs:46`）、フォルダ候補は `MAX_CANDIDATE_DESTINATIONS = 50` で打ち切られる（`src/discovery/mod.rs:111`） | 文書数え上げに効く既知の上限として記録。評価用に**変更しない**（打ち切りは既存挙動） |
| P0-8 | 実機: `~/Documents/kaseifu-benchmark/` も `~/Library/Application Support/kaseifu/benchmark/` も `mdfind` に出る（name / content 両クエリで実測）。`/tmp` 配下は `mdfind -onlyin /tmp` = 0 件（既知の実測と一致） | 除外理由は「索引されないから」ではなく「kaseifu の除外規則に当たるから」。索引確認コマンドで毎回実測する |
| P0-9 | qrels は公開・直接取得可能（実測）: `https://trec.nist.gov/data/robust/qrels.robust2004.txt` = HTTP 200、**6,543,541 バイト**、sha256 `f8f2c972d3c710d85daa7ead02daf4ffe2bbe3647c9f3904500182f43ddbf4c3`、311,410 行、4 フィールド `TOPIC 0 DOCNO REL`、`REL ∈ {0,1,2}`、トピック **249 種**（301--450 / 601--700 の 250 のうち **672 が欠番**）、正判定 17,412 行、正判定文書 15,940 種、判定済み文書 174,787 種。`REL` 分布 = {0: 293,998 / 1: 16,381 / 2: 1,031}。正判定 docno の接頭辞 = FT 6,198 / FBIS 5,180 / LA 4,887 / **FR 1,147** | `prepare` の入力。実データで決定性を検証（§9） |
| P0-10 | 正判定に **Federal Register (FR) の文書が 1,147 件含まれる**（実測）。一方、ユーザー側の 472,525 文書という前提は FR を除いた数（§2.2） | FR を除外したコーパスを使う場合、正判定 1,147 件の文書が不在になる。`scatter` は**配置できなかった doc_id を manifest に記録**し、`run` は入力不在を `skipped_missing_input` として数える（評価分母に混ぜない）。§2.2・§6 に反映 |

## 2. Phase 0 調査結果（一次情報）

### 2.1 コーパスの現行の正式な取得手続き（一次情報）

原典 §2 の「開発開始時に、利用可能な公式配布元、取得権限、データ形式を確認すること」に対する回答。
旧 URL `https://trec.nist.gov/data/cdrom2.html` は 404 だが、現行は**別 URL で存在する**。

| 項目 | 内容（一次情報） |
|---|---|
| 配布元 | `https://trec.nist.gov/data/cd45/index.html`（「TREC disks 4 and 5」。ページ作成 2019-02-05、リンク元は `https://trec.nist.gov/data.html` の「Ad hoc Test Collections」） |
| 必要書類 | **Organizational agreement**（所属組織のデータ責任者が署名し NIST へ送付）と **Individual agreement**（利用研究者全員が署名し**組織内で保管**。NIST へは送らない）。様式は `https://www.nist.gov/document/trecorghtm`（Organization 用）と `https://duc.nist.gov/forms/ind_appl_trec.html`（Individual 用） |
| 提出方法 | 「Send a scan of the Organizational form to NIST to: **trec-data-admin@nist.gov**」。メール件名は `request for TREC disks 4 and 5` |
| 納品方法 | 「Subject to our approval, a **download URL, login, and password will be sent via email**」（電子納品。物理媒体ではない） |
| 所要期間 | 「Please allow **seven business days** for a response.」 |
| 費用 | 一次情報に**費用の記載はない**（有償の記述なし。copyright のライセンス条件のみ） |
| 利用条件 | 「Some of the documents in the data set are copyrighted by the original source of the material and thus the documents must be licensed for **research use**」。NIST SRD ページ（`https://www.nist.gov/srd/trecinc`、2025-08-27 更新）は「TREC has been discontinued」「forms must be signed and returned to NIST before you can receive the disks」と補足 |
| 構成 | FT(1991--94) / Congressional Record 103rd(1993) / Federal Register(1994) / FBIS(1996) / LA Times(1989,90) |

### 2.2 コーパスの規模（一次情報と導出）

原典 §2 とユーザー側の事前調査（「CR/FR を除いて 472,525 文書」）を一次情報で裏取りした。TREC 2004 Robust Track ガイドライン
（`https://trec.nist.gov/data/robust/04.guidelines.html`）の Test Data 表:

```
Source                # Docs    Size (MB)
Financial Times       210,158   564
Federal Register 94    55,630   395
FBIS, disk 5          130,471   470
LA Times              131,896   475
Total Collection:     528,155  1,904
```

- Robust04 の定義は「TREC Disks 4 and 5 **minus the Congressional Record**」= **528,155 文書 / 約 1,904 MB（約 1.9 GB）**。
- ユーザー側の 472,525 = 528,155 - 55,630（Federal Register を追加で除いた数）。**導出として一致**する。
  どちらを使うかで文書数・必要ディスクが変わるため、`validate` / `scatter` は件数を実測して記録し、原典の要件（本文無改変）に影響しない範囲で**そのまま使う**。
- 実行上必要: 展開後 1.9 GB 以上 + 散乱コピーで同量（合計 4 GB 前後）。空き 81 GiB で足りる。
- **どちらを取得するかで D2D の適用範囲が変わる**: 正判定 17,412 件の内訳は FT 6,198 / FBIS 5,180 / LA 4,887 / FR 1,147（実測）。
  FR を除いた 472,525 文書版では FR の正判定文書が入力にも候補にも現れない（実装は不在を記録して分母から外す。P0-10）。
- トピックは 250（301--450 / 601--650 / 651--700）。601--700 は 3 段階判定（not relevant / relevant / highly relevant）だが、
  実 qrels の `REL` は `{0,1,2}` で、旧トピックとの互換のため「1 と 2 を関連として扱う」とガイドラインに明記されている。
- ガイドラインは「the actual relevance judgments may not be used during the processing of the submitted runs」とも述べており、
  原典 §2「文書本文や qrels を kaseifu の検索ロジックに埋め込まない」と同じ趣旨。

### 2.3 文書レベルランキングの取り出し方（実測にもとづく確定）

原典 §8 の二重拘束（「文書候補のランキング結果を評価用に取り出す」「フォルダランキングを D2D スコアとして代用してはならない」「ランキングロジックを変更しない」）
への回答。**Discovery の内部には既に文書単位のスコアがある**（P0-1/P0-2）。既存コードの出口は 3 層に分かれている:

| 層 | 既存コード | 内容 | 本ベンチでの扱い |
|---|---|---|---|
| L1 検索 | `build_search_query` + `spotlight::search`（`src/discovery/similarity.rs:42-46,143-149`） | mdfind 語クエリ。本文・メタデータの全文索引に当たる | **そのまま再利用**（検索語は `extract_terms` → `normalize_terms`、`src/discovery/mod.rs:68-69` と同一） |
| L2 文書スコア | `scoring::score_file`（`src/discovery/scoring.rs:175-201`）→ `SimilarFile.similarity_score` | ファイル名/拡張子ベースの文書スコア | **そのまま再利用**。値の再計算・再重み付けはしない |
| L3 フォルダ集約 | `aggregate_by_directory_with_exclusions`（`src/discovery/candidates.rs:14-116`） | ディレクトリ単位のランキング（age 補正・ディレクトリ名のみの順序付け） | **使わない**（原典 §8 が明示的に禁止） |

確定した取り出し方（`kaseifu` システム）:

1. `context::builder::build(path)` → `normalize::normalize_with_options(.., MetadataOptions::default())`（`src/classifier/pipeline.rs:226-227` と同じ手順）
2. `discovery::extract_terms` → `discovery::normalize_terms`（`src/discovery/mod.rs:68-69` と同一）
3. `discovery::find_similar_files_with(terms, filename, ext, path, parent, scoped_search)` (`src/discovery/mod.rs:78-85` と同関数・同引数)
4. 得た `Vec<SimilarFile>`（文書候補）を**既存の並び規約で並べる**: スコア降順 → 古い証拠優先 → パス昇順
   （規約は `src/discovery/candidates.rs:89-101` のディレクトリ順序と同一。スコア計算は一切しない）
5. 1 位のパスを scatter manifest で doc_id に逆引きして `predicted_doc_id` とする

**足りない点（原典「変更せずに報告」に該当。今回は変更しない）**:

- **Discovery には本文を使う検索語生成が存在しない**。検索語になるのは「ファイル名ステム由来の `F:` 信号」（`src/normalize/mod.rs:51-68` + `split_case` `src/normalize/mod.rs:150-180`）と Spotlight メタデータ由来の `M:`/`K:` 信号だけである。`grep -rn 'read_to_string|File::open' src/` は `src/config/loader.rs` / `src/completion/mod.rs` 以外にヒットせず、**入力ファイルの本文を読む経路は無い**。
  したがって TREC コーパスでの kaseifu のクエリは「入力ファイルの名前の語」（例: 元の docno 名なら `FBIS3 10082`、原典 §15.3 の不透明名なら `7f82a91c`）であり、
  Spotlight の全文索引に当たる語が**内容語にならない**。この条件下では、自分自身を除いた後に候補が残らず、`coverage = 0` / `hit_at_1 = 0` となる**可能性が高い**（実コーパス未取得のため予測であり、実測ではない。
  一部は、TREC 文書本文に `<DOCNO>` が残っていてもクエリ語がその文書自身にしか当たらないため同じ帰結になる）。
  これは評価基盤の不具合ではなく**現行 Discovery の能力境界**であり、本文を情報源にするには `search_terms` / `normalize` のロジック変更が必要になる（= 原典 §8 が禁じている変更なので**今回はやらない。ユーザー判断が要る論点**）。
- この帰結を隠さないため、結果 JSON に `diagnostics` を出す: `inputs_with_empty_terms`（正規化後の検索語が 0 件の入力数）、
  `inputs_with_zero_candidates`、`inputs_all_zero_candidates`。coverage 0 の原因を数値で切り分けられるようにする。
- Discovery に**本文類似度スコアが存在しない**。したがって TREC 本文に対する関連性は、ファイル名が情報を持つ条件ではファイル名由来の信号になり、
  散乱条件（不透明名）ではほぼ定数（`extension_match` のみ）になる。
  この帰結は原典 §15.9（「ファイル名をランダム化すると、ファイル名の類似度を利用する kaseifu の性能が低下する可能性がある。
  これは不具合ではなく…本文や検索語から関連ファイルを発見できるかを切り分けるための実験条件」）と整合する。
- 加えて、**検索語とランキングの情報源が一致していない**: 語が当たれば（本文のどこかに入力ファイル名の語があるファイルを）拾えるが、
  スコアはそのファイルの**名前**だけで付く。原典 §9 の「異なる情報源を利用する手法間の性能比較」に該当する。
- 文書レベルの**順序**を与える既存関数は無い（`find_similar_files_with` は mdfind 順のまま返す）。
  順序は上記 4 の 1 箇所だけベンチ側で定義する。**新しいスコア式は導入しない**。
- 並びが同点のときは最終キーがパスなので、不透明名 + 同一拡張子の条件では実質「パス昇順」で決まる（決定的だが無情報）。
  この性質は結果 JSON の `constraints` に記録し、スコアの解釈に使う。

### 2.4 Spotlight 制約（実測）

- `/tmp` 配下は `mdfind -onlyin /tmp zetaquartz` = **0 件**（実測）。隠しディレクトリも同様（過去ラウンドの実測）。
- `~/Documents/kaseifu-benchmark/corpus/probe-alpha.txt` は name クエリ・content クエリの両方で発見できた（実測、配置 10 秒後）。
- `~/Library/Application Support/kaseifu/benchmark/corpus/probe-beta.txt` も `mdfind` では発見できた（実測）。つまり
  **索引の可否ではなく kaseifu の除外規則（P0-5）が理由で不適**。原典 §5 のパスをそのまま使うと、kaseifu は全入力で候補 0 になる。
- 索引反映は非同期。原典 §7 の 5 段階（配置 → 状態確認 → 代表文書が発見できることの検証 → 準備完了 → 未完了なら中止）を
  `check-index` として機械化し、`run` は準備未完了なら**開始しない**。

### 2.5 ビルド・配布への影響（2 つ目のバイナリ）

- 原典 §11 の `kaseifu-bench` を**別バイナリ**として追加する（理由は §3-5）。`src/main.rs` は編集しない。
- `cargo test --all-targets --all-features` は新バイナリもビルドするため、ベンチ側のコードは**コンパイル時にデータを要求しない**（実行時のみ）。
- `cargo install` は既定で全 bin を入れるため `kaseifu-bench` も入る。研究・評価用インターフェースである旨を `--help` 本文に書く（README は今回触らない）。
- coverage（report-only）の分母が増える。ゲートではない。

## 3. 設計判断（選択肢・却下案・採用理由）

| # | 論点 | 採用 | 却下案と理由 |
|---|---|---|---|
| 1 | 文書レベルランキングの取り出し | §2.3 の 5 段階。`find_similar_files_with` を re-export 1 行で公開し、L1/L2 をそのまま再利用、L3 は使わない | (a) フォルダランキングを使う = 原典 §8 が明示的に禁止。(b) Discovery を書き換えて文書ランキングを返す = 「ランキングロジックを変更しない」に違反し並行作業とも衝突。(c) 新規スコア式 = 同じく違反 |
| 2 | 隔離の実現 | `search_onlyin`（新規、`mdfind -onlyin`）を `find_similar_files_with` の注入経路に渡す。**既定経路（`search`）は不変**。加えて結果の各パスを正規化（`canonicalize`、失敗時は字句正規化）してコーパス根の内含を検査し、外は捨てる。入力文書もコーパス内含を要求 | `-onlyin` だけに頼る = symlink / 別名経路で外に出る余地が残る。既存 `search` の既定変更 = 通常利用の挙動を変えるため不可 |
| 3 | コーパス配置先 | `~/Documents/kaseifu-benchmark/`（作業領域全体の根）。`datasets/` `corpus/` `workspace/scattered/` `manifests/` `runs/` `reports/` | 原典 §5 の `~/Library/Application Support/kaseifu/benchmark/` は kaseifu の除外規則（`destinations.rs:30-33,112-117`）で候補が全滅するため不可（実測 + 既存テスト固定）。`/tmp`・隠しディレクトリは索引されないため不可 |
| 4 | BM25 と Random | BM25 はリポジトリ内実装: `k1=1.2`, `b=0.75`, `idf = ln(1 + (N - n_t + 0.5)/(n_t + 0.5))`, `score = Σ idf * f(t,d)*(k1+1) / (f(t,d) + k1*(1 - b + b*|d|/avgdl))`。トークンは ASCII 英数字の小文字化（`[^a-z0-9]+` 区切り）。索引は入力に依存せず 1 回構築。Random は SplitMix64 を `(seed, source_doc_id)` から初期化し、コーパス文書（自分自身を除く）から 1 件選ぶ（入力順に依存しない） | BM25 のクレート追加 = 依存追加禁止。Random のプロセス全体 PRNG = 入力順で結果が変わり再現性が弱い |
| 5 | 別バイナリか隠しサブコマンドか | **別バイナリ `kaseifu-bench`** | 隠しサブコマンド（`hide = true`）は `src/main.rs` の編集が必要（並行作業と衝突しうる）で、`classify`/`organize` の dispatch 表にも触れる。別バイナリは `src/main.rs` を触らず、原典 §11 の表記どおりで、ユーザー向けヘルプ・補完・README の契約テストに影響しない |
| 6 | 再現性記録の取得 | **実行時**に `git rev-parse HEAD` / `rustc --version` / `sw_vers` / `mdfind` を呼ぶ。`build.rs` は作らない（並行作業が作成中で衝突するため。原典外の制約） | `build.rs` で commit を埋め込む = 衝突。ビルド時情報は実行時取得で代替可能 |
| 7 | 処理量の扱い | `mdfind` 呼び出しは 1 入力 1 回。`--timeout-ms`（既定 30000）で**ベンチ自身の** mdfind を打ち切り（超過は候補 0 として記録し、JSON に `timeouts` を記録）。打ち切りは既存 `MAX_SPOTLIGHT_HITS` と別に効く。実コーパスが無いので机上の断定はしない | 無制限待ち = 数十万文書規模で停止し得る。既存 `search` に timeout を入れる = 通常利用の挙動変更 |

## 4. 振る舞い（BDD シナリオ / 具体例）

### C1. `validate`（データ検証）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C1-1 | 正常系 | qrels が存在・読める: 行数・トピック数・正判定数・欠番トピック・`REL` 分布・sha256・バイト数を表示。exit 0 |
| C1-2 | 異常系（qrels 不在） | `FAIL qrels: not found: <path>` を表示し exit 1。**架空データを作らない**（原典 §2） |
| C1-3 | 異常系（書式不正） | 4 フィールドでない行・`REL` が整数でない行を**行番号つきで**列挙し exit 1 |
| C1-4 | 正常系（コーパス未配置） | コーパスが無くても qrels 部分は PASS し、`SKIP corpus: not present (qrels-only work can continue)` を出す。exit 0 |
| C1-5 | 異常系（配置先が除外規則に当たる） | コーパス根が `~/Library` 配下など kaseifu の除外規則に当たる場合、`FAIL corpus root: dropped by discovery exclusion rule ...` を出して exit 1（理由と `destinations.rs` の行を添える） |

### C2. `prepare`（D2D 正解集合 + manifest）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C2-1 | 正常系（実 qrels） | 正判定のみ使用（`REL > 0`）。入力 A の正解集合 = A と少なくとも 1 つの正判定トピックを共有する他文書。`REL = 0` は使用しない（未判定を「関連なし」と断定しない） |
| C2-2 | 正常系（多対多） | 1 トピックに複数の正判定文書がある場合、全ペアが相互に正解になる（例: q=301 の正判定が {A,B,C} なら A の正解に B,C） |
| C2-3 | 正常系（自己除外） | どの入力の正解集合にも入力自身を含めない |
| C2-4 | 正常系（評価対象外） | 正解候補が 1 件も無い入力は `entries` に含めない（`excluded_no_ground_truth` に件数を記録） |
| C2-5 | 正常系（複数候補の保持） | 正解候補を 1 件に縮退させない。`ground_truth_size` と（閾値以下なら）`ground_truth_doc_ids`、常に `ground_truth_sha256` を保存 |
| C2-6 | 正常系（判定値の保存） | 入力自身の正判定を `{topic, rel}` として保存（将来の段階的評価用）。`summary.rel_histogram` も保存 |
| C2-7 | 決定性 | 同一入力（同一 qrels・同一 seed・同一 `--max-inputs`）から**バイト単位で同一**の manifest が生成され、`manifest_sha256` が一致する |
| C2-8 | エッジ（サンプリング） | `--max-inputs N`（既定 200）は SplitMix64 で決定的に選び、選んだ後 doc_id 昇順に整列する。`--max-inputs 0` は全件 |
| C2-9 | エッジ（manifest 破損） | `manifest_sha256` が合わない manifest は `run` が拒否して停止する |

### C3. `scatter` / `verify-scatter` / `cleanup`（原典 §15）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C3-1 | 正常系（flat / uniform） | `--strategy flat` は単一ディレクトリ、`--strategy uniform --seed 42 --directories 100` は `dir_0001`.. に決定的に配置。qrels を配置決定に使わない |
| C3-2 | 正常系（本文無改変） | コピー後のバイトが元と一致（sha256 / サイズで検証）。元データは削除・移動しない |
| C3-3 | 正常系（不透明名） | ファイル名は doc_id と seed から決定的に導出した 8 桁 hex + `.txt`（例 `7f82a91c.txt`）。docno 由来の文字列を含めない |
| C3-4 | 正常系（日時） | 全ファイルの mtime を固定値に設定（`libc::utimensat`）。設定不能な birthtime は「設定できない」事実として manifest に記録し、`mdls`/`stat` の実測値も記録 |
| C3-5 | 異常系（上書き拒否） | 既存ファイルがある場合は無条件上書きせずエラー（`create_new`） |
| C3-6 | 異常系（領域外・脱出） | 出力先が作業領域外、または symlink で外に出る場合は拒否 |
| C3-7 | 正常系（cleanup） | `--execute --yes` のときだけ、**manifest に記録した自分が作ったファイル**を削除。元データには触れない |

### C4. `check-index`（索引準備ゲート）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C4-1 | 正常系 | scatter manifest から決定的に N 件（既定 5）を選び、`mdfind -onlyin <root>` の name クエリで各文書が発見できることを確認。`ready: true`、exit 0 |
| C4-2 | 異常系（未準備） | 発見できない文書があれば `ready: false`、`FAIL index: ... not found by mdfind (Spotlight indexing is asynchronous; re-run later)` を出して exit 1。**run はこの状態で開始しない** |
| C4-3 | 異常系（コーパス不在） | `FAIL corpus: not present` で exit 1（評価できない） |
| C4-4 | 正常系（記録） | macOS 版（`sw_vers`）、`mdutil -s` の生出力、`~/Library/Application Support/...` を避けていること、除外規則に当たるディレクトリ数、確認時刻を出力し JSON に残す |

### C5. `run`（評価実行）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C5-1 | 正常系（kaseifu） | manifest の各入力について 文書 A → 候補文書列 → 1 位 B を選び、B が A の正解集合にあれば `hit=1`。`latency_ms` は入力 1 件の処理時間 |
| C5-2 | 正常系（結果なし） | 候補が 0 件の入力は `predicted_doc_id` を空・`hit=0`・`candidate_count=0` とし、**分母から除外しない**（原典 §4） |
| C5-3 | 正常系（指標） | `hit_at_1 = hits / evaluated`、`coverage = 結果を返せた入力 / evaluated`、`no_result_rate = 1 - coverage`、latency の代表値、candidate_count の代表値 |
| C5-4 | 異常系（索引未準備） | ゲート結果が `ready: false` なら**1 件も検索せずに**停止し exit 1 |
| C5-5 | 異常系（評価対象外パス） | コーパス外のパスが候補に混ざっても採用しない（正規化後の内含検査）。コーパス外の入力を渡された場合は拒否 |
| C5-9 | エッジ（入力文書がコーパスに無い） | manifest の入力のうち scatter manifest に実ファイルが無いものは `skipped_missing_input` として数え、`evaluated` に含めない（分母を汚さない） |
| C5-6 | 正常系（読み取り専用） | 実行前後でコーパスのファイル集合・サイズ・mtime・内容ハッシュが不変 |
| C5-7 | 正常系（JSON/CSV） | §6 のキーで `runs/<system>-<timestamp>.json` と `runs/<system>-<timestamp>.csv` を書く |
| C5-8 | 正常系（bm25 / random） | 同一 manifest・同一 seed から決定的な結果。BM25 は本文、Random は種、kaseifu は Discovery |

### C6. `compare`

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C6-1 | 正常系 | 各 system の最新 run を読み、Hit@1 / Coverage / No Result Rate / Latency / Candidate Count を ASCII 罫線で並べる。原典 §9 の「TREC 公式 Q2D スコアとは直接比較できない」旨を必ず表示 |
| C6-2 | 異常系 | run が 1 件も無ければ `no runs found in <dir>` を表示し exit 1 |

## 5. タスク一覧（BDD → 実装 → 検証）

| # | タスク | 期待する振る舞い（受け入れ条件） | 主な変更先 | テスト |
|---|---|---|---|---|
| T1 | モジュール骨格と作業領域 | `--workspace`（既定 `~/Documents/kaseifu-benchmark`）配下の 6 ディレクトリを解決。パス正規化と内含検査が §3-2 のとおり動く | `src/benchmark/mod.rs` | C5-5 |
| T2 | qrels 読み込み | 4 フィールド・`REL` 整数・正判定のみ。行番号つきエラー | `src/benchmark/qrels.rs` | C1-2/C1-3、§12-1 |
| T3 | D2D 正解集合生成 | C2-1..C2-6（多対多・自己除外・評価対象外・複数候補・判定値保存） | `src/benchmark/qrels.rs` | §12-2/3 |
| T4 | manifest 生成と決定性 | C2-7/C2-8。同一入力から同一バイト列、sha256 一致 | `src/benchmark/qrels.rs` | §12-8 |
| T5 | 文書レベル取り出し（kaseifu） | §2.3 の 5 段階。`discover_with` と同じ検索語・同じ関数を通る（クエリ文字列の一致で固定） | `src/benchmark/systems.rs` | §12-10 補助 |
| T6 | 隔離された検索 | `-onlyin` 経路と内含検査。既定 `search` は不変 | `src/platform/macos/spotlight.rs`, `src/benchmark/mod.rs` | §12-6 |
| T7 | 索引準備ゲート | C4-1..C4-4。未準備なら `run` が開始しない | `src/benchmark/gate.rs` | §12-9 |
| T8 | 指標 | C5-2/C5-3。結果なしは 0 で分母維持 | `src/benchmark/run.rs` | §12-4/5 |
| T9 | 出力（JSON/CSV）と再現性 | §6。git commit / macOS / Rust / 索引状態 / checksum / 日時 | `src/benchmark/run.rs` | C5-7 |
| T10 | Random / BM25 | §3-4 の式とシード方式。決定的 | `src/benchmark/systems.rs` | C5-8 |
| T11 | 散乱（§15） | C3-1..C3-7 | `src/benchmark/scatter.rs` | §12-7（無変更の固定） |
| T12 | CLI | 8 サブコマンド（validate / prepare / check-index / scatter / verify-scatter / cleanup / run / compare）。ASCII・色なし・非 TTY でも同じ | `src/benchmark/cli.rs`, `src/bin/kaseifu_bench.rs` | C1..C6 |
| T13 | 既存への配線 | `pub mod benchmark;` 1 行、`[[bin]]` 1 ブロック、`.gitignore`、re-export 1 行 | `src/lib.rs`, `Cargo.toml`, `.gitignore`, `src/discovery/mod.rs` | 既存 450 テスト |
| T14 | 計画・issues | 本計画書と 3 issues | `documents/plans/dev/**` | レビュー |

## 6. 出力契約

`runs/<system>-<YYYYmmddTHHMMSSZ>.json`（原典 §10 + §15.9）:

```json
{
  "benchmark": "trec-robust04-derived-d2d",
  "metric": "hit@1",
  "system": "kaseifu",
  "results": { "evaluated": 200, "hits": 0, "hit_at_1": 0.0, "coverage": 0.0 },
  "auxiliary": {
    "no_result_rate": 1.0,
    "latency_ms": { "mean": 0.0, "median": 0.0, "p95": 0.0, "max": 0 },
    "candidate_count": { "mean": 0.0, "median": 0.0, "max": 0 }
  },
  "diagnostics": { "inputs": 200, "inputs_with_empty_terms": 0, "inputs_with_zero_candidates": 200 },
}
  "scattering": { "strategy": "uniform", "seed": 42, "directory_count": 100,
                   "filename_strategy": "deterministic-opaque", "timestamp_strategy": "fixed" },
  "reproducibility": {
    "kaseifu_commit": "9be5f71...", "dataset": { "name": "TREC Robust04 qrels", "version": "robust2004",
      "source_url": "https://trec.nist.gov/data/robust/qrels.robust2004.txt", "sha256": "...", "bytes": 6543541 },
    "corpus": { "files": 0, "bytes": 0, "tree_sha256": "..." },
    "manifest": { "path": "manifests/d2d-....json", "sha256": "..." },
    "macos": { "product_version": "26.4.1", "build_version": "25E253", "arch": "arm64" },
    "rust": "rustc 1.98.1 ...", "spotlight": { "ready": true, "checked_at": "...", "mdutil": "..." },
    "settings": { "seed": 42, "timeout_ms": 30000, "max_spotlight_hits": 10000, "search_scope": "...",
                  "image_metadata": { "spotlight": true, "exif_fallback": true } },
    "started_at": "...", "finished_at": "...", "timeouts": 0
  },
  "constraints": [ "...原典 §9 / §14 の 3 制約 + ファイル名のみランキングの事実..." ]
}
```

`runs/<system>-<YYYYmmddTHHMMSSZ>.csv`（列は原典 §10 の 5 列を固定、追加列なし）:

```csv
source_doc_id,predicted_doc_id,hit,latency_ms,candidate_count
```

`manifests/d2d-<seed>-<max>.json`:

```json
{
  "benchmark": "trec-robust04-derived-d2d", "schema_version": 1,
  "dataset": { "name": "TREC Robust04 qrels", "source_url": "...", "sha256": "...", "bytes": 6543541,
               "topics_total": 249, "topics_missing": [672], "qrels_lines": 311410, "positive_judgments": 17412,
               "rel_histogram": { "0": 293998, "1": 16381, "2": 1031 }, "judged_docs": 174787, "positive_docs": 15940 },  // 実測値
  "derivation": { "rule": "share >=1 positively judged topic", "positive_only": true,
                  "self_excluded": true, "negative_judgments_used": false, "graded_values_saved": true,
                  "note": "not a direct human document-to-document judgment" },
  "sampling": { "seed": 42, "requested": 200, "selected": 200, "algorithm": "splitmix64-shuffle-v1" },
  "excluded_no_ground_truth": 0,
  "entries": [
    { "source_doc_id": "LA010189-0001", "positive_topics": [ { "topic": 301, "rel": 1 } ],
      "ground_truth_size": 70, "ground_truth_sha256": "...", "ground_truth_doc_ids": [ "..." ] }
  ],
  "manifest_sha256": "..."
}
```

`ground_truth_doc_ids` は `ground_truth_size <= 256` のときだけ埋める（それ以上は sha256 と件数で固定）。
`manifest_sha256` は自身の `manifest_sha256` フィールドを除いた正規化 JSON の sha256。

## 7. 対象ファイル / 対象外ファイル

対象（新規）: `src/benchmark/{mod,qrels,scatter,gate,systems,run,cli}.rs`、`src/bin/kaseifu_bench.rs`、`tests/v090_trec_benchmark_test.rs`、
本計画書、issues 3 件。

対象（既存・最小差分）: `src/lib.rs`（+1 行）、`Cargo.toml`（`[[bin]]` 1 ブロック）、`.gitignore`（作業データ保護 3 行）、
`src/discovery/mod.rs`（re-export に `find_similar_files_with` を 1 行追加。**ロジック変更なし**）。
`src/platform/macos/spotlight.rs` は**編集しない**（§1 の対象リストの注記を参照）。

対象外: `src/main.rs`、`README.md`、`CHANGELOG.md`、`build.rs`、`swift/**`、`src/semantic/**`、`schemas/fm-*.json`、
`tests/v090_fm_bridge_test.rs`、`documents/plans/user/**`、`documents/plans/dev/20261014f-ai-rename-p0.md` とその issues、
既存の Discovery / ランキング / スコアリング / organize / watch / classify / inspect / doctor / undo / uninstall の挙動。

## 8. 原典 §14 の 3 制約の明記方法

出力 JSON の `constraints` と計画・issue に同じ文面を置く（`constraints.rs` の定数 1 箇所から供給し、テストで存在を固定する）:

- ① qrels は網羅的な正解一覧ではない（未判定文書が実際は関連している可能性があり、不正解に未判定の関連文書が混入しうる）。
- ② 共通クエリ由来の関連性は直接の文書間類似度ではない（測っているのは「共通の情報要求に関連する別文書を選択する能力」）。
- ③ 日付・ファイル名などのメタデータによる交絡があり得る（本文の意味的関連性として解釈するにはメタデータ寄与の別途検証が必要）。
- ④（§9 由来）kaseifu の文書スコアはファイル名と拡張子のみから作られ、検索語もファイル名/メタデータ由来であるため、
  BM25 比較は同一条件のアルゴリズム比較ではなく情報源の異なる手法の比較である。
- ⑤（実測ベースの予測）TREC コーパスのようにファイル名が内容語を持たない条件では、kaseifu の検索語が内容に当たらない可能性が高く、
  `coverage` が 0 に近づきうる。この場合の Hit@1 = 0 は「検索して外した」ではなく「検索語を作れなかった」を意味する。

## 9. 検証計画

1. `cargo fmt --all -- --check` = 違反 0
2. `cargo clippy --all-targets --all-features -- -D warnings` = 警告 0
3. `cargo test --all-targets --all-features` = 既存 450 + 新規すべて pass / 0 failed
4. `bash scripts/quality_check.sh` = **OVERALL: PASS**、CC>=16 = 0、最大 CC が既存 15 を超えない（新規関数は CC<=15）
5. 実データ検証（手動・証跡を報告に貼る）: 実 qrels に対して `validate` → `prepare --max-inputs 200` を 2 回実行し `manifest_sha256` の一致を確認。
   `--max-inputs 0` の全件 manifest も 1 回生成し件数を記録
6. 実機 `check-index`: コーパスが無いので「コーパス不在」で exit 1 になることを確認（失敗経路の実測）。加えて作業領域の可視性プローブで
   「`~/Documents/kaseifu-benchmark/` は索引される」「`~/Library/...` は kaseifu の規則で落ちる」を出力
7. 読み取り専用の固定: フィクスチャ・コーパスに対して `run`（検索注入）前後のファイル集合 / サイズ / mtime / sha256 の一致
8. CI 安全性: 新規テストは実 qrels・実コーパス・ネットワーク・実 HOME を使わない（隔離 HOME + 一時ディレクトリ + 注入経路のみ）

## 10. 今回の到達点と残り（コーパス入手後に 1 コマンドで本評価まで）

到達点: 評価基盤（qrels → D2D manifest → 指標 → 出力 → ベースライン → CLI → テスト）と Phase 0 調査。
**実コーパスが無いため、本評価の実行とスコア数値は今回の成果物に含めない（代用データでの実行もしない）。**

ユーザーが行う作業（§2.1）:

1. Organizational agreement を印刷・記入・署名し、スキャンを `trec-data-admin@nist.gov` へ送る（件名 `request for TREC disks 4 and 5`）。
2. Individual agreement を記入し、組織内で保管する（NIST へは送らない）。
3. 返信（目安 7 営業日）で届く download URL / login / password でコーパスを取得する。
4. 展開したデータを `~/Documents/kaseifu-benchmark/datasets/robust04/` に置く（**Git には入れない**。`.gitignore` でも保護）。

コーパス入手後の手順:

```bash
cargo build --release --bin kaseifu-bench
./target/release/kaseifu-bench validate
./target/release/kaseifu-bench prepare --max-inputs 200     # qrels のみで可能（今回検証済み）
./target/release/kaseifu-bench scatter --strategy uniform --seed 42 --directories 100
./target/release/kaseifu-bench check-index                  # 未準備なら run は開始しない
./target/release/kaseifu-bench run --system kaseifu
./target/release/kaseifu-bench run --system bm25
./target/release/kaseifu-bench run --system random
./target/release/kaseifu-bench compare
```

残るリスク: (a) 本評価のスコアは未測定（コーパス依存）、(b) ファイル名のみのランキングという性質上、散乱条件では kaseifu が無情報に近い挙動になりうる（§2.3。原典 §15.9 が想定済み）、
(c) 528,155 文書規模の BM25 は索引をメモリに持つため、実行時メモリは未測定、(d) Spotlight の索引状態は環境依存（`check-index` と `reproducibility.spotlight` で毎回記録）。

## 11. 実測記録（実装後・2026-10-14、HEAD `9be5f71`、macOS 26.4.1 build 25E253 arm64）

### 11.1 品質ゲート

```
rustfmt                PASS   violations 0
clippy                 PASS   warnings 0
cargo-test             PASS   passed 512, failed 0
complexity             PASS   analyzer 0.0.25
baseline-gate          PASS   no new/worsened CC>=16
report-tests           PASS   complexity_report self-tests green
coverage               PASS   report-only (line 81.59%)
OVERALL: PASS

Cyclomatic Complexity: functions 935 / CC<=10 926 (99.0%) / CC 11-15 9 / CC>=16 0 / max CC 15 / avg 2.87
```

- 変更前ベースライン 450 passed に対し **512 passed / 0 failed**（+62 = 新規テスト関数 62 個: `src/benchmark/**` 45 + `tests/v090` 17）。
- coverage は report-only のためゲート外。85.26% → 81.59%（新規モジュールの CLI ハンドラ層は実出力の確認を手動で行い、自動テストは
  parsing / help / 純関数が中心のため）。

### 11.2 公開 qrels（実データ）での決定性

```
$ kaseifu-bench --workspace ~/Documents/kaseifu-benchmark validate
PASS qrels: .../qrels.robust2004.txt (6543541 B, sha256 f8f2c972...f4c3)
     lines 311410 / topics 249 / missing [672] / positive judgments 17412 / positive docs 15940 / judged docs 174787
     relevance grades: {"0": 293998, "1": 16381, "2": 1031}

$ kaseifu-bench prepare --max-inputs 200        # 1 回目 / 2 回目で同一
inputs: 200 selected / 15940 eligible (seed 42, algorithm splitmix64-ranked-selection-v1)
manifest sha256: 6eb758ca2f97e29edb3e5fb55c1f9271fdda51613bf25315f9f43800abf4920
$ cmp d2d-seed42-200.json（1 回目） d2d-seed42-200.json（2 回目） -> バイト単位で同一

seed 42 / --max-inputs 100 -> manifest sha256 802e5031...（別物）
seed 43 / --max-inputs 200 -> manifest sha256 720b5540...（別物）
--max-inputs 0（全 15940 件） -> 15940 selected / 0 excluded / 38.8 MB / sha256 86b2b073... / 1.54 秒
```

独立検証（Python で qrels から再導出し、manifest と突き合わせ）:

```
manifests/d2d-seed42-200.json: entries=200 gt_size min/median/mean/max=8/147.0/165.2/703 embedded=155 over_limit=45
manifests/d2d-seed42-all.json: entries=15940 gt_size min/median/mean/max=2/141.0/160.0/1056 embedded=12674 over_limit=3266
  各 manifest から 60 件（先頭 40 + 末尾 20）を再導出し、ground_truth_size / ground_truth_sha256 / 埋め込み id 列 / 自己除外が全て一致
CROSS-CHECK: PASS
```

### 11.3 索引準備ゲート（実機）

```
$ kaseifu-bench check-index
corpus root: /Users/bookair18/Documents/kaseifu-benchmark/workspace/scattered
documents: 0 (corpus present: false)
macOS: 26.4.1 (build 25E253) aarch64
spotlight indexing: Some(true) / mdutil volume root: /: Indexing enabled. / data volume: /System/Volumes/Data: Indexing enabled.
FAIL index: no scattering manifest found: .../workspace/manifests/scattering.json
exit=1

$ kaseifu-bench run --system kaseifu
FAIL corpus: no scattering manifest at .../scattering.json (run scatter first; this needs the TREC corpus)
exit=1
```

- コーパス不在で **評価を開始しない**ことが実機で確認できた（原典 §7 の 5 番目）。
- `validate` は作業領域 `~/Documents/kaseifu-benchmark/` に対して除外規則の FAIL を出さない（= kaseifu が候補として扱える）。
  原典 §5 の `~/Library/Application Support/kaseifu/benchmark/` をコーパス根にすると `FAIL corpus root` で exit 1 になる
  （これを機械的に検出するのが `validate` / `check-index` の役割）。

### 11.4 実装中に見つかった既存実装の性質（未変更・要記録）

1. **`extract_terms` の順序は呼び出しごとに変わりうる**（`src/discovery/search_terms.rs:13` の `HashSet` を `into_iter()`）。
   同じ入力でも 2 回の呼び出しで `"1a6ac521 Text Plain"` / `"1a6ac521 Plain Text"` のように順序が異なる。
   mdfind の語クエリは AND なので**結果は不変**だが、クエリ文字列をそのまま再現性の証跡にするのは不適切。
   本実装は語の集合のみに依存し、テストは集合比較で固定する（`v090_the_discovery_search_path_is_the_product_search_path`）。
2. **macOS の一時ディレクトリはコーパス根に使えない**: `$TMPDIR`（`/var/folders/...`）は先頭コンポーネント `var` が
   `SYSTEM_ROOT_COMPONENTS`（`src/discovery/destinations.rs:61`）に入るため、kaseifu のヒットフィルタで全候補が落ちる。
   テストのフィクスチャは `CARGO_TARGET_TMPDIR` 下に置いた（`/tmp` が索引されない事実と合わせ、作業領域の選定根拠がさらに補強された）。
3. **不透明名 + 同一拡張子では文書スコアが情報を持たない（実測）**: 散乱コーパスの実測で 3 候補のスコアは
   `similarity_score == 0.60*filename_similarity + 0.25*0 + 0.15*1.0` の恒等式を満たし、`filename_similarity` は
   ランダム名の偶然の文字重なり（実測 < 0.5）でしかない。つまり順序は拡張子一致 + 偶然の名前類似で決まる。
   テスト `v090_opaque_names_leave_only_the_extension_score` がこの性質を固定する。
   → kaseifu の Hit@1 は **ファイル名情報が無い条件では偶然に近い**。これは不具合ではなく現行 Discovery の能力境界。
4. **パス正規化は「未作成のパス」も解決する必要がある**: `canonicalize` はパス全体の存在を要求するため、
   作成直前のファイルでは `/var/...` と `/private/var/...` が混ざって内含検査が誤って失敗する。
   `normalize_path` は「存在する最長の祖先を解決して残りを連結」する形にした（macOS の `/var` `/tmp` シンボリックリンク対策）。
   これは実装中に自分用のテストが検出したバグで、修正して単体テストで固定した。
