# organize 出力改善 4件 BDD 計画書

原典: 親スレッドのユーザーリクエスト（実機出力つき）。対象: worktree `kaseifu`、HEAD `6819ac9`（v0.4.2、作業ツリーはクリーン）。

本書は BDD タスク分解と検証エビデンスの記録である。実装・レビュー・QA の成果物は `documents/plans/dev/issues/` に issue 形式で保存する。

## 1. 問題の要約

`organize` の表示だけに 4 つの問題がある（判断・実行・journal・policy の挙動はすべて正しい）:

1. **選択先パスが出ない** — `destination: Some("D1")` と候補 ID だけで、Jev が選んだ候補がどのディレクトリなのか分からない（`OrganizeReport.candidate_path` には既に解決済みパスがある）
2. **「executed」が実行モード表示なのに実行済みと読める** — `KASEIFU / ORGANIZE executed` は `actually_move`（実行モードが指定されたフラグ）を出しており、policy=Leave では「実行したのに移動していない」矛盾に見える
3. **内部 enum の Debug が露出** — `== No move ==\n  NotAutoMove`（`PlanError`）、`== Policy ==\n  Leave { what: ... }`（`DecisionAction`）
4. **順序と体裁の不整合** — モード行が Decision と Policy を分断し、`organize` だけプレーン行（`inspect`/`classify`/`discover` は v3.1 Cyber Dashboard の ASCII パネル）

該当コード: `src/main.rs:283-323`（`handle_organize`）、`src/main.rs:325-361`（`print_organize_outcome`）、`src/presentation/renderers/generic.rs:26-32`（`dry_run_banner`）。

## 2. 実測（2026-10-13・修正前）

### 2.1 実 PDF の organize（rich）

```text
$ kaseifu organize ~/Desktop/THCO-COA-Non-Detect\ Updated.pdf --format rich
== Decision ==
  destination: Some("D1")
  confidence:  0.370
  margin:      0.110
KASEIFU / ORGANIZE dry-run (no files moved)
== Policy ==
  Leave { what: "confidence below review threshold" }
== No move ==
  NotAutoMove
```

- `--format rich` でも非 TTY では color 無効（`color_enabled`）のため無色で記録
- policy=Leave（confidence 0.37 < review 0.60）→ 無移動・journal 記録なし（安全動作は正しい）

### 2.2 同じ実行の `--format json`（報告専用・修正対象外）

```text
$ kaseifu organize ~/Desktop/THCO-COA-Non-Detect\ Updated.pdf --format json
== Decision ==
  destination: Some("D1")
  confidence:  0.310
  margin:      0.090
== Policy ==
  Leave { what: "confidence below review threshold" }
== No move ==
  NotAutoMove
```

JSON は出ず、通常のプレーン行が出る（`src/main.rs:307-321` が mode を見ずに print、rich 時のみヘッダ追加）。グローバル `--format auto|rich|json` の約束と矛盾する。**本タスクでは実装せず、親へ報告する**（§7）。

### 2.3 参考: 同一ファイルの classify（rich） — 統一先の体裁

```text
+-- * KASEIFU / CLASSIFY ---------------+
| TARGET                                |
| THCO-COA-Non-Detect Updated.pdf       |
| PDF - 123.6 KiB                       |
|-- JEV RANKING ------------------------|
| *1  D1  ~/OS/home/.../State Of Mind Labs  31% |
|-- JEV DECISION -----------------------|
| Selected     D1 - ~/OS/home/.../State Of Mind Labs |
| Confidence   0.31                      |
| Margin       0.03                      |
|-- POLICY -----------------------------|
| [LEAVE] LEAVE                          |
| confidence below review threshold      |
+---------------------------------------+
```

（実測の要約。パネル描画・`[LEAVE] LEAVE` の 2 行構成・`Selected/Confidence/Margin` の 13 桁キーは classify が既に確立した流儀で、organize はこれに合わせる）

### 2.4 テスト基準値

`cargo test --all-targets --all-features` = **279 passed / 0 failed**（lib 51 + 統合 228）。

### 2.5 重要な実装事実（grep/git で確定）

- `OrganizeOutcome::Refused` は **どこからも構築されていない**（`src/operations/organize.rs` は plan 失敗時に `Unplanable(PlanError::NotAutoMove)` を返す。`git log -S Refused` で watch 追加時から未使用）。したがって `policy refused`（T39/T40 の MUST 文言）は **NotAutoMove 分岐の表示**として出す必要がある
- `dry_run_banner` の既存文言を検証する自動テストは存在しない（`grep` で tests/ に 0 件）。`tests/user-test-plan.csv` の T34/T37/T39/T40 が文言に言及（手動計画）
- `--verbose` は organize に存在しない（意味を変える対象なし）

## 3. 設計方針（表示層のみ）

- **新規 `src/presentation/renderers/organize.rs`** に表示を集約（`ClassifyReport` と同じ「ライブラリ側 renderer」パターン。binary 内 fn のままだとテストできない）。`main.rs` の `handle_organize` は `organize_view::print_rich` / `render_text` を呼ぶだけにし、`print_organize_outcome` は削除
- **順序**: モード → DECISION（選択先パス）→ POLICY → 結果。モード行は先頭に固定し判断情報を分断しない
- **モード文言**: `generic::dry_run_banner` を `execute mode` / `dry run (no files moved)` に変更（`watch_line` 経由の watch も同じ文言規則になる）。実際に移動したかは結果セクションのみが述べる
- **パス表示**: rich は `paint_path(.., path_budget(), color)`（classify ランキングと同流儀）、プレーンは `relative_display`（classify --verbose と同流儀）。NONE/None はパスを出さず `no destination`
- **policy**: classify の `policy_view`（`[LEAVE] LEAVE` + reason）を `generic.rs` に移して共有（classify の出力は不変）。`Leave { what }` Debug を出さない
- **PlanError**: enum → 人間向け文の写像を organize renderer に置く。`Collision` は現状の REVIEW セクションを維持。構造（`OrganizeOutcome`）はそのままなので機械可読な原因は失わない

### 3.1 表示（修正後・目標）

rich（パネル）:

```text
+-- * KASEIFU / ORGANIZE --------------+
| dry run (no files moved)             |
|-- DECISION --------------------------|
| Selected     D1 - ~/OS/home/.../State Of Mind Labs |
| Confidence   0.370                   |
| Margin       0.110                   |
|-- POLICY ----------------------------|
| [LEAVE] LEAVE                        |
| confidence below review threshold    |
|-- NO MOVE ---------------------------|
| policy refused: automatic move not allowed |
+--------------------------------------+
```

プレーン（`--format json` / 非 TTY）:

```text
== Mode ==
  dry run (no files moved)
== Decision ==
  Selected     D1 - ~/OS/home/.../State Of Mind Labs
  Confidence   0.370
  Margin       0.110
== Policy ==
  [LEAVE] LEAVE
  confidence below review threshold
== No move ==
  policy refused: automatic move not allowed
```

## 4. BDD タスク分解（振る舞い単位）

### T1. Decision セクションに選択先ディレクトリを表示

- **振る舞い**: `OrganizeReport.candidate_path`（配線追加なし）を使い、`Selected <id> - <path>` を出す
- 正常系: `destination_id=Some("D1")` + `candidate_path=Some(~/OS/home/.../State Of Mind Labs)` → 相対表示（HOME 配下は `~/...`、cwd 配下は `./...`）。rich は `paint_path`（長パスは `.../parent/last` に短縮・最終セグメント強調）、プレーンは `relative_display`（短縮なし）
- NONE 系: `Some("NONE")` → `Selected     NONE - no destination`（**パスを出さない**）。`None` → `Selected     no destination`（同左）
- 異常系: id はあるが候補が見つからない → `unresolved candidate`（classify と同じ語）。HOME そのもの → `~`
- **非回帰**: policy が Leave でも「検討された行き先」としてパスが見える。`Selected` は Jev の選択であり実行許可として読ませない（POLICY セクションと別行・別セクション）

### T2. 実行モード文言の是正（`dry_run_banner`）

- **振る舞い**: `actually_move=true` → `execute mode`、`false` → `dry run (no files moved)`。`executed` の語は消える
- 一貫性: `watch_line`（watch の dry-run 表示）も同じ文言になることをテストで固定
- 実際の移動有無は結果セクション（MOVED / WOULD MOVE / NO MOVE）だけが述べる
- エッジ: color 有無の両方で文言は同一（ANSI のみ差）
- **対象外（残課題）**: `print_watch_start` の非 rich 文言 `watching ... (dry-run: true)` と `watch/runner.rs` の `dry-run {path} ...` は helper を使わない別文言で、矛盾する「executed」を含まないため変更しない（Plan Architect MED-1）

### T3. Policy セクションの人間向け化

- **振る舞い**: `DecisionAction` を `generic::policy_view` で `[KIND] KIND` + 理由文に変換（classify と同一の写像を共有）
- 正常系: Leave → `[LEAVE] LEAVE` / `confidence below review threshold`。AutoMove/Review も classify と同じ理由文
- **非回帰**: `Leave { what: ... }` 等の Debug が出力に現れない

### T4. 結果セクションの人間向け化（Debug 排除）

- **振る舞い**: `OrganizeOutcome` ごとに人間向けの行を出す
  - `Moved { operation_id, destination }` → `MOVED`（プレーン `== Moved ==`）+ `Destination  <path>` + `Operation    <id>`（既存の destination/operation 表示を後退させない）
  - `WouldMove { destination }` → `WOULD MOVE`（`== Would move ==`）+ `From         <source>` + `Destination  <destination>`（既存の「would move src -> dst」の情報を保持）
  - `Refused` → `NO MOVE` + `policy refused`（既存文言を維持）
  - `Unplanable(NotAutoMove)` → `NO MOVE` + `policy refused: automatic move not allowed`（T39/T40 の「policy refused」を含み、原因が読める）
  - `Unplanable(UnknownCandidate(id))` → `unknown candidate selected by the decision: <id>`
  - `Unplanable(SourceMissing(p))` → `source file is missing: <p>`／`SourceNotAFile(p)` → `source is not a regular file: <p>`／`DestinationNotADirectory(p)` → `destination is not a directory: <p>`
  - `Unplanable(Collision(p))` → `REVIEW` + `destination already exists: <p>`（現状表示の維持。T37 と整合）
  - `MoveFailed(e)` → `MOVE FAILED` + 既存のエラー文字列（この文字列の人間向け化は本タスクの対象外＝残課題に記録。Plan Architect MED-2: `operations/organize.rs` の `format!("{error:?}")` は watch にも流れる共有データのため触らない）
- **非回帰**: `NotAutoMove` / `UnknownCandidate` / `SourceMissing` / `SourceNotAFile` / `DestinationNotADirectory` の literal が表示に出ない（テストで固定）
- **policy 拒否の実経路（Plan Architect HIGH-2）**: `Refused` は未構築で、Review/Leave/NONE はすべて `Unplanable(NotAutoMove)` として返る。T39/T40 の MUST 文言「policy refused」は NotAutoMove 写像で担保し、未使用の `Refused` 分岐も防御的に「policy refused」のまま残す（削除しない＝挙動変更を避ける）

### T5. パネル統一とプレーン表示

- **振る舞い**: rich は `panel::frame_with_sections("KASEIFU / ORGANIZE", [mode/decision/policy/result])`。プレーンは `== Mode ==` / `== Decision ==` / `== Policy ==` / `== <結果> ==` + 2 スペースインデント
- 順序の固定（テスト）: mode 行 → `DECISION` → `POLICY` → 結果見出し。モード行が Decision/Policy の間に来ない
- **非回帰**: ASCII のみ（`strip_ansi` 後 `is_ascii`）、全行の可視幅一致、`NO_COLOR`/非 TTY の挙動不変（rich 判定は `resolve_mode` のまま）

### T6. `--format json` の扱い（報告のみ・実装しない）

- 現状: JSON を出さずプレーン行（§2.2 で実測）。本タスクはこの分岐に手を入れない（JSON 化もしない。`resolve_mode`／分岐構造も不変）
- 副作用の記録: プレーン表示の改善は `--format json` の出力文字列にもそのまま適用される（人間向け行のまま）。**修正前後の全文を最終報告に並べ**、「分岐挙動は不変・文字列はプレーン改善の対象」と明記する（Plan Architect HIGH-1。文字列バイト不変の解釈が必要なら親判断）

### T7. テスト（受入基準5）

- 新規 `tests/organize_output_test.rs`（表示のみ検証。Jev/ファイルシステム不要の純フィクスチャ）
  - T1（パス表示・NONE/None・unresolved）／T2（banner + watch_line 一貫）／T3（[LEAVE] と Debug 排除）／T4（全 outcome の文言。Moved/WouldMove のパスと operation id 含む）／T5（順序・ASCII・幅整合・プレーン全文）
  - 各文言は「存在」と「不在」（`NotAutoMove` 等、`executed`）の両方向で固定
  - パスフィクスチャは $HOME 配下の短いパス（truncation 非依存）とし、`COLUMNS` に依存させない（Plan Architect MED-3）
- 既存 279 件を維持（特に `tests/v31_render_test.rs` の classify 表示が `policy_view` 移動後も不変であること。Plan Architect MED-4）

### T8. ドキュメント

- `CHANGELOG.md` `[Unreleased]` に Changed を追記（バージョン・タグ・commit・push はしない）
- `README.md` / `documents/design/05-execute.md` には organize の出力例が存在しないため更新不要（grep で確認、§5）

### T9. 品質ゲート（受入基準4）

- `bash scripts/quality_check.sh` OVERALL PASS（fmt / clippy -D warnings / test / complexity / baseline-gate / report-tests）
- 新規 CC16+ なし・新規依存なし（`Cargo.toml`/`Cargo.lock` 不変）。`reports/quality/baseline.json` は複雑度の意図的変化がないため更新しない

## 5. 不変条件（厳守）

1. 判断・policy・plan・mover・journal のコードは変更しない（表示層のみ）。ファイルシステムへの書き込みなし
2. ASCII 表示方針・`NO_COLOR`・非 TTY・TUI 禁止を維持。`discover` の `#[command(hide = true)]` は不変
3. `main.rs` の変更は `handle_organize` の表示委譲と renderer import に限定（並行タスクの `completions` 追加と衝突しない。`Commands` enum / ディスパッチには触れない）
4. `--format json` の分岐挙動は不変（JSON を実装しない・`resolve_mode` の判定も不変）。T6 として報告
5. 新規ファイルは `git add` して tracked に。`.gitignore`・`reports/quality/baseline.json` は変更しない（`policy_view` 移動は baseline-check が moved として許容することを確認済み。Plan Architect 検証）
6. 秘密情報・位置情報を出力・ログ・コミットに含めない（レポートのパスは `~/` 表記 + 省略）

## 6. 変更ファイル（予定）

- New: `src/presentation/renderers/organize.rs`（renderer 本体）
- Modify: `src/presentation/renderers/mod.rs`（`pub mod organize;`）
- Modify: `src/presentation/renderers/generic.rs`（`dry_run_banner` 文言 + `policy_view` を classify から移動）
- Modify: `src/presentation/renderers/classify.rs`（`generic::policy_view` を使用。出力不変）
- Modify: `src/main.rs`（`handle_organize` の表示委譲・`print_organize_outcome` 削除・import 整理）
- Modify: `CHANGELOG.md`
- New: `tests/organize_output_test.rs`
- New: 本書 + `documents/plans/dev/issues/20261013-{plan-architect,reviewer,qa}-organize-output.md`

## 7. `--format json` の報告事項（親の判断用・本タスク実装なし）

- **現状**: `organize --format json` は JSON を出さずプレーン行（§2.2 実測）。`setup/auth/config/doctor` は `emit_report` で「json → JSON / rich → パネル / それ以外 → プレーン」の規律を守っており、organize だけ逸脱している
- **影響**: スクリプトが `--format json` を指定しても構造化データが得られない。他コマンドの json との一貫性がない
- **整合案（いずれも本タスク外）**: (a) `emit_report` と同型に organize の結果を JSON 化（decision/policy/outcome/candidate_path）、(b) `--format json` を organize では明示エラーにする、(c) 現状を明記して仕様とする

## 8. ツール根拠（推測でタスクを切らない）

- `read_file: src/main.rs:283-362` — `handle_organize`/`print_organize_outcome` の現状、`--format json` が mode を見ない print であることを確認
- `read_file: src/operations/organize.rs` — `OrganizeReport.candidate_path` が既にある（配線追加不要）こと、`OrganizeOutcome` の全分岐を確認
- `read_file: src/operations/plan.rs` — `PlanError` 6 変種（NotAutoMove/UnknownCandidate/SourceMissing/SourceNotAFile/DestinationNotADirectory/Collision）
- `read_file: src/presentation/renderers/classify.rs` — `policy_view` / `Selected/Confidence/Margin` / `[LEAVE] LEAVE` の既存流儀（共有対象）
- `read_file: src/presentation/renderers/generic.rs` — `dry_run_banner` の使用箇所（organize rich ヘッダ + `watch_line`）
- `read_file: src/presentation/components/panel.rs` / `path.rs` — `frame_with_sections` / `paint_path` / `relative_display` / `path_budget` の再利用
- `read_file: src/presentation/format.rs` — `resolve_mode`（json は rich=false/color=false）
- `grep: '== REVIEW ==|NotAutoMove|dry_run_banner'` — 既存テストに文言検証なし（更新対象なし）／`tests/user-test-plan.csv` T34/T37/T39/T40 が文言に言及
- `git log -S Refused -- src/operations/organize.rs` — `Refused` は未構築（NotAutoMove が policy 拒否の実経路）
- `read_file: scripts/complexity_report.py:262-330` — baseline-check は (file,function) キーで freed name の一致を moved/renamed として扱う（`policy_view` 移動は gate 失敗にならない）
- `read_file: tests/presentation_test.rs:80-156` — パス系テストの流儀（`$HOME` fixture・`COLUMNS` 隔離）
- `terminal: cargo test` — ベースライン 279 passed
- `terminal: kaseifu organize ~/Desktop/THCO-COA-Non-Detect Updated.pdf --format rich|json` — 修正前出力の実測（§2.1/2.2）。`classify --format rich` で統一先の体裁を実測（§2.3）

## 9. 完了条件（受入基準の対応）

1. 実 PDF の修正後出力（rich dry-run/Leave + 可能なら `--execute` Leave）を全文提示
2. `--format json` の分岐挙動が不変であること（JSON を実装していない）＋報告（§7）
3. Moved/WouldMove のパス・operation id の非後退をテストで提示
4. `bash scripts/quality_check.sh` OVERALL PASS、279 + 新規テスト全緑、新規 CC16+ なし、新規依存なし
5. 表示文言・順序を固定するテスト（`tests/organize_output_test.rs`）
- 付帯: CHANGELOG `[Unreleased]`、`git add` のみ（commit/push/tag なし）

## 10. 完了記録（2026-10-13 実測）

### 10.1 実装

- `src/presentation/renderers/organize.rs`（新規）: `render_panel` / `print_rich` / `render_text`。モード → DECISION → POLICY → 結果の順、パスは `paint_path`（rich）/ `relative_display`（plain）、`PlanError` は人間向け文（`plan_error_message`）、`NONE`/None は `no destination`
- `src/presentation/renderers/generic.rs`: `dry_run_banner` を `execute mode` / `dry run (no files moved)` に、`policy_view` を classify から移動（共有）
- `src/presentation/renderers/classify.rs`: `generic::policy_view` へ委譲（出力不変）
- `src/main.rs`: `handle_organize` は表示委譲のみ、`print_organize_outcome` 削除（他は不変）
- `tests/organize_output_test.rs`（新規 17 件）、`CHANGELOG.md`（Unreleased）
- 新規依存なし・`Cargo.toml`/`Cargo.lock`/`.gitignore`/`baseline.json` 不変

### 10.2 修正後の実機出力（実 PDF・実 Jev）

実測時刻 22:05–22:07（ファイルが元の場所 `~/Desktop/` にあった時点）。この後 22:09:34 に外部要因でファイルが `~/Desktop/test/` へ移動（ジャーナルは全 DB 0 行で、kaseifu は移動を記録していない。§10.4）。

`organize <pdf> --format rich`（dry run）:

```text
+-- * KASEIFU / ORGANIZE ---------------------------+
| dry run (no files moved)                          |
|-- DECISION ---------------------------------------|
| Selected     D1 - ~/OS/home/.../State Of Mind Labs |
| Confidence   0.300                                |
| Margin       0.060                                |
|-- POLICY -----------------------------------------|
| [LEAVE] LEAVE                                     |
| confidence below review threshold                 |
|-- NO MOVE ----------------------------------------|
| policy refused: automatic move not allowed        |
+---------------------------------------------------+
```

（キャプチャツールの折り返しを除去して復元。幅は端末幅依存）

プレーン（`--format json` / 非 TTY）:

```text
== Mode ==
  dry run (no files moved)
== Decision ==
  Selected     D1 - ~/OS/home/.../State Of Mind Labs
  Confidence   0.300
  Margin       0.060
== Policy ==
  [LEAVE] LEAVE
  confidence below review threshold
== No move ==
  policy refused: automatic move not allowed
```

### 10.3 テスト・ゲート

| 検証 | 結果 |
|---|---|
| `cargo test --all-targets --all-features` | ✅ 296 passed / 0 failed（279 + 新規 17） |
| `bash scripts/quality_check.sh` | ✅ **OVERALL PASS**（fmt / clippy / test / complexity / baseline-gate / report-tests / coverage 74.08%） |
| 新規 CC16+ | ✅ なし（最大 CC 15 は既存。`policy_view` 移動は baseline-check が moved として許容） |
| 新規依存 | ✅ なし |

### 10.4 環境ノート（親に報告）

- 実 PDF は 22:09:34 に外部要因で `~/Desktop/` から `~/Desktop/test/` へ移動した（ディレクトリ mtime と ctime が一致）。存在する kaseifu のジャーナル（作業ツリー `data/kaseifu.db`、`~/Desktop/images/data/kaseifu.db`）はどちらも 0 行で、kaseifu は移動を記録していない（＝ kaseifu による移動ではない）。ファイルは無傷（126597 bytes）
- `--execute`（policy=Leave）の実機キャプチャは、作業中のリビルドで macOS の Keychain アクセス許可が失われ、許可ダイアログが未応答のため実行不能（`auth status` も同様にブロック）。`execute mode` 表示はテスト `execute_mode_is_a_mode_not_a_moved_claim` で固定済
- `--format json` は実装前後ともプレーン行（JSON ではない）。分岐挙動は不変で、表示改善のみ適用（§7）
