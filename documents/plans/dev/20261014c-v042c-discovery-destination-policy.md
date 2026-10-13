# v0.4.2c Discovery 候補「移動先になれないディレクトリ」排除（規則ベース）BDD 計画書

原典: 親スレッドのユーザリクエスト（v0.4.2 の残余 P1、実機計測済み）。対象: worktree `kaseifu`、HEAD `ac494e7` + v0.4.2 未コミット差分（画像スコープ + 集約後切り捨て）。

本書は BDD タスク分解と検証エビデンスの記録である。実装・レビュー・QA の成果物は `documents/plans/dev/issues/` に issue 形式で保存する。

## 1. 問題の要約

実写真 `~/Desktop/images/IMG_3020.jpeg` の `discover` は候補 50 件中 **38 件が Eagle のアプリ内部ストア**（`~/Pictures/Eagle.library/images/<hash>.info`）で埋まる。これは「写真を含むディレクトリ」ではあるが、**1 写真 = 1 ディレクトリの内部実装**であり、ユーザーが写真の移動先として選ぶ場所ではない。原因は 2 つ:

1. **探索段階（`src/discovery/similarity.rs:176-184`）の除外判定が部分一致リスト（`/system,/private,/usr,/bin,/sbin,/var,/tmp`）しか持たない** — `~/Library/**`・`*.library/**`・隠しディレクトリ・キャッシュ・機械生成名は素通りする
2. **集約段階（`src/discovery/candidates.rs:13-29`）は「自身の親ディレクトリ」以外を一切除外しない** — ディレクトリ名が何であっても候補になる

## 2. 計測（2026-10-14 実測・修正前）

### 2.1 一次計測（実写真）

| 計測 | 結果 |
|---|---|
| mdfind クエリ | `kMDItemContentTypeTree == 'public.image' && (kMDItemFSName == '*3020*'c \|\| kMDItemFSName == '*IMG*'c)` |
| raw ヒット / 採点済み / 集約ディレクトリ / 返却候補 | **3545 / 3526 / 1363 / 50（上限）** |
| 候補 50 件のクラス内訳 | **アプリ内部ストア（Eagle `.info`）38 件**、ユーザーフォルダ 12 件 |
| raw ヒットのうち Eagle 配下 | **2276 件（採点対象の 65%）**、`~/Library` 配下 16 件 |

### 2.2 上位候補のスコア内訳（`KASEIFU_DEBUG=1` breakdown）

| 候補 | evidence | 代表ファイル | filename | token | ext | file score | base | age | bonus | dir score |
|---|---|---|---|---|---|---|---|---|---|---|
| D1 `~/Pictures/Eagle.library/images/LY2TLHNM2EO5K.info` | 1 | `IMG_5020.jpg` | 0.88 | 0.33 | 0.00 | 0.61 | 0.61 | ~845d | 1.00 | **0.71** |
| D4 `.../LMQG2H2X8WMBW.info` | 2 | `IMG_2020.png` + `_thumbnail.png` | 0.88 | 0.33 | 0.00 | 0.61 | 0.61 | ~1154d | 1.00 | 0.68 |
| D2 `~/OS/media/.../documents/media/misc`（実フォルダ） | 152 | `IMG_xxxx.jpg` | 0.75 | 0.33 | 1.00 | 0.61 | 0.61 | ~422d | 1.00 | 0.71 |

**Eagle が高スコアになる機序（計測で確定）**:

- Eagle は原本を `<hash>.info/` に「元のファイル名のまま」保存する（`IMG_5020.jpg` + `IMG_5020_thumbnail.png`）。
- 上位 38 件すべて **`*IMG*` 条件で到達**（`*3020*` では到達しない。`IMG_5020`/`IMG_2020`/`IMG_3060` のような別番号の兄弟写真が `IMG` だけでマッチする）。
- 正規化名 `img 5020` vs `img 3020` はレーベンシュタイン距離 1 → **filename similarity 0.88**（8 文字中最長の差 1 文字）。token jaccard 0.33、ext 0（`.jpg` vs `.jpeg`）で file score 0.61。
- ディレクトリ中央値 mtime が 636–1378 日前 → **年齢ボーナスが飽和（+0.10）**し、dir score 0.61 → 0.71。実フォルダ（`misc` 0.61 + 0.10）と同点帯になり、tie-break の「古い順」で Eagle が先頭に立つ。
- 38 件中 30 件は `_thumbnail.png` を含む 2 evidence（`evidence_count` はスコアに加算されないが、dir 施行に影響）。

### 2.3 候補 50 件のクラス分類（修正前）

| クラス | IMG_3020.jpeg | IMG_3267.PNG | 判定 |
|---|---|---|---|
| アプリ内部ストア（`*.library/**`、Eagle `.info` = ハッシュ名ディレクトリ） | **38 / 50** | **36 / 50** | 除外対象 |
| `~/Library/**`（Anki2 media、VSCode agentSessionData、Firefox storage） | 0 / 50（集約 1363 中 9） | 0 / 50 | 除外対象 |
| 隠しディレクトリ（`.` 始まり） | 0 / 50 | 0 / 50 | 除外対象（規約） |
| キャッシュ（`Caches`/`TemporaryItems`/`*.cache`） | 0 / 50 | 0 / 50 | 除外対象（規約） |
| ユーザーフォルダ（写真・メディア・作業フォルダ） | 12 / 50 | 14 / 50 | **維持** |

集約 1363 ディレクトリの内訳: `bundle`（`*.library` 祖先）1226、`library`（`~/Library`）9、その他 128（`~/OS/home/**` 71、`~/OS/media/**` 26、`~/Desktop/**` 24 など）。

### 2.4 二次ケース（IMG_3267.PNG）

| 計測 | 結果 |
|---|---|
| クエリ | `... && (kMDItemFSName == '*3267*'c \|\| kMDItemFSName == '*IMG*'c)` |
| raw / 集約 / 候補 | 3545 / 1363 / 50 |
| 修正前 D1–D3 | すべて Eagle `.info`（`LG2SE60NHCWDP.info` 0.83、`LR1MA5966EKRC.info` 0.83、`LT80QV3R4FCY1.info` 0.78） |
| 候補 50 件のクラス | Eagle 36 件 + ユーザーフォルダ 14 件 |

### 2.5 PDF 回帰基準（不変条件）

| 計測 | 結果 |
|---|---|
| クエリ | `Non Detect THCO Updated COA`（語クエリ・不変） |
| raw / 採点 / 候補 | 25 / 21 / 17 |
| D1 | `~/OS/home/.../sinks/State Of Mind Labs` score **1.00** / `exact normalized filename` |
| 除外対象の混入 | 1 件（`~/Library/Application Support/Code/User/workspaceStorage/<hash>/chatSessions`、D17） |

## 3. 規則の設計（個別パス黒名单ではなく「移動先になれないディレクトリ」の規則）

### 3.1 採用する規則（計測に基づく取捨選択）

| # | 規則 | 適用範囲 | 計測された該当 | 根拠 |
|---|---|---|---|---|
| R1 | **バンドル／アプリ内部ストア**: パス構成要素が大文字小文字を問わず `.library` `.app` `.bundle` `.framework` `.pkg` `.plugin` `.xcodeproj` で終わる | 全構成要素 | Eagle `.library` 1226/1363 | 実測（本件の根本原因）。macOS のパッケージ規約 |
| R2 | **macOS ライブラリツリー**: 深さ 0–2 の構成要素が `Library`（`/Library`、`/Users/<u>/Library`、`/Volumes/<v>/Library`） | 深さ ≤2 | `~/Library` 9/1363 + PDF 1/17 | 実測。深さ 3 以降の `Library` はユーザー作成フォルダとして維持（過剰除外の回避） |
| R3 | **隠しディレクトリ**: 候補ディレクトリ**自身の名前**が `.` 始まり | basename のみ | 0（3 データセット） | macOS の可視性規約（`~/.Trash` 等への移動防止）。祖先の隠し要素は除外しない（`~/.ghq`・`~/.local/share` 配下の通常フォルダを落とさないため。実測: 本リポジトリの worktree も `.delta/**` 配下） |
| R4 | **キャッシュ・一時領域**: 構成要素が `Cache`/`Caches`/`TemporaryItems`（大文字小文字を問わず）または `*.cache` で終わる | 全構成要素 | 0（3 データセット） | macOS／アプリのキャッシュ命名規約。`~/.cache/**` も `.cache` 要素で捕捉 |
| R5 | **機械生成名**: basename（最後の `.` より前の語 = `scoring::split_stem` を再利用）が、① UUID（8-4-4-4-12 hex、大小文字不問）、② **16 文字以上で全文字が hex**（`0-9a-f`、大小不問）、③ **12 文字以上で全文字が `A-Z0-9` かつ英字と数字を各 1 文字以上含む**のいずれか | basename のみ | 0（規則 R1–R4 適用後） | Eagle の `<hash>.info` の一般形。`~/.docker/sbom/sha256/<64hex>`・`~/.copilot/session-state/<uuid>`・`20260925T175534` 等の機械生成名を捕捉 |
| R6 | **OS 予約領域**: 構成要素が `.Trash` / `.Trashes` / `.Spotlight-V100` / `.fseventsd` / `.DocumentRevisions-V100` / `.TemporaryItems`（大文字小文字不問） | 全構成要素 | 0（3 データセット） | 実装中の追加（reviewer MAJOR-2）。組織化ツールがゴミ箱・ボリューム管理領域へファイルを移す危害を規則で防ぐ。R3 を祖先まで広げると `~/.ghq/**` 等の正当ツリーが消えるため、OS 予約名の列挙で `~/.Trash/old-album` のネストだけを閉じる |

### 3.2 過剰除外の検証（計測）

- **実測スキャン**: `$HOME` + `/Volumes` の **171,428 ディレクトリ**を走査し、各規則の一致数を計測。
  - UUID 892（すべて `~/.copilot/session-state/**` 等の機械生成）、hex≥16 1289（`.docker/sbom/sha256/**` 等）、`[A-Z0-9]{12,}` 1913（`20260925T175534` のようなタイムスタンプログ、Chrome ストレージ ID、Eagle の `<hash>.info`）。
  - 規則 R1–R4 を適用した後に **R5 だけが追加で除外するもの 2,183 件のうち、人手で作ったと読める名前は 0 件**（全件が機械生成またはアプリ内部）。
- **維持される名前の境界例**（テストで固定）: `2024-photos`（区切りあり）、`PhotoBackup2024`（小文字混在）、`IMG3020`（12 文字未満）、`roll-001`、`State Of Mind Labs`、`camera`、`misc`、`Life`。
- **R5 の既知トレードオフ（記録）**: `[A-Z0-9]{12,}` は `PHOTOS2024BACKUP` のような「区切りなし全大文字 12 文字以上」の人手命名も除外し得る。実測（171,428 ディレクトリ）では該当 0 件のため採用し、境界テストで固定する。
- **既知の残余（記録）**: 隠しディレクトリの**配下**（例 `~/.Trash/old-album`）は候補になり得る（basename が可視のため）。3 データセットで発生 0 件。R3 を祖先まで広げると `~/.ghq/**`・`~/.local/share/**` のような正当な作業ツリーが消えるため、計測に基づき basename 限定を採用する。

### 3.3 適用位置（1 つの述語・2 つの呼び出し点）

- 新規 `src/discovery/destinations.rs` に純関数 `is_excluded_destination(dir: &str) -> bool` を実装（syscall なし・文字列判定のみ）。
- **呼び出し点 1（探索段階・最適化 + debug 整合）**: `similarity.rs` のヒットループで「親ディレクトリが除外対象」のヒットを、`exists`/`canonicalize`/`metadata` の前に落とす（実測: 2276/3526 件を削減）。判定は **親ディレクトリのみ**（ヒット自身がファイルの場合、その機械生成名を理由に証拠を落とさないため）
- **呼び出し点 2（集約段階・権威）**: `candidates.rs` の `aggregate_by_directory_with_exclusions` で除外。**Jev criteria（`DecisionContext::from_normalized`）と `discover` 表示は同じ候補リストから作られるため、両方から消える**（`src/operations/plan.rs` の移動計画も同じ）
- 両呼び出し点で `KASEIFU_DEBUG=1` に削減数（skip ヒット数 / 除外ディレクトリ数）を出力し、実測レポートの根拠にする

## 4. BDD タスク分解（振る舞い単位）

### T1. 規則の実装（`src/discovery/destinations.rs` 新規）

- **振る舞い**: `is_excluded_destination(dir)` が、移動先になれないディレクトリに対して `true` を返す
- 正常系（除外）:
  - `~/Pictures/Eagle.library/images/LY2TLHNM2EO5K.info` → true（R1。実測パス）
  - `~/Library/Application Support/Anki2/User 1/collection.media` → true（R2。実測パス）
  - `~/Library/Application Support/Code/User/workspaceStorage/<hash>/chatSessions` → true（R2。実測パス）
  - `/Library/Fonts`、`/Volumes/Backup/Library/Photos` → true（R2）
  - `~/Pictures/Eagle.library/images/LY2TLHNM2EO5K.info` の basename 判定 → true（R5）
  - `~/.Trash`（R3）、`~/dev/app/.cache/img`（R4）、`/Volumes/X/TemporaryItems`（R4）、`/Applications/Foo.app/Contents/Resources`（R1）
- 異常系（除外しない＝過剰除外の防止）:
  - `~/Pictures`、`~/Documents`、`/Volumes/External/Photos`→ false
  - `~/OS/media/.../documents/media/misc`（実測 D2 相当）→ false
  - `~/OS/home/.../GM.Shop/images`（実測 D3 相当）→ false
  - `~/Documents/Library/images`（深さ 3 の `Library`）→ false（R2 の境界）
  - `~/dev/my.library-notes`（`.library` で終わらない）→ false
  - `~/.ghq/github.com/me/blog/assets/img`（隠し祖先の下の可視フォルダ）→ **false**（R3 は basename 限定）
  - `2024-photos`、`PhotoBackup2024`、`IMG3020`、`IMG_3020`、`roll-001`、`State Of Mind Labs` → false（R5 の境界）
  - `~/Pictures/Caches`→ true（R4、キャッシュ命名）／`~/Pictures/Cache-ish`→ false
- エッジ: 非 ASCII 名（`Heml博覧会`）は R5 で誤判定しない（`is_ascii` 要求）／`/` 直下・相対風の空セグメントで panic しない／末尾スラッシュ・二重スラッシュを正規化

### T2. 集約段階での適用（`src/discovery/candidates.rs`）

- **振る舞い**: 除外ディレクトリの `SimilarFile` は集約されず、`CandidateDestination` に現れない
- 正常系: 除外ディレクトリと実フォルダが混在 → 実フォルダのみが残り、**ID は D1 から連番で振り直される**（既存挙動の維持）
- 異常系/エッジ: 全ディレクトリが除外対象 → 空ベクタ（panic しない）／`excluded_dirs`（自身の親）との二重除外でも挙動不変
- データ例: `SimilarFile` の `parent_directory` に実測 Eagle パスと `~/OS/media/.../misc` を混在させる

### T3. 探索段階での適用（`src/discovery/similarity.rs`）

- **振る舞い**: 親ディレクトリが除外対象のヒットは `SimilarFile` として返らない（採点前にスキップ）
- 正常系: 除外対象配下のヒットのみを与える → 返却 0 件・`discover_with` の候補 0 件
- エッジ: 親を持たないパス（`/` 直下）は従来どおり処理／既存の `is_system_directory` 判定と競合しない
- 計測: 実写真で `SimilarFile count` が 3526 → 約 1234 に減り、候補集合は不変（削減分はすべて除外対象配下）

### T4. 実写真の候補品質（受入基準1・before/after）

- **振る舞い**: `discover ~/Desktop/images/IMG_3020.jpeg` の上位が、ユーザーが移動先として妥当と判断できるディレクトリ（写真・メディアフォルダ）になり、アプリ内部ストアが消える
- 正常系: before/after の候補一覧（JSON）を保存して比較。**上位 20 件に `*.library`/`~/Library`/隠し/キャッシュ/機械生成名が 0 件**
- 予測（集約後ランキングの再計算）: D1 `~/OS/media/.../media/misc`（n=152）、D2 `.../GM.Shop/images`、D3 `~/Desktop/auction/done/yamaha` …
- エッジ: 自身の親ディレクトリ `~/Desktop/images` は除外されたまま（設計どおり）

### T5. 二次ケース（IMG_3267.PNG）

- **振る舞い**: 同様に Eagle `.info` 36 件が消え、写真・メディアフォルダが上位に来る（予測 D1 `.../Outreach/20250627/seo` 0.78）

### T6. PDF 回帰（不変条件）

- **振る舞い**: `discover ~/Desktop/THCO-COA-Non-Detect Updated.pdf` で D1 `State Of Mind Labs` score 1.00 / `exact normalized filename` が先頭のまま
- 期待差分（記録必須）: 候補 17 → 16 件（`~/Library/Application Support/Code/.../chatSessions` が除外される）。D2 以降の順位・スコアは不変

### T7. テスト（受入基準3）

- 新規 `tests/v042c_destination_exclusion_test.rs`（実 Spotlight 非依存・注入 search のみ）
  - T1 の規則表（除外 12 例 / 維持 10 例以上。実測パスを fixtures として使用）
  - T2 集約・ID 連番・空リスト・自身の親除外との併用
  - T3 探索段階スキップ（除外配下のみ → 候補 0）
  - T6 PDF 回帰（`State Of Mind Labs` が D1 = 1.0）
- ユニットテストを `src/discovery/destinations.rs` 内にも置く（規則の境界を最速で固定）
- 既存 260 件を維持（`cargo test --all-targets --all-features`）

### T8. 品質ゲート（受入基準3）

- `bash scripts/quality_check.sh` OVERALL PASS（fmt / clippy -D warnings / test / complexity / baseline-gate / report-tests / coverage）
- 新規 CC16+ 関数なし（規則は 1 関数 1 責務の小さな純関数に分割）。`reports/quality/baseline.json` は意図的な複雑度変化がない限り更新しない
- 新規依存なし（`Cargo.toml` / `Cargo.lock` 不変。正規表現クレートを足さない）

### T9. ドキュメント

- `CHANGELOG.md` `[Unreleased]` Fixed に「移動先になれないディレクトリの排除」を追加
- `documents/design/03-discover.md` の Safety（候補として許可しないもの）に規則の要約を追記
- 計画書・レビュー・QA の issue を `documents/plans/dev/issues/` に保存

## 5. 不変条件（厳守・テストで担保）

1. 撮影日時・GPS に Rust 側の適合度スコアを付けない（v3.2 原則。本変更はパス文字列のみを扱う）
2. 自身の親ディレクトリ除外（`src/discovery/mod.rs` → `aggregate_by_directory_with_exclusions`）は設計どおり維持
3. 非画像ターゲットのクエリ・候補生成は不変（PDF 回帰: D1 = `State Of Mind Labs` 1.00）
4. `discover_with` / `find_similar_files_with` の注入 search 境界 `Fn(&str, usize)` を維持
5. ASCII 表示方針・NO_COLOR/非 TTY・TUI 禁止・JSON 互換を維持。`discover` の `#[command(hide = true)]` は変更しない
6. 秘密情報・位置情報をログ・コミット・レポートに含めない（レポートのパスは `~/` 表記 + `<hash>` プレースホルダ）
7. 新規ファイルは `git add` して tracked にする。`git commit` / `push` / `tag` は行わない（親が実施）
8. `.gitignore` は変更しない

## 6. 変更ファイル（予定）

- New: `src/discovery/destinations.rs`（規則 + ユニットテスト）
- Modify: `src/discovery/mod.rs`（`mod` 宣言 + `pub use`）、`src/discovery/candidates.rs`（集約時の除外）、`src/discovery/similarity.rs`（採点前スキップ）、`CHANGELOG.md`、`documents/design/03-discover.md`
- New: `tests/v042c_destination_exclusion_test.rs`
- New: `documents/plans/dev/20261014c-v042c-discovery-destination-policy.md`、`documents/plans/dev/issues/20261014-{plan-architect,reviewer,qa}-v042c*.md`

## 7. ツール根拠（推測でタスクを切らない）

- `read_file: src/discovery/similarity.rs` / `candidates.rs` / `mod.rs` / `scoring.rs` — 除外判定の現状（部分一致 7 要素のみ）・集約ロジック・スコア式（`W_FILENAME=0.60`/`W_TOKEN=0.25`/`W_EXT=0.15`、`DIR_W_MAX=0.85`、`AGE_WEIGHT=0.10`）を確認し、Eagle の 0.7083 を式から再現
- `read_file: src/models/candidate.rs` / `src/classifier/jev.rs:403-408` — Jev criteria が候補リストから作られること（除外が criteria と表示の両方に効く）を確認
- `read_file: src/classifier/pipeline.rs:212` / `src/operations/plan.rs:44-73` — 候補リストの消費者は discovery の 1 本であることを確認
- `read_file: src/policy/mod.rs` — `policy` は Phase 5 の意思決定閾値であり、パス規則の置き場ではないと判断（`discovery` 内に新モジュール）
- `read_file: tests/v042_discovery_target_scope_test.rs` / `phase4_*_test.rs` — テストフィクスチャは `target/<fixture>` 配下で、新規則 R1–R5 に抵触しないことを確認
- `terminal: KASEIFU_DEBUG=1 discover`（実写真・`.debug.txt` 解析）— クエリ・件数・dir スコア内訳・evidence を全量取得し、Eagle 38 件すべてが `*IMG*` 到達であること、2276/3526 が Eagle 配下であることを確定
- `terminal: discover ~/Desktop/THCO-COA-Non-Detect Updated.pdf` — PDF 回帰基準（D1 1.00）と、除外対象 1 件（`~/Library/.../chatSessions`）を確定
- `terminal: python3`（`$HOME`+`/Volumes` 171,428 ディレクトリ走査）— R5 の偽陽性 0 件を確認（過剰除外の回避根拠）

## 8. 完了条件（ユーザ受入基準の対応）

1. T4/T5: 実写真 2 件の before/after 候補一覧（上位が写真・メディアフォルダ、アプリ内部ストア 0 件）
2. T6: PDF で D1 `State Of Mind Labs` score 1.00 が先頭（候補 17 → 16 の意図的差分つき）
3. T7/T8: 既存 260 件 + 新規テスト緑、`quality_check.sh` OVERALL PASS、新規 CC16+ なし、新規依存なし
4. T1: 除外規則そのものの境界テスト（除外される/されない）
- 付帯: T9 ドキュメント、`git add` のみ（commit/push/tag なし）

## 9. 完了記録（2026-10-14 実測）

### 9.1 実装

- `src/discovery/destinations.rs`（新規）: 規則 R1–R6 の純関数 `is_excluded_destination` + ユニットテスト 10 件
- `src/discovery/candidates.rs`: 集約段階で適用（権威ゲート）+ 除外件数の debug 出力
- `src/discovery/similarity.rs`: ヒットの親ディレクトリを `exists`/`canonicalize`/`metadata` の前に判定しスキップ + 削減数の debug 出力（`parent_directory` ヘルパで重複排除）
- `src/discovery/mod.rs`: `mod destinations` + `pub use`、集約コメント更新
- `tests/v042c_destination_exclusion_test.rs`（新規 9 件）、`CHANGELOG.md`、`documents/design/03-discover.md`
- 新規依存なし。実装中の発見で R1 を「拡張子が `library` 終わり or 既知パッケージ拡張子」に一般化（拡張子なしの `mylibrary` 等は維持）

### 9.2 根本原因（計測結果）

- Eagle は `<hash>.info/` に元のファイル名で画像を保存（`IMG_5020.jpg` + `_thumbnail.png`）
- 画像クエリの `kMDItemFSName == '*IMG*'c` で到達（上位 38 件すべて `*IMG*` 由来。`*3020*` なし）
- 正規化名 `img 5020` vs `img 3020` = レーベンシュタイン距離 1 → filename similarity **0.88**、token 0.33、ext 0（`.jpg` vs `.jpeg`）→ file score 0.61
- 中央値 mtime 636–1378 日前 → 年齢ボーナス飽和（+0.10）で dir score **0.71**。実フォルダ（0.61 + 0.10）と同点帯になり、古い順 tie-break で先頭
- raw 3545 ヒット中 **2276 件（65%）が Eagle 配下**、16 件が `~/Library` 配下 → 採点対象の大半が使えない目的地

### 9.3 修正前後の候補比較（実機 JSON）

| 位 | IMG_3020.jpeg 修正前 | IMG_3020.jpeg 修正後 |
|---|---|---|
| D1 | `~/Pictures/Eagle.library/images/LY2TLHNM2EO5K.info` 0.708 | `~/OS/media/.../documents/media/misc` 0.708（n=152） |
| D2 | `~/OS/media/.../documents/media/misc` 0.708 | `~/OS/home/.../4_select_shop/GM.Shop/images` 0.705（n=8） |
| D3 | `~/OS/home/.../GM.Shop/images` 0.705 | `~/Desktop/auction/done/iphoen14pro` 0.633（n=6） |
| D4–D50 | Eagle `.info` 36 件含む | すべてユーザーフォルダ（`Heml博覧会`・`infographic`・`references`・`found`・`mine`・`yamaha`・`camera` 等） |
| クラス | アプリ内部ストア 38 / ユーザー 12 | **アプリ内部ストア 0 / ユーザー 50** |

| 位 | IMG_3267.PNG 修正前 | IMG_3267.PNG 修正後 |
|---|---|---|
| D1 | `.../LG2SE60NHCWDP.info` 0.835 | `~/OS/home/02_Life/theory_of_instgram/relations` 0.780（n=12） |
| D2 | `.../LR1MA5966EKRC.info` 0.835 | `~/OS/home/.../Outreach/20250627/seo` 0.776（n=7） |
| D3 | `.../LT80QV3R4FCY1.info` 0.783 | `~/OS/media/.../documents/media/misc` 0.776（n=152） |
| クラス | アプリ内部ストア 36 / ユーザー 14 | **アプリ内部ストア 0 / ユーザー 50** |

- 両ケースで `~/Desktop/images`（対象の親）は候補に現れない（設計どおり）
- 計測値: raw 3545 → skip 2292 → 採点 1234（修正前 3526）→ 集約 128（修正前 1363）→ 候補 50

### 9.4 PDF 回帰（不変条件）

| 計測 | 修正前 | 修正後 |
|---|---|---|
| 候補件数 | 17 | **16**（`~/Library/Application Support/Code/.../chatSessions` を R2 で除外。意図的差分） |
| D1 | `~/OS/home/.../sinks/State Of Mind Labs` 1.00 / `exact normalized filename` | **同一**（score 1.00・reasons 全文不変） |
| D2–D5 | 完全一致 1.0 群 | 同一（順位・スコア不変） |

### 9.5 性能・資源（A/B 実測）

同一索引状態で交互に 5 回実行した中央値（検索段階のスキップあり/なし）:

| 構成 | real | max RSS |
|---|---|---|
| スキップなし（集約ゲートのみ） | 1.75s | 16.9 MB |
| **スキップあり（本実装）** | **1.45s** | **16.1 MB** |

候補 JSON は両者で一致（差は年齢ボーナス由来の浮動小数末桁のみ）。mdfind 自体は 0.46–1.7s で環境負荷に応じて変動するため、上記は同一ウィンドウでの交互測定値。

### 9.6 テスト・ゲート

| 検証 | 結果 |
|---|---|
| `cargo test --all-targets --all-features` | ✅ **279 passed / 0 failed**（既存 260 + 新規 19） |
| `bash scripts/quality_check.sh` | ✅ **OVERALL PASS**（fmt / clippy / test / complexity / baseline-gate / report-tests / coverage） |
| 新規 CC16+ | ✅ なし（最大 CC 15 は既存、baseline.json 未更新） |
| 新規依存 | ✅ なし |

### 9.7 成果物

- `documents/plans/dev/issues/20261014-plan-architect-v042c-review.md`（APPROVE）
- `documents/plans/dev/issues/20261014-reviewer-v042c.md`（APPROVE WITH COMMENTS、MAJOR 2 件は修正・記録済）
- `documents/plans/dev/issues/20261014-qa-v042c.md`（PASS）
- 新規ファイルは `git add` 済（`git commit` / `push` / `tag` は親が実施）

### 9.8 残余（記録）

- 隠しディレクトリの**配下**にある可視フォルダ（例 `~/.config/*/album`）は候補になり得る。3 データセット・実機走査で発生 0 件。R3 を祖先まで広げると `~/.ghq/**`・`~/.local/share/**`・worktree の `.delta/**` のような正当な作業ツリーが消えるため、basename 限定を維持（reviewer/QA に記録）
- `[A-Z0-9]{12,}`（区切りなし全大文字 12 文字以上）の人手命名（例 `PHOTOS2024BACKUP`）は除外され得る。実機 171,428 ディレクトリで該当 0 件のため採用し、テストで固定
- PDF 候補件数 17 → 16 は意図的差分（D1 は不変）

