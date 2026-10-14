# AIリネーム Phase 8（Batch Rename）実装計画

## 0. 位置づけと進め方

- 原典: `documents/plans/user/20261014a.md` の §8.5（一括処理のプレビュー例）、§9.3（Journal と異常終了時の復旧）、§10（一括リネームも戻せる）、§11.2（Batch Processing / Bounded Concurrency / Early Exit）、§12（失敗した単位はスキップ）、§13.6（SAFE10 プロセス異常終了、SAFE11 TOCTOU）、§14 Phase 8、§15。
- 作業基点: `e6104eb`（Phase 7 着地、774 passed / gate PASS）。
- **本ラウンドの進め方（正直な記録）**: サブエージェントへの委任を2回試み、いずれも書き込み前に停止した（隔離 worktree は base commit のまま clean で回収物なし）。以降は親スレッドが計画・実装・検証を直接行い、reviewer / QA の記録は `issues/` の3件に**手順をなぞった自己検証として**残す（p5b・p7 の reviewer 記録と同じ扱い）。

## 1. ベースライン（実測）

- 実装前: `cargo test --all-targets --all-features` = **774 passed / 0 failed / 4 ignored**、`bash scripts/quality_check.sh` = OVERALL PASS。
- 実装後: **791 passed / 0 failed / 4 ignored**（+17 = 単体 8 + 統合 9。既存テストの assert は 1 件も変えていない）。
- `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --all-targets --all-features` = **791 / 0 / 4**。
- `bash scripts/quality_check.sh` = OVERALL PASS（rustfmt 0、clippy 0、関数 1369、CC<=10 1356（99.1%）、CC 11-15 = 13、**CC>=16 = 0**、最大 CC 15（ベースライン同）、baseline-gate PASS、coverage 84.91% report-only）。
- `Cargo.toml` / `Cargo.lock` 無変更（新規 crate なし）。

## 2. 利用する土台の実測再確認（file:line）

- `OperationHistory::last_successful_move` は `operation = 'move' AND result = 'success' AND undone_at IS NULL` だけを選ぶ（`src/storage/history.rs`）。**一括の行も `operation = 'move'` のまま**にする必要がある。
- journal の列は `id, timestamp, operation, source_path, destination_path, candidate_id, confidence, margin, result, error, undone_at` で、バッチ/グループ列は無い。
- `pending_entries()` の利用者は `main.rs::print_pending_warning` のみ。「自動復旧はしない」という既存判断をそのまま引き継ぐ。
- 再帰ウォーカーはリポジトリに存在しない（`context/builder.rs` は親ディレクトリ、`src/context/directory.rs` は深さ1、`organize` は単一ファイル、`operations/mover.rs` は単一 plan）。
- Phase 7 の `context::directory::{refuse_reason, RefusalEnv, is_bundle_container, is_library_tree, is_cache_or_temp, is_system_area, is_hidden_name}` と `rename::validate::{snapshot_directory, recheck_directory_before_execute}`（dev/ino）を再利用する。
- 対話確認の先例: `src/auth/prompt.rs`（`IsTerminal`）と `main.rs::handle_organize_select`（非 TTY なら作業前に拒否 + `--yes`）。
- `emit_report` / `resolve_mode` は `+--` パネルと素の行を切り替える。パネルは長い行を折り返すため、**項目一覧は常に素の行**にする。

## 3. スコープ

### 3.1 対象（今回実装する）

- `kaseifu rename <folder> --recursive`（原典 §8.5）。プレビュー、衝突検出、実行計画、一括 Undo、非 TTY での非ブロッキング、上限の明示。
- `--yes`（TTY の確認を飛ばす）。
- 変わる可視面: `rename --help`、guidance、`README.md`、`--format json`（バッチは新しい形）。
- テスト `tests/v097_ai_rename_p8_batch_test.rs` と `src/rename/batch.rs` の単体テスト。

### 3.2 対象外（着手しない）

- Phase 9（推論時間・セッション再利用・大規模評価・メモリ）、OCR、TREC 本評価、`--to <dir>`（ZC-14）。
- 単一ファイル / 単一フォルダ / 画像 / 動画の既存挙動（不変）。複数の位置引数（`rename a b c`）は導入しない。
- 対話的な項目選択（`organize --select` 相当）や、プレビュー内での個別除外。

## 4. 設計判断（選択肢と結論）

### D1. 起動形（`--recursive` と Phase 7 の共存）

- 採用: `rename <folder>` はフォルダ自身を改名（Phase 7 のまま）、`rename <folder> --recursive` は**中身**を改名。ファイルに `--recursive` を付けたら明示エラー（`--recursive needs a folder`）。位置引数は1つのまま。
- 却下: 複数の位置引数（原典に無く、`--patterns` などの管理フラグとの衝突規則が増える）、`--recursive` を拡張子なしのディレクトリ全般の既定にする（Phase 7 の意味を壊す）。

### D2. プレビュー（原典 §8.5）

- 採用: サマリを `+--` パネル（rich 時）または素の `key: value` 行で出し、**番号付きの項目行は常に素の行**で出す。原典の `->`（矢印）は ASCII の `->` に置き換える（本ツールは非 ASCII と em dash を出力しない）。1行1項目: `N. current -> proposal  [note]`。
- サマリ: `folder` / `proposals` / `conflicts` / `held` / `noop` / （上限に当たったときだけ）`bounded` / `status` / `mode`（または `order` + `undo`）。
- 却下: 項目行もパネルに入れる（狭い端末で折り返して読めない。実測で長い提案名が折り返された）、番号を振らない（原典の形と `undo --last N` の件数が対応しない）。

### D3. 衝突検出（本ラウンドの核）

- 採用: Ready な項目を **(宛先のディレクトリ, 宛先ファイル名を小文字化したもの)** でまとめ、**2件以上ある群は全員を `conflict` で保留**にする。理由には共有された名前を1回だけ書く。
  - 大文字小文字だけ違う2件は、既定の macOS ボリュームでは同じ1ファイルなので衝突として扱う。
  - 宛先が既に存在する場合は単一ターゲットの既存規則（`check_destination_free`）が既に保留にしているので、ここでは扱わない。
  - 連鎖（`a.txt -> b.txt` と `b.txt -> c.txt`）は、前者の宛先 `b.txt` が存在するため**計画時点で既に保留**になり、2件同時に Ready にはならない（実測）。
  - Unicode 正規化差はここでは畳まない（正規化表を積むには新規 crate が要る＝禁止）。代わりに適用時の項目ごとの再確認が「既に在る」を検出して**上書きせず**その項目だけ失敗させる（§9 で実測）。
- 却下: トポロジカルに並べて連鎖を成功させる（実行順序の正しさを名前の推移に依存させると、失敗時の状態が読みにくい）、正規化表の導入（依存追加）、衝突を警告だけで通す（原典 §9.1 の「同名ファイルは上書き禁止」に反する）。

### D4. 変更の入口（`--execute` と確認）

- 採用: 実際に改名するのは `--execute` だけ（既存の `rename` / `organize` / `watch` と同じ）。加えて **stdout が端末で、`--format json` でなく、`--yes` が無いとき**だけ `Apply changes? [y/N]` を出す（原典 §8.5 の UX）。既定は No。非 TTY では**問い合わせない**（パイプがハングしない）。JSON は 1 文書に保つため問い合わせない。
- 実測: PTY ではプロンプトが出て入力待ちになる（確認として正しい）。`--yes` で出ない。`--execute` を `/dev/null` から実行（非 TTY）では出ず、そのまま適用される。
- 却下: 非 TTY でも確認する（ハングする）、JSON でも確認して2文書を出す（パース不能）、`--execute` をやめて確認だけにする（既存の全コマンドの作法を壊す）。

### D5. 上限と除外

- 採用: 深さ **3**（与えたフォルダの直下が1）、対象 **50 件**、1ディレクトリあたり 2000 entry。名前はソート順。除外は `is_noise_file` / `is_hidden_name` / `is_bundle_container` / `is_library_tree` / `is_cache_or_temp` / `is_system_area` を**子の名前1つ**に対して適用（除外したフォルダには潜らないので部分木ごと外れる）。シンボリックリンクは改名も追跡もしない。
- 上限に当たったら `bounded:` 行で正直に言う（黙って切らない）。
- 却下: 無制限の再帰（原典 §5.4 が禁止）、深さの制限だけ／件数の制限だけ（片方だけでは「深いが広くない」または「浅いが広い」で破綻する）。

### D6. 一括 Undo

- 採用: **既存の `undo --last N`** をそのまま使う。1件の改名が1行なので、バッチは「N行を新しい順に戻す」であり、結果表示に `undo: kaseifu undo --last N` を出す。journal は**書き換えない**。スキーマ変更なし。
- 却下: journal にバッチ id 列を足す（既存ユーザー DB のマイグレーションが必要で、原典 §16 の「新しい設定ファイル・別DBなし」の精神に照らして得られる精度が小さい）、`operation` を `move-batch` にする（**既存の undo が拾わなくなる**。実測で確認）。
- 失敗モードの比較（計画に明記した論点）: バッチ直後に単発改名した場合、`undo --last N` はバッチの行だけを狙わない。ただし新しい順に戻るので**状態は常に一貫**し、余分な行も戻るだけである（取り消せない変化は起きない）。バッチ途中で中断した場合は、実行済みの行だけが `success`、実行中だった1行が `pending` として残り、`undo` は `pending` を無視して警告する（§10 の判断）。

### D7. 適用順序

- 採用: **深い順（子が先）**。フォルダの改名は中のパスを無効化するため、子を先に改名し、親を後に改名する。`undo` は新しい順なので親が先に戻り、その後に子の行が解決できる（Phase 7 と同じ保証）。同深度はパス順で決定的。
- 実測: `tree/d/note.txt` と `tree/d` の2件で、子→親の順に適用され `tree/<新名>/<新名>.txt` になった（逆順なら子の plan が `SourceMissing` で失敗する）。`undo --last 2` で元の `tree/d/note.txt` に戻る。
- 却下: プレビュー順（親が先）で適用する（子の plan が必ず1件失敗する）、包含関係のある組を保留にする（ツリーの一括改名という主用途を拒否することになる）。
- 表示は歩いた順（親が先）のままとし、`order: applied children before parents (deepest first)` を結果に明記して差を隠さない。

### D8. モデルが使えないとき

- 採用: ウォーク後、対象が1件以上あるときだけ `provider.availability()` を1回確認し、使えなければ**何も改名せずに**理由付きで終了する（原典 §15「安全に終了する」）。対象0件ならモデルを要求しない（空フォルダは空のバッチ）。
- 却下: 対象ごとに同じ失敗を繰り返す（時間だけ消費する）、unavailable を held として続行する（1件も提案できないことが分かっている実行を続ける意味がない）。

### D9. 異常終了（SAFE10 / §9.3）

- 採用: 各行は改名の**前**に `pending` で記録されるので、途中で SIGKILL されても「実行中だった1行」が残る。`undo` は既存どおり `pending` を選択せず警告だけ出す。自動復旧はしない（既存判断の継続）。バッチは1行ずつ進むため、中断しても**完了済みの改名は確定**し、それは `undo` で戻せる。
- 却下: 開始時にバッチ全体を1トランザクションとして記録する（journal は追記型で、途中状態の意味が壊れる）、クラッシュ復旧でファイルを勝手に戻す（正しい状態を壊しうる）。

### D10. 並行度

- 採用: **逐次（同時1）**。オンデバイスのモデルは1つの資源で、原典 §11.2 の Bounded Concurrency は「上限を設ける」であり1も上限である。実測の生成は1件あたり数秒〜十数秒で、並列化の利得より失敗時の切り分けの複雑さが勝つ。
- 却下: 固定本数の並列（セッションの取り合いが計測できない、失敗の再現性が落ちる）。Phase 9 の計測対象として記録する。

### D11. 変わるユーザー可視面

- `rename --help`（`--recursive` / `--yes` の説明と、フォルダの説明に1文）、`rename` の guidance（`batch:` 行）、`README.md`（`--recursive` の例とバッチの項、JSON の項）。内部 Phase 語は出さない（`tests/v070_zero_config_p1_help_test.rs` が全 help を走査）。
- `--format json`: 単一ターゲットの形は不変で、バッチは `status` / `target` / `proposals` / `conflicts` / `held` / `noop` / `truncated` / `applied` / `undo_last` / `items` を持つ新しい形（各 item は単一形のキー名を再利用）。

## 5. BDD シナリオ

### バッチ境界（`src/rename/batch.rs`）

- B1 上限: 深さ4の入れ子を作る → 深さ3までしか列挙しない（`level2/level3/level4` まで）。
- B2 除外: `.DS_Store` / `.hidden` / `App.app` / `Caches` / `Library` / シンボリックリンク混在 → 素の1ファイルだけが残る。
- B3 件数: 55件 → 50件で打ち切り `truncated = true`。
- B4 衝突: 同一ディレクトリで同じ提案2件 + 大文字違い1件 → 3件すべて `Conflict`、別名1件は Ready。
- B5 ディレクトリが違えば衝突しない。
- B6 順序: 親・子・兄弟の混在 → 子が先、同深度はパス順。
- B7 集計: proposals / conflicts / held / noop / recorded が対応する。

### 配線（`src/main.rs`）

- C1 `--recursive` にファイル → 明示エラー。
- C2 `--yes` を `--recursive` 無しに → clap エラー（フラグが必要だと名指しする）。
- C3 空フォルダ → `proposals: 0` で exit 0、モデル要求なし（`bounded:` も出ない）。JSON は `status: proposed` / `proposals: 0` / `truncated: false` / `items: []`。
- C4 guidance に `--recursive` が出る。
- C5 非 TTY の `--execute` は問い合わせず完走する（適用されたか、モデル不可の明示理由か）。

### 実機（合成ツリーのみ、隔離 HOME）

- R1 プレビュー: 2件（フォルダ + 子）で 13.7 秒、`proposals: 2` / `status: proposed` / `mode: dry-run`。
- R2 衝突: `--template reports` を2ファイルに当てる → `proposals: 0` / `conflicts: 2`、両方 `[conflict: two or more items propose the same name (reports.txt); all are held]`、ファイルは不変（12.0 秒）。
- R3 実行: `--execute`（非 TTY）→ `status: applied`、フォルダと子が改名（13.4 秒）。
- R4 一括 Undo: `undo --last 2` → フォルダが先に戻り、続いて子が `note.txt` に戻る。
- R5 上限: 55件（拡張子非対応なのでモデル呼び出し0回）→ `held: 50` + `bounded:`、0.024 秒。
- R6 非 TTY: `--execute` を `/dev/null` から → プロンプト無し。PTY では出る。`--yes` で出ない。

## 6. タスク一覧（実装順）

- T1 `src/rename/batch.rs`: ウォーク（B1-B3）、集合衝突（B4-B5）、集計（B7）。
- T2 適用順序（B6）。
- T3 `plan` / `apply` をライブラリ側に置く（fake provider でテストするため）。
- T4 `main.rs`: `--recursive` / `--yes`、`rename_batch`、`rename_options` の抽出、`require_model`、`confirmed_by_prompt`。
- T5 表示（サマリ + 項目行）と JSON（C3-R5）。
- T6 help / guidance / README。
- T7 `tests/v097_ai_rename_p8_batch_test.rs`（C1-C5 + ライブラリ側の衝突・順序・部分失敗）。
- T8 ゲートと実機検証、検収記録。

## 7. 検証手段（実測で示す）

- `cargo test --all-targets --all-features`（774 -> 791、既存 assert の変更なし）。
- `KASEIFU_FM_FORCE_STUB=no-sdk` で同スイート（791）。
- `bash scripts/quality_check.sh`（CC>=16 = 0、最大 CC 15）。
- 実機: R1-R6 を隔離 HOME（`/tmp/p8-verify/home`）と合成ツリーのみで実行。

## 8. 受け入れ基準（原典のうち本ラウンド分）との対応

| 原典 | 実装 | 実測の証跡 |
|---|---|---|
| §8.5 一括処理 / `--recursive` | `batch::walk` + `rename_batch` | R1、C1 |
| §8.5 変更一覧の提示（番号付き・件数） | `emit_batch_report` / `batch_item_lines` | R1、C3 |
| §8.5 衝突の件数 | `batch::hold_conflicts` | R2、B4 |
| §8.5 `Apply changes? [y/N]` | `confirmed_by_prompt` | R6 |
| §9.1 同名は上書き禁止 | 既存 `check_destination_free` + 適用時の再確認 + 集合衝突 | R2、v097 の部分失敗テスト |
| §9.1 Unicode / 大文字小文字 | 小文字化した名前で衝突を畳む（正規化差は適用時の再確認が担う） | B4、§10 のリスク |
| §9.2 TOCTOU | 既存の file / folder の再確認をそのまま使う | v097 の部分失敗テスト |
| §9.3 / SAFE10 異常終了 | 1行ずつ `pending` -> `success`、`undo` は `pending` を無視して警告 | §9.6 |
| §10 一括リネームも戻せる | `undo --last N` + 結果の `undo:` 行 | R3、R4 |
| §11.2 Batch / Bounded Concurrency / Early Exit | 逐次、上限50、深さ3、非対応は着手前に held | R5、B2、B3、D10 |
| §12 失敗した単位はスキップ | 1件の失敗を `Applied::Failed` にして続行 | v097 の部分失敗テスト |
| §13.6 SAFE06 / SAFE07 / SAFE08 / SAFE09 | dry-run 既定、`--execute`、`undo`、二重 undo 防止 | R1、R3、R4、C5 |
| §14 Phase 8 再帰 / プレビュー / 衝突 / 実行計画 / 一括 Undo | 本ラウンドの全項目 | R1-R5 |
| §15 一括の安全な終了（モデル不可） | `require_model` | C5 の分岐、v097 |

## 9. 検収記録（実測）

### 9.1 テストとゲート

- `cargo test --all-targets --all-features` = **791 passed / 0 failed / 4 ignored**（ベースライン 774 + 17 = 単体 8 + 統合 9。既存 774 の assert は 1 件も変更していない）。
- `KASEIFU_FM_FORCE_STUB=no-sdk` = **791 / 0 / 4**（`v097_cli_never_waits_for_input_without_a_terminal` はモデル不可の明示理由を受容する分岐を通る）。
- `bash scripts/quality_check.sh` = **OVERALL: PASS**。rustfmt 0、clippy 0、関数 1369、CC<=10 1356（99.1%）、CC 11-15 = 13、CC>=16 = 0、最大 CC 15（ベースライン同）、coverage 84.91%。新規関数の最大は `batch_item_note` 5 / `walk_level` 5 / `hold_conflicts` 4 / `batch_item_status` 5 / `batch_summary_lines` 5 / `rename_batch` 6。
- `git status --short`: 変更 3（`README.md`、`src/main.rs`、`src/rename/mod.rs`）+ 新規 2（`src/rename/batch.rs`、`tests/v097_ai_rename_p8_batch_test.rs`）+ 本計画と issue 3件。`Cargo.toml` / `Cargo.lock` は無変更。

### 9.2 実機（隔離 HOME=/tmp/p8-verify/home、合成ツリーのみ）

- **R1 プレビュー**（`tree/` = `d/note.txt`、13.7 秒）: `proposals: 2` / `conflicts: 0` / `held: 0` / `status: proposed` / `mode: dry-run (pass --execute to rename)`。項目は `1. d -> 2026-10-14_...` と `2. note.txt -> 2026-10-14_....txt`（提案は子の本文由来）。
- **R2 衝突**（`conflict-zone/` に内容の同じ2ファイル、`--template reports`、12.0 秒）: `proposals: 0` / `conflicts: 2` / `held: 0`、両項目が `[conflict: two or more items propose the same name (reports.txt); all are held]`、**ファイルは1つも動いていない**。
- **R3 実行**（`--execute`、stdin を `/dev/null`、13.4 秒）: `status: applied` / `order: applied children before parents (deepest first)` / `undo: kaseifu undo --last 2`、両項目に `[renamed op-...]`。結果のツリーは `tree/<新名フォルダ>/<新名>.txt` で、**子が先に適用された**ことを示す（逆順なら子の plan が `SourceMissing` で失敗する）。
- **R4 一括 Undo**: `undo --last 2` -> `restored: /private/tmp/p8-verify/work/tree/d`（フォルダ）に続いて `restored: .../tree/d/note.txt`（子）。ツリーは `tree/d/note.txt` に戻った。
- **R5 上限**（55件、拡張子が非対応なのでモデル呼び出し0回、0.024 秒）: `held: 50` + `bounded: the folder holds more than 50 targets; only those were planned` + `status: proposed`。JSON は 11 キー（`applied` / `conflicts` / `held` / `items` / `noop` / `proposals` / `status` / `target` / `truncated` / `undo_last`、item は `current_name` / `date_source` / `operation_id` / `pattern` / `proposal` / `reason` / `status` / `target`）。
- **R6 確認**: PTY では `--execute` が `Apply changes? [y/N]` を出して入力待ちになる（確認として正しい挙動。実測で観測）。`--yes --execute` は PTY でも質問せず完走（`status: not applied`）。`--execute` を `/dev/null` から（非 TTY）では質問せず、同じ入力で `status: applied` になった。
- **非退行**: `rename <file>` の単一経路は不変（v092-v096 が無変更で通過、`--recursive` 無しのフォルダ改名も Phase 7 のまま）。

### 9.3 実装中に確定した追加判断

- **`Confirmed` 表示の誤り**: `--execute` で確認待ちに入るとき、直前のプレビューが `mode: dry-run (pass --execute to rename)` と出ていた（実機で検出）。`BatchPhase::{Preview, Awaiting, Result}` を導入し、待機中は `mode: execute (waiting for confirmation)`、結果は `status: applied` 系 + `order` + `undo` を出すようにした。
- **衝突理由の重複**: 同じ提案名が2件並ぶと理由に同じ名前が2回出ていた（実機で検出）。理由を作るときにソート + 重複除去して1回だけ書く。
- **`plan` / `apply` をライブラリに置いた**: 当初 `main.rs` に書いたが、fake provider で検証できないため `src/rename/batch.rs` へ移した（CLI テストはモデル依存になる）。
- **空フォルダはモデルを要求しない**: `require_model` をウォークの後、対象があるときだけ呼ぶ。空フォルダのバッチが CI でも決定的に検証できる。

### 9.4 検証できなかったこと

- 実ユーザーのフォルダ（設計上、合成ツリーのみ）。
- 深さ・件数の上限を**実機 CLI** で深いツリーに対して確かめたのは「55件の上限」だけで、深さ3の打ち切りは単体テスト（B1）で固定している。
- Unicode 正規化差だけが異なる2つの提案が同時に発生する実例（適用時の再確認で上書きされないことは v097 の部分失敗テストで固定している）。
- モデル出力の質（同時に並べた項目の題名が互いに似すぎる等）。Phase 9 の評価対象。

## 10. リスクと対策

| リスク | 対策 |
|---|---|
| 大規模ツリーで時間が伸びる | 逐次 + 上限50 + 深さ3 + 非対応は着手前に除外。実測: 55件の非対応は 0.024 秒、実生成は1件あたり数秒〜十数秒 |
| 1件の失敗でバッチ全体が止まる | 1件を `Applied::Failed` として続行（§12）。実測: 計画後にソースを消した1件だけが失敗し、他は改名された |
| 同じ名前への2件が片方だけ適用される | 集合の衝突検出で両方保留。残る正規化差は適用時の再確認が上書きせず失敗させる |
| 親を先に改名して子の plan が無効になる | 深い順に適用。実測: 子→親の順で両方成功 |
| 非 TTY の実行が入力待ちでハングする | 端末があるときだけ質問する。実測: `/dev/null` からは質問なしで完走 |
| JSON が2文書になる | JSON では質問せず1文書。実測: バッチ JSON のキー集合を固定 |
| 一括 Undo がバッチ以外の行を戻す | `undo --last N` は新しい順に N 行を戻すだけ。状態は常に一貫し、余分な行も取り消し可能。バッチ id 列はスキーマ移行が要るため却下（D6） |
| モデルが無いのに長いバッチを始める | ウォーク後に対象があるときだけ1回確認し、無ければ何もせず理由付きで終了 |
| 上限に当たったことを黙る | `bounded:` 行と JSON の `truncated` で明示（実測 R5） |
