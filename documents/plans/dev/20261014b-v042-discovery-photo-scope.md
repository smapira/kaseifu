# v0.4.2 Discovery 候補品質改善（対象種別スコープ + 集約後切り捨て）BDD 計画書

原典: 親スレッドのユーザリクエスト（P1 2件、実機計測済み）。対象: worktree `kaseifu`、HEAD `ac494e7`（v0.4.1 後）。

本書は BDD タスク分解と検証エビデンスの記録である。実装・レビュー・QA の成果物は `documents/plans/dev/issues/` に issue 形式で保存する。

## 1. 問題の要約（v0.2.0 設計由来・v0.4.1 の回帰ではない）

実写真 `~/Desktop/images/IMG_3020.jpeg` の `discover` は候補 35 件すべてがコード/JSON/キャッシュで、写真フォルダが 1 つも出ない。原因は 2 つ:

1. **種別スコープなし** — `src/discovery/similarity.rs:42` が検索語を空白結合し、`src/platform/macos/spotlight.rs:119-131` が素の `mdfind <query>` に渡す。写真の検索語は `IMG`/`3020`（ファイル名）+ `image`（`kMDItemKind` = "JPEG image" 由来）で、全ファイル種別が対象になる
2. **切り捨てが集約前** — `similarity.rs:46` の `search(&search_query, 50)` と `spotlight.rs:131` の `stdout.lines().take(limit)` により、mdfind の出力順の先頭 50 件だけを採点する。画像はヒット末尾に埋もれる

## 2. 修正前の実測（2026-10-13、本スレッドで再取得）

| 計測 | コマンド | 結果 |
|---|---|---|
| 検索語（実写真） | `KASEIFU_DEBUG=1 kaseifu discover ~/Desktop/images/IMG_3020.jpeg` | `semantic_signals=[K:JPEG, K:image, F:IMG, F:3020]` → terms `[IMG, image, 3020]`、query `IMG image 3020` |
| raw 取得件数（同上） | 同 discover | **50**（上限で打ち切り、索引ヒットは 120） |
| 候補（写真） | 同 discover `--all` | **35 件**。D1=CapCut キャッシュ js、D2=sharetribe assets、D3=jupyter sample … 写真なし |
| 検索語（実 PDF） | `discover ~/Desktop/THCO-COA-Non-Detect Updated.pdf` | query `Non Detect THCO Updated COA` |
| raw 件数（PDF） | 同 discover | 25（上限未満。切り捨てなし。採点 21、候補 17） |
| 候補（PDF） | 同 discover `--all` | 17 件。**D1=`State Of Mind Labs` score 1.00 / `exact normalized filename`**（回帰基準点） |
| 実行時間・メモリ（写真） | `/usr/bin/time -l kaseifu discover …` | real 0.42s / max RSS 34.6 MB |
| システム上の IMG 系画像 | `mdfind 'kMDItemContentTypeTree == "public.image" && kMDItemFSName == "*IMG*"'` | **3311 件**（性能計測の基準規模） |
| 既存テスト | `cargo test --all-targets --all-features` | **252 passed / 0 failed** |

## 3. PoC: mdfind クエリ言語の実測（macOS 26.4.1 / BuildVersion 25E253）

**構文検証が必須**（動かない構文を採用しない）ため、実写真に対して以下を実測した。

| # | クエリ | ヒット | 判定 |
|---|---|---|---|
| Q1 | `IMG 3020 image`（現行・素の語 AND） | 120 | ベースライン |
| Q2 | `(IMG 3020 image) && kMDItemContentTypeTree == "public.image"` | — | ❌ `Failed to create query`（終了コード 1） |
| Q3 | `IMG 3020 image kMDItemContentTypeTree == "public.image"` | — | ❌ 同上 |
| Q4 | `kMDItemContentTypeTree == "public.image" && IMG 3020` | — | ❌ 同上 |
| Q5 | `kMDItemContentTypeTree == "public.image"` | 45091 | ✅ 構文可（スコープのみ） |
| Q6 | `kMDItemContentTypeTree == "public.image" && kMDItemFSName == "*IMG*"c && kMDItemFSName == "*3020*"c` | **1** | ✅ 構文可だが**実行不能**（自分自身のみ＝候補 0 件） |
| Q7 | `kMDItemContentTypeTree == "public.image" && (kMDItemFSName == "*IMG*"c \|\| kMDItemFSName == "*3020*"c)` | **3545** | ✅ **採用**（兄弟写真・写真フォルダを含む） |
| Q8 | Q7 + `kMDItemFSName == "*image*"c` | 3833 | ❌ 不採用（`/System/Library/…/ImageKit.framework`、`~/Library/Application Support/Anki2/…`、`…/save_image` 等のシステム/アプリ領域が 288 件増えるだけ） |
| Q9 | `kMDItemContentTypeTree == "public.image" && kMDItemFSName == "IMG*"c` | 3478 | 参考（兄弟写真は取れるが部分一致の一般性が落ちる） |

**PoC の結論**:

- 素の語と属性条件は**併記できない**（Q2–Q4。CLI は「語の列」か「式」のどちらかしか解釈しない）。したがって画像では語を**属性条件として表現**する
- 語を名前条件として **AND** すると写真では自分自身しか残らない（Q6: 系列 `IMG` と番号 `3020` の同時要求 = 同一写真）。**OR** にすると兄弟写真（`IMG_xxxx`）と写真フォルダが取れる（Q7）
- `kMDItemKind` 由来の `image` 語は画像では型ノイズで、システム領域・アプリキャッシュを増やすだけ（Q8）→ 画像クエリではファイル名に現れる語のみを使う
- Q7 の実データ内容（親ディレクトリ分布の上位）: `~/Desktop/images` 176、`~/OS/media/…/misc` 152、`…/images/Life` 54、`…/theory_of_instgram/mine` 39 など**写真・メディアフォルダが主**。`mdfind` 実行時間 0.459s

## 4. 確定仕様

### 変更1: 画像ターゲットの mdfind スコープ（`src/discovery/similarity.rs`）

- 画像（`crate::context::image_metadata::is_image_file` を再利用して拡張子で判定）のとき、クエリを以下にする:
  `kMDItemContentTypeTree == 'public.image' && (kMDItemFSName == '*<語1>*'c || kMDItemFSName == '*<語2>*'c || …)`
- 語は**検索語のうち対象ファイル名に含まれるもの**のみ（`kMDItemKind` 由来の `image` 等の型ノイズを自然に除外）。ソート + 重複排除で**決定的な文字列**にする
- mdfind のクエリメタ文字（`'` `"` `*` `?` `|` `&` など）を含む語は**スキップ**（クエリ破壊・意図しないワイルドカード拡張の防止）
- 画像でも上記語が 0 件のときは従来の語クエリへフォールバック（安全網）
- 非画像（PDF 等）は**完全に従来どおり**（`terms.join(" ")`）。種別述語を証拠なく追加しない

### 変更2: 切り捨てを集約の後へ（`src/discovery/similarity.rs` + `src/discovery/mod.rs`）

- `similarity.rs:46` の固定 `50` を廃し、名前付き定数 `MAX_SPOTLIGHT_HITS`（初期値 10_000）で全ヒットを取得（`search` 境界 `Fn(&str, usize)` は維持）
- 全ヒットを採点 → ディレクトリ集約 → ランキング → `MAX_CANDIDATE_DESTINATIONS`（初期値 50）で上位のみ返す。切り捨ての根拠は各定数の doc コメントに明記（性能・メモリ）

## 5. BDD タスク分解（振る舞い単位）

### T0 事前実測（済・§1–§3）

### T1 画像ターゲットのクエリスコープ（変更1）

- 振る舞い: 実写真（`IMG_3020.jpeg`）の `discover_with` が受ける mdfind クエリが `kMDItemContentTypeTree == 'public.image' && (…FSName… || …FSName…)` 形になる
- 正常系: 条件語 = ファイル名に現れる検索語のみ（`IMG`, `3020`）。`image` は含まれない
- 異常系: 画像でもファイル名に検索語がない → 従来の語クエリ（フォールバック）
- エッジ:
  - 語順・重複はソート + dedup で決定的（同一入力 → 同一クエリ文字列）
  - クエリメタ文字を含む語（例: `A&B`, `it's`）は条件から除外され、クエリが壊れない
  - 非画像（`.pdf`）→ クエリは `Non Detect THCO Updated COA`（不変）

### T2 切り捨て順序（変更2）

- 振る舞い: 注入 search が上限 50 件超（例 120 件）を返しても、集約・ランキングは**全件**に対して行われる
- 正常系: 良い候補が mdfind 出力順の後方にあっても候補に現れる（修正前は先頭 50 件で消える）
- 異常系: 探索上限（`MAX_SPOTLIGHT_HITS`）は注入 search の第 2 引数として渡る（実 Spotlight に依存しない）
- エッジ: 候補ディレクトリが上限超 → スコア降順の先頭 `MAX_CANDIDATE_DESTINATIONS` 件のみ返る
- データ例: PDF 回帰（`State Of Mind Labs` の完全一致 1 件を**末尾**に置いた 60 件シナリオでも D1 になる）

### T3 PDF 回帰（受入基準2）

- 振る舞い: 実 PDF で D1 `State Of Mind Labs` score 1.00 / `exact normalized filename` が従来どおり先頭
- 正常系: 実機 `discover` の before/after 比較（順位・スコア・reasons）
- エッジ: 変更2 により取得件数は増えない（当該クエリは 25 件 < 上限）

### T4 実写真の候補品質（受入基準1）

- 振る舞い: `discover ~/Desktop/images/IMG_3020.jpeg` の上位に写真を含むディレクトリが現れ、コード/JSON ディレクトリが上位を占めない
- 正常系: before/after の候補一覧（JSON）を保存して比較。上位 5 件が画像ディレクトリであること
- エッジ: 自身の親ディレクトリ（`~/Desktop/images`）は除外されたまま（設計どおり）
- 既知の残余（記録・スコープ外）: 名前類似度が高いアプリ内画像ストア（`~/Pictures/Eagle.library/images/<hash>.info/IMG_xxxx.jpg`）も上位に来る。パス除外の追加は本リクエストのスコープ外として記録する

### T5 性能・メモリ実測（受入基準4）

- 振る舞い: 画像 3311 件規模のクエリ（Q7 = 3545 件）で実行時間・最大 RSS を測る
- 計測: `/usr/bin/time -l` による before/after、`mdfind` 単体時間、raw/候補件数
- エッジ: 上限 `MAX_SPOTLIGHT_HITS` の根拠を数値で示す（ヒット数 × 1 パス ~100B、採点 1 件あたりの syscall 数）

### T6 テスト（受入基準3）

- 新規 `tests/v042_discovery_target_scope_test.rs`（実 Spotlight に依存しない。注入 search のみ）
  - T1: 画像クエリ形（スコープ + FSName 条件）／`image` を含まない／決定的／メタ文字除外
  - T2: 120 件注入で全件採点 → 後方の良候補が D1／limit 引数 = `MAX_SPOTLIGHT_HITS`／候補上限
  - T3: PDF 回帰（完全一致候補が D1 = score 1.0）
  - 回帰: v0.4.1 の「画像メタデータが検索語を変えない」テストは無変更で緑
- 既存 252 件を維持（`cargo test --all-targets --all-features`）

### T7 品質ゲート（受入基準3）

- `bash scripts/quality_check.sh` OVERALL PASS（fmt / clippy -D warnings / test / complexity / baseline-gate / report-tests / coverage）
- 新規 CC16+ 関数なし。意図的な複雑度変化がない限り `reports/quality/baseline.json` は更新しない
- 新規依存なし（Cargo.toml / Cargo.lock 不変）

### T8 ドキュメント

- `CHANGELOG.md` `[Unreleased]` に Fixed（Discovery の画像スコープと切り捨て順序）を追加
- `README.md` の discovery 記述がある場合のみ整合を確認（無ければ変更しない）
- 計画書・レビュー・QA の issue を `documents/plans/dev/issues/` に保存

## 6. 不変条件（厳守・テストで担保）

1. **Rust 側で撮影日時・GPS に適合度スコアを付けない**（撮影年の検索シード化も対象外）
2. 自身の親ディレクトリ除外（`src/discovery/mod.rs`）は設計どおり維持
3. 非画像ターゲットのクエリは不変（PDF 回帰: D1 = `State Of Mind Labs` 1.00）
4. `discover_with` / `find_similar_files_with` の注入 search 境界 `Fn(&str, usize)` を維持（テストは実 Spotlight 非依存）
5. ASCII 表示方針・NO_COLOR/非 TTY・TUI 禁止・JSON 互換を維持。`discover` の `#[command(hide = true)]` は変更しない
6. 秘密情報・位置情報をログ・コミット・レポートに含めない
7. 新規ファイルは `git add` して tracked にする。`git commit` / `push` / `tag` は行わない（親が実施）

## 7. 変更ファイル（予定）

- Modify: `src/discovery/similarity.rs`（クエリ生成 + 上限）、`src/discovery/mod.rs`（定数 + 候補上限）、`CHANGELOG.md`
- New: `tests/v042_discovery_target_scope_test.rs`, `documents/plans/dev/20261014b-v042-discovery-photo-scope.md`, `documents/plans/dev/issues/20261014-*.md`

`src/platform/macos/spotlight.rs` は `search(query, limit)` の境界を維持するため原則変更しない（`limit` の意味と `take` の位置は不変。呼び出し側が大きな上限を渡す）。

## 8. ツール根拠（推測でタスクを切らない）

- `read_file: src/discovery/similarity.rs` / `src/discovery/mod.rs` / `src/discovery/scoring.rs` — 検索 1 本・`terms.join(" ")`・集約前 `take(50)`・集約/ランキング/ID 採番を確認
- `read_file: src/platform/macos/spotlight.rs:119-131` — `mdfind` 1 コマンド + `stdout.lines().take(limit)` を確認
- `read_file: src/normalize/metadata.rs:44-49` — `K:`（kMDItemKind）由来の `image` 語の発生源を確認
- `read_file: src/context/image_metadata.rs:52-60` — `is_image_file`（拡張子判定）を再利用可能な既存ユーティリティとして確認
- `grep: find_similar_files` / `discover_with` — 呼び出しは `src/discovery/mod.rs` と `tests/phase4_*`、`tests/v0401_*` のみ（実 Spotlight を使うテストなし）
- `terminal: mdfind` 9 種（Q1–Q9）— クエリ言語の AND/OR・属性条件併記・`c` 修飾子・件数を実測
- `terminal: mdfind` 親ディレクトリ分布 — Q7 が写真フォルダを含むことを確認
- `terminal: /usr/bin/time -l kaseifu discover …` — 修正前の real / max RSS を取得
- `terminal: cargo test --all-targets --all-features` — 修正前 252 passed / 0 failed

## 9. 完了条件（ユーザ受入基準の対応）

1. T4: 実写真の上位に写真を含むディレクトリ（before/after 比較つき）
2. T3: 実 PDF で D1 `State Of Mind Labs` score 1.00 が先頭
3. T6/T7: 既存 252 件 + 新規テスト緑、`quality_check.sh` OVERALL PASS、新規 CC16+ なし、新規依存なし
4. T5: 使用クエリ文字列・ヒット件数・候補件数・実行時間・メモリの実測報告
- 付帯: T8 ドキュメント、`git add` のみ（commit/push/tag なし）

## 10. 完了記録（2026-10-13 実測）

### 10.1 実装

- `src/discovery/similarity.rs`: `build_search_query` / `image_scoped_query` / `filename_terms` / `is_query_safe` を追加。呼び出し上限を `MAX_SPOTLIGHT_HITS` に
- `src/discovery/mod.rs`: 名前付き定数 2 件（根拠を doc コメントに記載）+ 集約・ランキング後の `truncate`
- `tests/v042_discovery_target_scope_test.rs`: 新規 8 件（クエリ形 / 決定性 / メタ文字 / フォールバック / PDF 語クエリ / 切り捨て順序 / PDF 回帰 / 候補上限）
- `CHANGELOG.md` `[Unreleased]` Fixed 追加。新規依存なし。差分は src 2 ファイル +103/-7

### 10.2 受入基準の実測

| # | 基準 | 結果 |
|---|---|---|
| 1 | 実写真の上位に写真を含むディレクトリ | ✅ 修正後 D1=`~/Pictures/Eagle.library/images/<hash>.info`、D2=`~/OS/media/…/documents/media/misc`（n=152）、D3=`…/GM.Shop/images`（n=8）。修正前の D1–D3（CapCut js / sharetribe assets / jupyter sample）は全消滅 |
| 2 | PDF 回帰 | ✅ D1=`~/OS/…/State Of Mind Labs` score 1.0 / reasons 完全一致（候補 17 件・順序も同一） |
| 3 | テスト・ゲート | ✅ 260 passed / 0 failed（252 + 8）、`quality_check.sh` OVERALL PASS、新規 CC16+ なし（新規最大 CC 2）、新規依存なし |
| 4 | クエリ・件数の実測報告 | ✅ §10.3 |

### 10.3 クエリ・件数・性能（実行値）

| 対象 | 修正前 | 修正後 |
|---|---|---|
| 写真クエリ | `IMG image 3020` | `kMDItemContentTypeTree == 'public.image' && (kMDItemFSName == '*3020*'c \|\| kMDItemFSName == '*IMG*'c)` |
| 写真 raw / 採点 / 候補 | 50 / 49 / 35 | **3545 / 3526 / 50（上限）** |
| 写真 実行時間 / max RSS（5 回） | 0.22–0.24s / 34.0–34.5 MB | 0.93–0.97s / 18.3–19.3 MB |
| PNG クエリ | `IMG image 3267` | `kMDItemContentTypeTree == 'public.image' && (kMDItemFSName == '*3267*'c \|\| kMDItemFSName == '*IMG*'c)` |
| PNG raw / 候補 | 50（索引 610）/ 12–13 | 3545 / 50（上限） |
| PDF クエリ | `COA THCO Detect Updated Non` | 同一（不変） |
| PDF raw / 採点 / 候補 | 25 / 21 / 17 | 25 / 21 / 17（順位・スコア・reasons 一致） |

- `mdfind` 子プロセスの最大 RSS: 語クエリ 34.1 MB / 属性スコープクエリ 8.3 MB → `/usr/bin/time -l` の値は子プロセス支配。Rust 側増分は 3.5k パスの採点で数 MB 未満
- 上限 `MAX_SPOTLIGHT_HITS = 10_000` の根拠: 実測 3545 件（約 0.5 MB）+ 1 件あたり `exists`/`canonicalize`/`metadata` の 3 syscall → 最悪 10000 件でも ~0.5–1s 相当、メモリ ~1 MB のパス文字列

### 10.4 成果物

- `documents/plans/dev/issues/20261014-plan-architect-v042-review.md`
- `documents/plans/dev/issues/20261014-reviewer-v042.md`
- `documents/plans/dev/issues/20261014-qa-v042.md`
- 新規ファイルは `git add` 済（`git commit` / `push` / `tag` は親が実施）

### 10.5 残余（スコープ外・記録）

- アプリ内画像ストア（`~/Pictures/Eagle.library/images/<hash>.info`）が名前類似度で上位に混入。パス除外は本リクエストの 2 項目のスコープ外（次期検討候補）
- `discover --all` の表示は最大 50 件（ランキング上位）になる（意図的差分）
- 撮影年を検索シードに使う案は対象外（ユーザ指示どおり）
