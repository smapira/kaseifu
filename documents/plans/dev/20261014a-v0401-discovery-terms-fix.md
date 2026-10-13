# v0.4.1 パッチ修正 — v3.2 Discovery 汚染（可用性マーカーの検索語漏れ）BDD 計画書

原典: 親スレッドのユーザリクエスト（修正案A・仕様確定済み）。対象: worktree `kaseifu`、HEAD `2bbe310`（release v0.4.0）。

本書は BDD タスク分解と検証エビデンスの記録である。実装・レビュー・QA の成果物は `documents/plans/dev/issues/` に issue 形式で保存する。

## 1. バグの要約（再現済み）

v3.2 で `src/normalize/mod.rs` が `M:Capture date available` / `M:GPS available` / `M:Camera available` を `NormalizedFileContext.semantic_signals` へ追加した。この配列は 2 つの消費者を持つ:

1. **Discovery** — `extract_terms` が接頭辞を剥がして語化 → `normalize_terms`（34語フィルタ）→ 全語を空白結合した 1 本の mdfind クエリ（AND 意味論）
2. **Jev** — `DecisionContext.semantic_signals` として `state.semantic_signals` に載る

結果、可用性マーカーが検索語として消費され検索が痩せる（実測は §2）。

## 2. 修正前の実測（2026-10-14、実施済み・証跡あり）

| 計測 | コマンド | 結果 |
|---|---|---|
| 検索語（JPEG 実写真） | `KASEIFU_DEBUG=1 kaseifu discover ~/Desktop/images/IMG_3020.jpeg` | terms に `Capture date available` / `GPS available` / `Camera available` が混入。query = `IMG 3020 Capture date available image GPS available Camera available` |
| Spotlight raw 件数（同 query） | `mdfind -count '...'` | **3** |
| マーカー除去 query | `mdfind -count 'IMG 3020 image'` | **120** |
| マーカーのみ query | `mdfind -count 'Capture date available GPS available Camera available'` | 47 |
| 候補（JPEG） | 同 discover | 3 件（Zed copilot dist / ruri-v3 / ACE-Step）＝意味的無関係ノイズ |
| 検索語（PNG 実写真） | `discover ~/Desktop/images/IMG_3267.PNG` | query に `Capture date available` 混入、raw 50（上限）、候補 13 |
| マーカー除去 query（PNG） | `mdfind -count 'IMG 3267 image'` | 610 |
| 既存テスト | `cargo test --all-targets --all-features` | **247 passed / 0 failed** |

証跡ファイル: `/tmp/pre_fix_discover_3020.txt`, `/tmp/pre_fix_discover_3267.txt`, `/tmp/pre_fix_tests.txt`（本スレッドの作業マシン）。

## 3. 確定仕様（ユーザ指定・変更しない）

- **変更1** `src/normalize/mod.rs`: 可用性マーカーの signal 追加を削除（`semantic_signals` は内容 signal のみに戻す）
- **変更2** `src/classifier/jev.rs`（`from_context_with_privacy` / `filtered_signals` 周辺）: `image_metadata` の有無と privacy フラグ（`send_capture_time` / `send_gps`）から同一文言・同一順序（capture→GPS→camera）の 3 マーカーを生成し `state.semantic_signals` に合成。privacy 時の除去挙動は現行と同一
- **変更3** `src/normalize/image.rs::signals` はアダプタ側へ移設し、定義を二重に残さない

### 実装設計（Plan Architect レビュー対象）

- normalize 出所: `semantic_signals` = `[metadata M:/K:..., xattr X:..., F:terms...]`。マーカーは**アダプタで末尾に合成**（修正前の normalize が末尾に追記していた位置・順序と同一に保つ）
- アダプタ出所（`src/classifier/jev.rs`）:
  - 新 `image_availability_signals(image: Option<&ImageMetadata>, send_capture_time, send_gps) -> Vec<String>`
    - `send_capture_time && meta.has_capture_time()` → `M:Capture date available`
    - `send_gps && meta.has_location()` → `M:GPS available`
    - `meta.camera.model.is_some() || meta.camera.make.is_some()` → `M:Camera available`（写真は非 location 依存・privacy 対象外、現行同一）
  - 現 `filtered_signals` / `keep_signal` を置換: 受信 `semantic_signals` から 3 マーカー文字列（stale/legacy）を除去し、生成マーカーを末尾に合成。**マーカーの単一情報源は `image_metadata`**
- `state.target` のマッピング（`capture_date_for` / `camera_for` / `location_for` / `dimensions_for`）は不変
- 新規依存なし・API スキーマ（フィールド）不変・Rust 側スコアなし

#### マーカー合成の意味論（現行との一致点）

| privacy (capture, gps) | 修正前 wire（production） | 修正後 wire |
|---|---|---|
| (on, on) | content + `M:Capture date available` + `M:GPS available` + `M:Camera available` | 同一 |
| (on, off) | content + capture + camera | 同一 |
| (off, on) | content + GPS + camera | 同一 |
| (off, off) | content + camera | 同一 |

（production 経路では「マーカーが存在 ⟹ image_metadata が存在」だったため、生成方式でも集合・順序が一致する）

## 4. BDD タスク分解（振る舞い単位）

### T0 事前実測（済・§2）

### T1 normalize: マーカー追加の削除（変更1）

- 振る舞い: フルメタデータ画像の `normalize` → `semantic_signals` に `available` を含むマーカーが無い。`image_metadata` は従来どおり保持（validate 適用後）
- 正常系: Spotlight 8属性フル → signals = `[F:IMG, F:1234]`（内容のみ）、`image_metadata` は Some
- 異常系: 非画像・メタデータなし → 従来どおり signals 内容のみ / `image_metadata` None
- エッジ: 既存の `M:`/`K:`/`X:`/`F:` signal 生成・順序は不変

### T2 Jev アダプタ: マーカー生成と合成（変更2）

- 振る舞い: privacy 4 通り（capture on/off × gps on/off）で、production 形 `DecisionContext`（normalize 出力 + image_metadata）から送られる `state` が修正前 wire と完全一致
- 正常系: 4 通りの wire が v0.4.0 ゴールデンと一致（§3 表）
- 異常系: privacy off → 値もマーカーも trace を残さない（`M:GPS available` を出さない等）
- エッジ:
  - image_metadata None → マーカー 0 個（legacy shape 不変）
  - 受信 signals に stale マーカー文字列 → 二重送信しない（単一情報源）
  - camera のみの画像 → camera マーカー 1 個
  - `timezone` 無し capture（EXIF）→ `value` のみ送出（不変）

### T3 `normalize::image::signals` の削除（変更3）

- 振る舞い: `src/normalize/image.rs` に `signals` 定義が残らない（`validate` は残る）。参照ゼロ
- 副作用: `tests/v32_image_metadata_test.rs` の import と `signals(&valid)` 断言を、validate の責務（capture/location 保持）へ更新

### T4 回帰テスト（受入基準1）: メタデータが検索語を変えない

- 新規 `tests/v0401_discovery_fix_test.rs`
- 振る舞い: 「フルメタデータ画像」と「メタデータなしファイル」で `discover_with` が受ける mdfind クエリの検索語集合が同一（= `{IMG, 3020}`）で、`available` を含まない
- 正常系: 上記一致 + 語集合の明示 assert
- 異常系: なし（メタデータ有無の 2 ケース比較）
- エッジ: メタデータ側は `image_metadata.is_some()` をガード（vacuous pass 防止）。語順は既存 `HashSet` 仕様で非決定的のため**集合比較**とする
- fixture: EXIF 無し最小 JPEG（`FF D8 FF D9`）+ Spotlight 画像属性 evidence

### T5 wire ゴールデン（受入基準2）: privacy 4 通りの wire 同一性

- 新規テスト（同ファイル）: mock HTTP server で `state` JSON を捕捉し、**修正前実測で採取したゴールデン 4 種**と比較
- 構築: 実 fixture（HEIC 拡張子 + フル Spotlight evidence）→ `normalize_with_options` → `DecisionContext::from_normalized` → `RealJevIntegration` 4 通り
- 正常系: `state`（target の capture_date/camera/location/dimensions + semantic_signals）がゴールデンと一致
- エッジ: ゴールデンは本体実装コードの変更なしに採取する（同一テストコードを修正前後で実行して compare）

### T6 実機確認（受入基準3）: 実写真での回復

- 振る舞い: `discover ~/Desktop/images/IMG_3020.jpeg` の検索語に `available` が無く、候補が回復（実測は §2 と同条件で比較）。PNG（IMG_3267.PNG）も同様
- 正常系: JPEG/PNG 両方
- エッジ: 候補数の回復は「マーカー除去クエリ相当（120 件類似ファイル）からの集約」であることを実測で示す

### T7 品質ゲート（受入基準4）

- `bash scripts/quality_check.sh` OVERALL PASS（fmt / clippy -D warnings / test / complexity / baseline-gate / report-tests）
- 既存テスト全緑（修正前 247）＋新規テスト追加
- 新規高 CC 関数（16+）を作らない。ベースライン更新は不要（意図的変更なし）

### T8 ドキュメント（意図的な表示差分の記録含む）

- 記録: Rich `inspect` の SEMANTIC SIGNALS からマーカー 3 行が消える（CAPTURE METADATA / LOCATION パネルと重複情報・パネル側は不変）。加えて `classify --verbose` の stderr 診断（`== Semantic signals ==`）からも消える（DecisionContext は内容 signal のみになるため）。公開 JSON（inspect=FileContext / classify=decision）は不変
- `documents/jev-api.md`: `semantic_signals` のマッピング記述を「内容 signal verbatim + アダプタ生成の可用性マーカー」へ更新
- `CHANGELOG.md` `[Unreleased]` に Fixed 項目を追加

## 5. 不変条件（厳守・テストで担保）

1. Jev へ送る内容が修正前と完全一致: `state.target`（capture_date/camera/dimensions/location）と `state.semantic_signals` のマーカー 3 種（T5）
2. 公開 JSON 不変: `inspect --format json` = FileContext、`classify --format json` = decision（`semantic_signals` は公開 JSON に出ない）
3. Rust 側で撮影日時・GPS に独自スコアを付けない（v3.2 §1/§8）
4. 新規依存なし。ASCII 表示方針・NO_COLOR/非 TTY・TUI 禁止を維持

## 6. 変更ファイル

- Modify: `src/normalize/mod.rs`, `src/normalize/image.rs`, `src/classifier/jev.rs`, `tests/v32_image_metadata_test.rs`, `documents/jev-api.md`, `CHANGELOG.md`
- New: `tests/v0401_discovery_fix_test.rs`, `documents/plans/dev/20261014a-v0401-discovery-terms-fix.md`, `documents/plans/dev/issues/20261014-*.md`

## 7. ツール根拠（推測でタスクを切らない）

- `read_file: src/normalize/mod.rs:107-133` — マーカー追加箇所と `image_metadata` の流れを確認
- `read_file: src/discovery/search_terms.rs` / `src/discovery/similarity.rs:42` — `terms.join(" ")` の 1 本クエリと 34 語フィルタを確認
- `read_file: src/classifier/jev.rs:392-526` — `from_context_with_privacy` / `filtered_signals` / `keep_signal` / `*_for` マッピングを確認
- `read_file: src/models/decision.rs` — `DecisionContext.image_metadata` 保持と `skip_serializing` を確認
- `read_file: src/normalize/image.rs` — `signals()` 定義の唯一の参照を確認
- `grep: image::signals|Capture date available` — 参照は normalize/mod.rs と tests/v32 のみ
- `terminal: cargo build` → 修正前バイナリで `discover` 実測（§2）
- `terminal: mdfind -count` 3 種 — 検索語混入の因果を定量確認

## 8. 完了条件（ユーザ受入基準の対応）

1. T4 回帰テスト（メタデータが検索語を変えない）が追加され PASS
2. T5 mock server wire 比較（privacy 4 通り、修正前ゴールデン一致）が PASS
3. T6 実写真 2 種で `available` 不含・候補回復を実測
4. T7 `bash scripts/quality_check.sh` OVERALL PASS / 既存 247 維持 / 新規 CC16+ なし
- 付帯: T8 の意図的表示差分記録、git commit/push/tag は行わない（親が実施）

## 9. 完了記録（2026-10-14 実測）

### 実装

- 変更1 `src/normalize/mod.rs`（-6/+6）: マーカー追加を削除、理由コメント追加
- 変更2 `src/classifier/jev.rs`: `signals_for_jev` / `is_availability_marker` / `image_availability_signals` を新設（旧 `filtered_signals` / `keep_signal` を置換・stale マーカー除去 + 生成合成）
- 変更3 `src/normalize/image.rs`: `signals()` 削除（`grep` 残存参照 0）
- テスト: 新規 `tests/v0401_discovery_fix_test.rs`（5 件）+ v32 テストの責務更新
- ドキュメント: `documents/jev-api.md` マッピング更新、`CHANGELOG.md` [Unreleased] Fixed 追加
- 差分: 6 ファイル +72/-49（本体）+ 新規テスト/ドキュメント。新規依存なし

### 検索語・候補の実測比較（同一マシン・同一条件）

| 対象 | 修正前 | 修正後 |
|---|---|---|
| IMG_3020.jpeg 検索語 | `[IMG, 3020, Capture date available, image, GPS available, Camera available]` | `[IMG, image, 3020]`（語順は HashSet 仕様で非決定的） |
| IMG_3020.jpeg mdfind raw | 3 | 50（アプリ上限。同一語集合の索引ヒットは 120） |
| IMG_3020.jpeg 候補 | 3（全て意味的無関係） | 34（複数回安定、初回 36） |
| IMG_3267.PNG 検索語 | `[3267, Capture date available, IMG, image]` | `[image, 3267, IMG]` |
| IMG_3267.PNG query 索引ヒット | 552 | 610 |
| IMG_3267.PNG 候補 | 13 | 12（双方 raw 50 上限のため同水準） |

### wire 同一性の検証方法（受入基準2）

1. 修正前ビルドで、本番形状 context（実 fixture + `normalize_with_options` → `DecisionContext::from_normalized`）を mock server に送り、privacy 4 通りの `state` JSON を**ゴールデンとして採取**（テストコードは修正前後で不変）
2. 修正後、同一テスト（`v0401_jev_wire_state_matches_v0400_goldens`）がゴールデンと完全一致することを検証
3. 4 通りの `state`（capture_date/camera/location/dimensions + semantic_signals）の期待値:
   - (on, on): `[F:IMG, F:1234, M:Capture date available, M:GPS available, M:Camera available]` + location あり
   - (on, off): `[F:IMG, F:1234, M:Capture date available, M:Camera available]`
   - (off, on): `[F:IMG, F:1234, M:GPS available, M:Camera available]` + location あり
   - (off, off): `[F:IMG, F:1234, M:Camera available]`
4. 端到端補強: ローカル mock に `classify` CLI（実写真）を接続し、wire に内容 signal + マーカー（capture/camera、GPS は既定 off）が載ることを実測

### テスト・品質ゲート

- 新規テスト Red→Green: 修正前に 3 件 RED（discovery 語集合 / normalize マーカー不含 / metadata 単一情報源）、ゴールデン 2 件は非回帰ガードとして GREEN
- `cargo test --all-targets --all-features`: **252 passed / 0 failed**（修正前 247 + 5）
- `bash scripts/quality_check.sh`: **OVERALL PASS**（fmt/clippy/test/complexity/baseline-gate/report-tests/coverage）。CC16+ = 0、新規最大 CC 7
- ベースライン更新なし（意図的な複雑度変更なし）

### 意図的な表示差分（許容・記録）

- Rich `inspect` SEMANTIC SIGNALS から `K`/`F` のみに（マーカー 3 行消滅）。CAPTURE METADATA / LOCATION パネルは不変（実機確認）
- `classify --verbose` の stderr 診断からもマーカー行が消える（公開 JSON ではない）
- 公開 JSON（inspect=FileContext / classify=decision）は不変

### 成果物

- `documents/plans/dev/issues/20261014-plan-architect-v0401-review.md`
- `documents/plans/dev/issues/20261014-reviewer-v0401.md`
- `documents/plans/dev/issues/20261014-qa-v0401.md`

