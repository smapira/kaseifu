# kaseifu デバッグシナリオ設計 成果物 v2（S01–S28 / C01–C12 統合）

原典（正・ユーザー管理・編集禁止）:

- v1.1: `documents/plans/user/20261013b.md`（kaseifu デバッグシナリオ設計書 v1.1）
- v1.2: `documents/plans/user/20261013c.md`（kaseifu CLIデバッグシナリオ追加指示書 v1.2、基準文書 = v1.1）

対象: worktree `kaseifu`、HEAD `6819ac9`（v0.4.2 `chore: parse .env with dotenvy; release v0.4.2`）。実測日 2026-10-13。

範囲（v1.1 §1 / v1.2 §13末尾）: **設計資料のみ**。テストコード・Mock・Fixture・CI 設定は作成しない。`src/`・`tests/` は変更しない。原典 2 ファイルも編集しない。

---

## 0. 成果物一覧（本ドキュメントが索引）

| # | 原典の要求 | 結合先 | ファイル |
|---|---|---|---|
| 1 | v1.1 §7-1 デバッグシナリオ一覧（D01–D16） | 本書 §2 | `20261014d-debug-scenario-design.md` |
| 2 | v1.1 §7-2/3 検証シナリオ S01–S20 の完全仕様 | 分割 2 冊 | `20261014d-scenarios-S01-S10.md` / `20261014d-scenarios-S11-S20.md` |
| 3 | v1.2 §12-1 S21–S28 詳細仕様 | 1 冊 | `20261014d-cli-scenarios-S21-S28.md` |
| 4 | v1.2 §12-2 C01–C12 横断仕様 | 1 冊 | `20261014d-cli-scenarios-C01-C12.md` |
| 5 | v1.1 §7-4 既存テストとの対応表（実測） | 1 冊 | `20261014d-existing-test-coverage.md` |
| 6 | v1.2 §12-3/4/5 CLI 対応表・カバレッジ表・重複と依存 | 1 冊 | `20261014d-cli-coverage-map.md` |
| 7 | v1.1 §7-5/6 優先度別実施順序・未検証領域とリスク | 本書 §3/§4/§5 | 本書 |
| 8 | v1.2 §12-7 未確認仕様と未カバー領域 | 本書 §5/§6/§8 + カバレッジ表 | 本書 + `20261014d-cli-coverage-map.md` |

検収記録（Plan Architect / Reviewer / QA）:

- `documents/plans/dev/issues/20261014-plan-architect-debug-scenario-review.md`
- `documents/plans/dev/issues/20261014-reviewer-debug-scenario.md`
- `documents/plans/dev/issues/20261014-qa-debug-scenario.md`

読み方: 期待結果はすべて現行実装（`src/`、`ファイル:行` 付き）に接地させる。実装と原典が食い違う箇所は「設計上の指摘」（§5）として明記し、コードは直していない。テストデータは「方針」のみで、Fixture は作っていない。

---

## 1. 設計方針

v1.1 §1 の 4 原則を、そのまま全シナリオの判定基準に使う。

1. ファイルが消失しないこと
2. 意図しない上書きが発生しないこと
3. ファイルとジャーナルの不整合を検出できること
4. 不正な分類結果によってファイルが移動されないこと

v1.2 が加える CLI 契約の観点（入出力契約 / 引数の組み合わせ / 終了コード / 副作用 / コマンド間整合）は、D01（CLI）と D13（Config/Auth）に落とし、C01–C12 で横断的に扱う。

各シナリオ仕様は v1.1 §5 のスキーマ（`scenario_id` / `title` / `priority` / `test_level` / `question` / `preconditions` / `inputs` / `expected` / `failure_conditions` / `existing_test` / `status`）で記述し、v1.2 §12 の要求に合わせて `observation_points`（観測ポイント）と `evidence`（実装根拠）を追加した。

- `status: DESIGNED` = 設計済み。テスト成功を意味しない（v1.1 §5 の定義を継承）。
- `test_level` は `UNIT` / `INTEGRATION`（テストバイナリ単位） / `E2E`（実 CLI プロセス + 実 Mac 環境）を使う。
- `existing_test` は `20261014d-existing-test-coverage.md` の実測結果（カバー済み / 一部 / 未カバー）を指す。

---

## 2. デバッグシナリオ一覧 D01–D16（検出基準つき）

「何をもって検出したとするか」= その領域のバグが存在すると判定できる最小の観測事実。「対応シナリオ」列が実際の手順を持つ。

| ID | 領域 | 検証対象 | 優先度 | 何をもって検出したとするか（検出基準） | 対応シナリオ |
|---|---|---|---|---|---|
| D01 | CLI | 引数・終了コード・Tab補完 | P2 | (a) 期待終了コード（0/1/2）と実測が不一致、(b) `--help` に出るコマンド集合と実行可能集合が不一致、(c) 補完候補集合とサブコマンド集合が不一致 | S21–S28（コマンド別）、C01–C12（横断） |
| D02 | Observe | 通常ファイル・Spotlight未索引 | P1 | `inspect` の `evidence` が空でも異常終了しない一方、値の捏造（`(null)` や空文字が値として現れる）または canonical 化の取り違えが 1 件でも観測される | S26、S07、S17 |
| D03 | Normalize | 欠損・異常値・日時 | P1 | `semantic_signals` に原典に無い語が現れる、`file_type` が欠落する、`mtime` が撮影日時として現れる、区切り文字の正規化が崩れる | S26、S07、S17 |
| D04 | Discover | 候補0件・複数件・除外・自己参照 | P0 | 除外対象（`*.library` / `~/Library` / 隠し / キャッシュ / 機械生成名 / システムパス）が候補に現れる、自身の親ディレクトリが候補になる、0 件時に空でない候補が返る | S02、S14、S27、S21 |
| D05 | Jev | 正常応答・認証・通信障害 | P1 | 失敗が `Err` ではなく `NONE`（正常な判断）に変換される、または失敗時にファイルが移動する | S08、S02 |
| D06 | Jev Validation | 未知ID・不正JSON・不正確率 | P0 | 候補に無い ID・パス・自由文が `AutoMove` に到達する、NaN/Inf/範囲外確率が confidence として採用される | S02、S08、S19 |
| D07 | Policy | confidence / margin 境界値 | P0 | 閾値ちょうど／直下の verdict が v1.1 §3 の表（= 実装の `>=`）と乖離する | S03 |
| D08 | Execute | Dry-run・実行制御 | P0 | dry-run でファイル移動・ジャーナル行・宛先作成のいずれかが起きる、`--execute` 無しで移動する | S09、S11、S28、C03/C04 |
| D09 | Execute Safety | 衝突・権限・競合状態 | P0 | 上書き・削除・内容変更が 1 件でも起きる、rename 失敗で移動元が失われる | S04、S10、S11、C11 |
| D10 | Journal | pending・success・異常中断 | P0 | ミューテーションが未記録のまま残る、失敗が `success` で記録される、`pending` が残っていることが検出できない | S05、S01、C11 |
| D11 | Undo | 復元・二重実行・衝突 | P0 | 同一 move が 2 回復元される、復元で上書きが起きる、FS とジャーナルが矛盾する | S01、S12、S13 |
| D12 | Watch | FSEvents・重複・安定性 | P0 | 同一ファイルが二重処理される、書き込み中ファイルを処理する、自己生成イベントが再処理される | S06、S18、S28 |
| D13 | Config/Auth | 設定優先順位・Keychain | P1 | 優先順位（CLI > env > TOML/Keychain > 既定）の逆転、秘密値の表示・ログ出力、TOML と Keychain の不一致 | S15、S16、S22–S25、C12 |
| D14 | Metadata | EXIF・HEIC・GPS保護 | P1 | 欠損値の推測補完（mtime を撮影日時にする等）、GPS が既定で Jev へ送信される、出典（`source`）の欠落 | S07、S15、S26 |
| D15 | Performance | 大量ファイル・高負荷 | P2 | メモリ／所要時間が入力サイズに対して非有界、候補上限（50）・ヒット上限（10,000）を超えて返る | S18、S27 |
| D16 | Regression | 更新前後の挙動比較 | P1 | 事前に記録した基準（終了コード・JSON 形状・候補集合）と比べて、意図しない差分が出る | S20、C02、S27 |

---

## 3. 優先度別 実施順序

v1.1 §7 末尾の最重要順（S05 → S01 → S04 → S02 → S06）を先頭に固定し、v1.2 の S28（P0）と残りの P0、P1、P2 を依存関係順に並べる。

| Wave | 順序 | シナリオ | 理由・依存関係 |
|---|---|---|---|
| W0 | 1–5 | **S05 → S01 → S04 → S02 → S06** | v1.1 §7 指定のファイル操作安全 5 本。S05 は S01/S04 の中断版、S01 は S04 の逆操作、S06 は S01 の入力となる |
| W1 | 6–12 | S28 → S03 → S11 → S12 → S13 → S09 → S10 | 残 P0。S28 は S06 の CLI 版（起動・停止・副作用）。S03 は D07 の境界、S11–S13 は D09/D11 の残り、S09/S10 は D08 の実行制御 |
| W2 | 13–14 | S07 → S08 | P1 だが P0 の入力（S07 は S03/S02 の判定材料、S08 は S02/S03 の前提）→ W1 の直後に置く |
| W3 | 15–21 | S14 → S27 → S21 → S22 → S23 → S24 → S25 | D04/D13 の P1。S27 は S14/S02 の CLI 版。S21–S25 はコマンド別（S21 は現行実装の再定義を含む） |
| W4 | 22–27 | S26 → S17 → S15 → S16 → S19 → S20 | 観測・横断。S26 は S07/S17 と共通ケース（v1.2 §8 の指定どおり重複を統合して扱う） |
| W5 | 28–30 | S18 → C01–C12（一括） → C08（補完、実装待ち） | P2 と横断。C 系は全コマンド横断なので最後に一括実施（C06/C07/C12 は各コマンド実施時に都度確認してよい） |

依存関係（1 行で）: 観測（D02/D03）→ 探索（D04）→ 分類・検証（D05/D06）→ 判定（D07）→ 計画・実行（D08/D09）→ 記録（D10）→ 復元（D11）。Watch（D12）は「実行 + 記録」に 1 周遅れで乗る。設定（D13）とメタデータ（D14）は全段の入力条件。

---

## 4. 未検証領域と安全上のリスク一覧（リスクの高い順）

「未確認」= コード上に検出・復旧の経路が見つからない、またはテストが 1 件もない領域。

| 順 | リスク | 内容 | 根拠（実装） | 未確認事項 | 対応シナリオ |
|---|---|---|---|---|---|
| R1 | **高** | rename 成功後に `mark_success` が失敗すると、ファイルは移動済み・ジャーナルは `pending` のまま残る。Undo は `result = 'success'` のみ対象なので**この移動は戻せない**（検出手段が無い） | `src/operations/mover.rs:93-99`（rename 後 mark_success、失敗時は pending のまま）、`src/storage/history.rs:140-154`（`result='success' AND undone_at IS NULL` のみ） | 自動復旧は未実装（v1.1 §3 S05 の注意と同じ）。`doctor` もジャーナルを検査しない（`src/doctor.rs:51-58`） | S05、S01、C11 |
| R2 | **高** | プロセス中断（Ctrl+C / クラッシュ）はハンドラ無しの既定終了で、`record_pending` と `rename` の間・`rename` と `mark_success` の間で落ちると `pending` が残留する | `src/main.rs:185-187`（`#[tokio::main] async fn main`）、`src/` に signal/SIGINT ハンドラ無し（grep 0 件）、`src/storage/history.rs:93-109`（INSERT）と `src/operations/mover.rs:93`（rename）は非トランザクション | 中断点ごとの残留状態は未確認。再実行時の検出 CLI が無い | S05、C11 |
| R3 | **高** | Undo の二重実行は「同じ move を 2 度戻さない」が、**2 回目はその前の成功 move を戻す**。`undo --last N` はそれを N 回繰り返す | `src/storage/history.rs:140-154`（`ORDER BY rowid DESC LIMIT 1`）、`src/main.rs:427-431`（`for _ in 0..last`） | 複数 move がある状態での 2 回目以降の期待は未設計 → S13 で定義 | S13 |
| R4 | **高** | Undo の「`mark_success` 成功 → `mark_undone` 失敗」では、undo 行は success、元 move は未 undone。再度の undo は `SourceMissing`（元の移動先にファイルが無い）として失敗し、原因が「ジャーナル不整合」に見えない | `src/operations/undo.rs:113-124` | 失敗時のメッセージと状態の対応は未検証 | S13、S05 |
| R5 | **中** | ジャーナルが CWD 相対（`data/kaseifu.db`）。実行ディレクトリを変えると別ジャーナルになり、Undo 対象が消える。`config/default.toml` の `[storage] database` とも別ファイル | `src/storage/history.rs:16`、`src/main.rs:295,382,425`、`src/watch/coordinator.rs:38-40`（managed_paths も相対）、`config/default.toml:36-37`（`data/semantic-file-organizer.db`） | どこを「正」とするかは未決定（設計上の指摘 F-05） | S01、S05、S13、C11 |
| R6 | **中** | Jev の確率分布は実行ごとに変動し得る。閾値近傍（confidence ≈ 0.85 / margin ≈ 0.40 / 0.60）では AutoMove と Review が反転する | `src/policy/decision.rs:76-79`（`>=`）、confidence/margin は応答確率から算出（`src/models/decision.rs:221-236`） | 実 Jev の分散は未計測（v1.1 S19 が観測を要求） | S19、S03 |
| R7 | **中** | 動画（MP4/MOV）は画像判定に含まれず、動画の撮影日時・GPS は Jev に渡らない。判定材料はファイル名と Spotlight の文字情報だけ | `src/context/image_metadata.rs:52-60`（`jpg/jpeg/heic/heif/hif/tif/tiff/png/webp` のみ）、`src/context/exif.rs:56-70`（他は `NoExif`） | 動画を対象に含めるかは未決定（設計上の指摘 F-11） | 新規（S21–S28 に動画シナリオ無し = 未カバー領域） |
| R8 | **中** | `is_system_directory` が部分一致（`/system`, `/private`, `/usr`, `/bin`, `/sbin`, `/var`, `/tmp`）なので、`~/var/...`、`~/system-backup`、`~/private-docs`、`~/tmp/...` のようなユーザーフォルダが候補から消える | `src/discovery/similarity.rs:199-207`（`path_lower.contains(p)`） | 過剰除外の実測は未実施。テスト 0 件 | S14、S27 |
| R9 | **中** | `--format json` が `organize` / `undo` / `scan` / `watch` で無効（常に人間向けテキスト） | `src/main.rs:268-280`（scan）、`307-321` + `326-361`（organize）、`420-433`（undo）、`402-417`（watch） | 自動化・CI での構造化取得は未提供（設計上の指摘 F-07） | C07、S09、S28 |
| R10 | **中** | `organize` / `watch` は dry-run でも Jev API を呼ぶ（オフラインでは検証不能） | `src/operations/organize.rs:97-98`（classify → Jev）、`src/main.rs:297-305` | オフライン時の期待挙動は未定義 | S09、S28、S08 |
| R11 | **低** | メタデータが全滅（Spotlight 未索引 + EXIF なし）でも推測しない代わりに、判定材料ゼロで Jev に渡る | `src/context/image_metadata.rs:79-98`（空なら `None`）、`src/classifier/jev.rs:449-490`（欠損時はフィールド省略） | 「材料ゼロの NONE」を誤検知として扱うかは未定義 | S07、S26 |
| R12 | **低** | 大量ファイル時はヒット上限（10,000）と候補上限（50）で切り捨てられ、切り捨ての可視性が debug ログのみ | `src/discovery/mod.rs:31,39,110-117`、`src/discovery/similarity.rs:46` | 切り捨て発生時の品質劣化は未計測 | S18、S27 |

---

## 5. 設計上の指摘（原典 ↔ 実装の食い違い / 精密化が必要な期待）

コードは修正していない。各項目は「原典の記述」「実装の事実」「扱い」の 3 点で書く。

| ID | 原典 | 実装の事実 | 扱い |
|---|---|---|---|
| F-01 | v1.1 §3 S03: 0.850/0.400 → AutoMove ほか 6 行の境界表 | `>=` 比較（`src/policy/decision.rs:77-78`）、閾値は README.md:213-215 と一致（0.85 / 0.40 / 0.60） | **一致**。表はそのまま採用。ただし原典に無い第 3 の経路（`destination_id` が `None` / `NONE` なら confidence に関係なく Leave、`src/policy/decision.rs:52-56`）を S03 に追加 |
| F-02 | v1.1 §6「対応候補の整理」 | 実測では S02/S08 は `phase4_*`、**S03 は `phase5_execute_test.rs`**、S07/S15 は `v32_*`、S16 は `v33_*`、S01/S04/S09–S13 は `phase5_*`、S06 は `phase6_*` | 食い違い（対応領域の割り当て誤り）→ `20261014d-existing-test-coverage.md` で事実に置換 |
| F-03 | v1.1 §4 S13「二重実行 → 同じ移動を二度復元しない」 | `mark_undone` 済みは対象外（`src/storage/history.rs:145`）なので同一 move は戻らない。ただし 2 回目は**別の move を戻す**（`ORDER BY rowid DESC LIMIT 1` + `--last` ループ、`src/main.rs:427-431`） | 期待を精密化（S13: 単一 move なら `NothingToUndo` / 複数 move なら次の move が対象） |
| F-04 | v1.1 §3 S05 の障害点表「rename 直前 = 移動前の状態を識別できる」 | 実装は verify → `record_pending` → rename → `mark_success`（`src/operations/mover.rs:80-99`）。衝突・権限などの検証失敗は**ジャーナル行を作らない**（`src/operations/mover.rs:80-91` の前に `verify_preconditions`） | 障害点表を実装の 4 段階に対応づけ、各点の期待状態を明記（S05） |
| F-05 | v1.1 §1 パイプライン / T43「data/kaseifu.db に成功行」 | 実装の定数は `data/kaseifu.db`（`src/storage/history.rs:16`）だが、`config/default.toml:36-37` は `data/semantic-file-organizer.db`。`[storage]` はユーザー TOML のスキーマに存在せず（`src/config/loader.rs:12-23` の 4 セクションのみ + `deny_unknown_fields`）、`[scanner]` / `[context]` も誰も読まない（grep 0 件） | 設計上の指摘（設定ファイルの死んだ記述）。シナリオでは `data/kaseifu.db`（CWD 相対）を実測対象と明記 |
| F-06 | v1.2 §13「公開CLIコマンド 11 種類すべてにシナリオが対応」 | 実測 `--help` は 11（inspect, scan, organize, undo, classify, watch, setup, auth, config, doctor, help）。`discover` は `#[command(hide = true)]`（`src/main.rs:88`）で隠し・デバッグ専用。`completions` は HEAD に存在しない（grep 0 件、並行タスクが追加中） | 11 種類で整合。**discover を 12 番目の対象として明示**し、`completions` は未実装として扱う |
| F-07 | v1.2 §11 C07「stdout / stderr の分離」 | 実測: 正常出力は stdout、エラーは stderr（例: `inspect` の `Error: cannot resolve ...` は stderr のみ）。ただし `organize` / `undo` / `scan` / `watch` は `--format json` でも JSON を出さない | 契約は「stdout = 人間向けテキスト（JSON 対応コマンドのみ JSON）、stderr = 診断 + エラー」。未対応は未カバー項目として明記 |
| F-08 | v1.2 §7 S25-10「診断による不要な副作用がない」 | `doctor` は `--check-connection` 無しでネットワークを触らない（`src/doctor.rs:143-151`）が、Storage チェックで app ディレクトリを作成する（`src/doctor.rs:128-134`） | 「ネットワーク副作用なし・ディレクトリ作成のみ」に精密化 |
| F-09 | v1.2 §11 C08 Shell Tab Completion | `completions` サブコマンドも生成コードも存在しない（HEAD `6819ac9`、`clap_complete` 依存なし） | **未実装**（設計上の提案として S/C の期待を記述。並行タスクの完了後に再評価） |
| F-10 | v1.2 §4 S22「設定ファイルと Keychain の更新順序」 | `save_to(TOML)` → `store.set(Keychain)` の順（`src/main.rs:742-753`）。Keychain 失敗時は TOML が更新済みの部分状態で終了 | 失敗時の識別手順を S22 に明記（部分状態の検出方法） |
| F-11 | v1.2 §3 S21「ディレクトリを走査しファイル集合を出力」 | `scan` は**バナー出力のみ**（`src/main.rs:268-280`、doc コメントも `target banner only (no mutation, no discovery)`）。実際の走査は `discover` / `classify` / `inspect` が担う | 食い違い（機能期待の差）。S21 は現行契約（バナー + 引数/エラー契約）に再定義し、走査系の期待は S26/S27 に移す |
| F-12 | v1.2 §6 S24-09「CLI・環境変数・設定ファイルの競合」 | 実装は CLI > env（`.env` は env の下位フォールバック）> ユーザー TOML > 既定（`src/config/resolver.rs:191-226,252-269`）。API キーのみ env > Keychain（`src/config/resolver.rs:228-245`） | 一致。`operations.dry_run` / `operations.enable_undo` は解決されるが**挙動に未接続**（`src/main.rs` は CLI フラグのみ使用）→ 未確認仕様として明記 |
| F-13 | v1.1 §3 S07「HEIC のメタデータ取得失敗」 | HEIC/HEIF/HIF は拡張子で EXIF フォールバック対象（`src/context/image_metadata.rs:63-71`）、失敗時は `Unsupported` / 破損は `Corrupt` で推測しない（`src/context/exif.rs:56-70`） | 一致。期待を「取得失敗は無情報として扱い、値を作らない」に明記 |
| F-14 | v1.1 §6 の `tests/user-test-plan.csv`（全シナリオとの照合） | CSV は T01–T84 の手動チェックリストで `Pass` 列は全行空（未実施） | 「未実施の手動チェックリスト」として扱い、自動シナリオと混同しない |

---

## 6. テストデータ方針（Fixture は作らない）

リポジトリへは何もコピーしない。実測で確認済みの既存素材をシナリオの入力方針として参照する。

| 素材 | 用途（シナリオ） | 備考 |
|---|---|---|
| `~/Desktop/test/IMG_2741.jpeg`（約 5.8 MB） | 画像系: S07、S15、S26、S27、S03（実写真での判定再現） | EXIF/Spotlight の有無は実行時に観測する |
| `~/Desktop/test/THCO-COA-Non-Detect Updated.pdf`（約 127 KB、空白を含む名前） | 非画像系: S27、C10（スペース入りパス）、S20 の回帰基準 | v1.1 系の PDF 回帰（D1 = score 1.00 の実測記録が v0.4.2c 計画書にある） |
| `~/Desktop/test/vape_ad_20260913_v2_final.MP4`（約 5.7 MB） | **動画 = 現行シナリオ無し**: 未カバー領域の代表 | 画像判定の対象外であることの観測（R7 / F-11） |
| `~/Desktop/test/video_621038756216701629-5riO1bQe.MP4`（約 6.4 MB） | 上と同じ（機械生成風の名前 = 除外規則 R5 の観測にも使える） | 名前が `SERIAL/HASH` 形状の観測例 |
| 一時ディレクトリ（`/tmp/kaseifu-*` 相当） | S01/S04/S05/S09–S13 の移動・衝突・復元 | シナリオ実行時に作る（本書では作り方を方針としてのみ記載） |
| リポジトリ既存フィクスチャ | 参考: `tests/*.rs` は `target/<fixture>` 配下に作る（ignore 済） | 新規 Fixture コードは本作業の範囲外 |

禁止事項（全シナリオ共通）: 実ユーザーの絶対パス・API キー・GPS 座標を出力・記録しない。パスは `~/` 表記と `<hash>` / `<user>` プレースホルダで書く。UI 例に非 ASCII 罫線を使わない（`+--` `|` のみ）。

---

## 7. ツール根拠（推測で書かないための確認記録）

| 用途 | 実行/参照 | 得た事実 |
|---|---|---|
| 原典の読解 | `read_file: documents/plans/user/20261013b.md`、`documents/plans/user/20261013c.md` | S01–S20 の設計、§7 の成果物範囲と最重要順、v1.2 の S21–S28・C01–C12 と完了条件 |
| 実装の接地（policy） | `read_file: src/policy/decision.rs` | `>=` 比較、`NONE`/`None` の分岐、閾値定数と `Thresholds` |
| 実装の接地（execute/journal/undo） | `read_file: src/operations/{mover,plan,undo,organize}.rs`、`src/storage/history.rs`、`documents/design/05-execute.md` | record → rename → mark の順序、衝突の位置、`last_successful_move` の条件 |
| 実装の接地（watch） | `read_file: src/watch/{coordinator,runner}.rs`、`src/platform/macos/fsevents.rs` | 500ms / 1s / 10s の既定、抑制理由、自己イベント 2 パス登録、エラー分離 |
| 実装の接地（metadata/privacy） | `read_file: src/context/image_metadata.rs`、`src/normalize/image.rs`、`src/context/exif.rs`、`src/classifier/jev.rs` | 出典保持、欠損を推測しない、GPS 既定 OFF、マーカーは `image_metadata` から導出 |
| 実装の接地（discovery） | `read_file: src/discovery/{destinations,similarity,candidates,mod,search_terms}.rs` | 除外規則、システムパス部分一致、自身の親除外、上限 50 / 10,000、D1.. 連番 |
| 実装の接地（CLI/config/auth） | `read_file: src/main.rs`、`src/config/{resolver,loader,defaults,storage}.rs`、`src/auth/keychain.rs`、`src/doctor.rs`、`src/presentation/format.rs` | 優先順位、`--format` の効き方、TOML→Keychain の順、doctor の副作用 |
| 既存テストの実測 | `cargo test --all-targets --all-features`（2026-10-13 実行） | **279 passed / 0 failed**（lib 51 + 統合 228）。テストバイナリ別の内訳は `20261014d-existing-test-coverage.md` |
| CLI 契約の実測 | `./target/debug/kaseifu {--version,--help,foo,organize --execute --dry-run,watch,watch --path <file>,inspect <missing>,undo --last 0,scan --format json}` | 終了コード 0/1/2、stdout/stderr 分離、`--format json` の非対応コマンド、`--help` の 11 コマンド |
| テストデータ実測 | `ls -la ~/Desktop/test/` | 上記 4 件（画像 1・PDF 1・MP4 2） |
| 未使用設定の確認 | `grep -rn "max_directory_entries|use_spotlight_metadata|walkdir|dry_run" src/ Cargo.toml` | `[scanner]` / `[context]` / `walkdir` / `operations.dry_run` / `operations.enable_undo` は挙動に未接続 |

---

## 8. 実装済み仕様と設計上の提案の区別

| 区分 | 項目 |
|---|---|
| **実装済み（現行コードが保証）** | Policy の `>=` 境界と `NONE`/`None` の Leave（`src/policy/decision.rs:51-79`）/ 候補外 ID・パスの拒否（`src/models/decision.rs:115-132`、`src/classifier/pipeline.rs:238-245`）/ 確率の厳密検証（`src/models/decision.rs:241-249`）/ verify → record → rename → mark（`src/operations/mover.rs:79-107`）/ 衝突の非上書き（`src/operations/plan.rs:97-100`、`src/operations/mover.rs:55-57`）/ Undo の非上書き（`src/operations/undo.rs:76-78`）/ 未成功 move のみ Undo 対象（`src/storage/history.rs:140-154`）/ debounce・stability・自己抑制・in-flight 排除（`src/watch/coordinator.rs:113-247`）/ 欠損メタデータを推測しない（`src/normalize/image.rs:12-70`）/ GPS 既定 OFF（`src/config/defaults.rs:34`）/ 除外規則 R1–R6（`src/discovery/destinations.rs:66-170`）/ 秘密の非表示（`src/config/resolver.rs:30-34`、`src/auth/keychain.rs:19-34`）/ 終了コード 0/1/2（実測） |
| **設計上の提案（未実装・原典は要求/示唆）** | `completions` サブコマンド（C08）/ `pending` 滞留の検出 CLI（R1・R2）/ ジャーナルの絶対パス化（R5）/ `organize`・`undo`・`scan`・`watch` の JSON 出力（R9）/ 動画（MP4/MOV）のメタデータ扱い（R7）/ `operations.dry_run`・`operations.enable_undo`・`[scanner]`・`[context]`・`[storage]` の配線（F-05, F-12）/ `is_system_directory` の規則ベース化（R8） |

---

## 9. 完了条件への対応

v1.1 §7: (1)(2)(3) → S01–S20 の 2 冊、(4) → テスト対応表、(5) → 本書 §3 + 対応表 §6、(6) → 本書 §4。テストコード等の実装は含まない（v1.1 §7 末尾）。

v1.2 §13: 公開 11 コマンド（+ 隠し `discover`）にシナリオを対応（カバレッジ表）/ サブコマンド（`auth` 3、`config` 2）を対象化 / 正常系・異常系・境界値は各シナリオの `inputs` と `failure_conditions` で整理 / 反証可能な期待結果は `expected` を「観測できる値」（終了コード・行の有無・状態語）で記述 / 重複は対応表 §5 で明示 / 未カバーは §4・§8 とカバレッジ表の `covered: no` / 実装済みと提案は §8 で分離。

---

## 10. 検収記録

| 工程 | 成果物 | 判定 |
|---|---|---|
| Plan Architect | `issues/20261014-plan-architect-debug-scenario-review.md` | 計画の指摘を反映済み（本書 §5・各シナリオの `evidence`/`observation_points` 追加、S21 の再定義、S13 の精密化） |
| Reviewer | `issues/20261014-reviewer-debug-scenario.md` | 検収レビュー（下記） |
| QA | `issues/20261014-qa-debug-scenario.md` | 品質チェック（下記） |
