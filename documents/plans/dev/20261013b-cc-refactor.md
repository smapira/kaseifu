# CC リファクタリング計画 (BDD) — 2026-10-13

原典: ユーザ要求 (CC 16+ 3件・11-15 5件の改善、振る舞い不変、ゲート回避分割禁止、176テスト全緑)
根拠: `baseline.json` 実測 (236関数, max44, avg3.32)・`src/main.rs` / `inspect.rs` / `metadata.rs` 等を読んで分解。

## BDD タスク分解 (振る舞い単位)

- T1 `main` (CC44, src/main.rs:116) — 入力: 7サブコマンド (inspect/scan/organize/undo/discover/classify/watch) x rich/json x フラグ (all/verbose/execute/dry_run/last/paths)。期待: 同一stdout/stderr/exit、dry-run既定、衝突時REVIEW、watch非dirでbail、undoループ継続/中断条件不変。設計上有意味な分割: コマンド別ハンドラ (handle_inspect/scan/organize/undo/discover/classify/watch) に分離。mainはparse+dispatchのみ (目標CC<=8、各ハンドラ<=10)。
- T2 `inspect body_sections` (CC17) — 入力: show_all x verbose x signals空/非空 x entries 0/5/7 x evidence長/切詰 x parent有/無。期待: FILE先頭、Type/Size/Modified/Location、SEMANTIC SIGNALS (MISSING時`-`)、Nearby entries件数文言・`--all`文言、DETAILSはverboseのみ、ASCIIのみ・幅揃え。分割: file_head / signals_section / directory_section / details_section (文書化されたL1/L2/L3層に対応)。目標 各<=7、合成<=5。
- T3 `classify body_sections` (CC12) — 入力: 確率降順・安定ソート、RANKING_LIMIT 5、選択がtop5外、NONE含む、policy 3種+未知、verbose evidence。期待: 降順表示・値無変更・再正規化なし、選択常時表示 (`ranked #N`)、`incl. NONE`フッタ、MoveTo非表示、ASCII・幅揃え。分割: target_head / ranking_section (+selected_outside_top/ranking_footer) / decision_section / policy_section。目標 各<=7、合成<=8。
- T4 `normalize_metadata` (CC16) — 入力: kMDItemContentType / Title* / Kind* / Authors / 作成・変更日 / 無視キー x String/非String x 空term。期待: file_type/authors/created_date/semantic_signals (M:/K: prefix) が現行通り、未知キー無視。分割: MetadataKey enum+classify (キー分類) + push_prefixed_terms (Title/Kind重複統合) + 適用ヘルパ。目標 各<=6、合成<=8。
- T5 `normalize_xattr` (CC13) — 入力: source Xattr以外 / allowlist外 / 非String / 空・Malformed/invalid / agent有無 / URL。期待: FinderInfoのみ処理、未知黙殺、破損skip、downloader抽出、urls現状空。分割: apply_xattr_item (フィルタ管) + apply_finder_info (解析) + is_corrupted_finder_value。目標 各<=5、合成<=4。
- T6 `undo_last_move` (CC13) — 入力: NothingToUndo / SourceMissing / SourceNotAFile / DestNotDir / Collision / History失敗 / rename成功・失敗。期待: FS不変条件・journal順序 (verify→record pending→rename→mark) 不変。分割: validate_undo_preconditions (検証相) + commit_undo (記録・実行・確定相)。目標 各<=7、合成<=5。
- T7 `build_reasons` (CC11) — 入力: evidence件数 / bonus>0+age / filename>=1.0/>=0.5/未満 / ext>=1.0+拡張子有無 / terms重複・空・5件上限。期待: 文言・順序 (found→established→filename→extension→match x5) 不変、降順・再正規化なし。分割: age_reason / filename_reason / extension_reason / term_reasons (理由カテゴリ別)。目標 各<=5、合成<=6。
- T8 `row_to_entry` (CC12) — 入力: 11列 (NULL可: candidate_id/confidence/margin/error/undone_at)。期待: 順序・型・NULL対応不変。方針: 線形マッピングで分岐なし・CCは`?`計数による inflated のため**分割しない** (分割自体がゲート回避に該当)。残存理由として明記。目標: 現状維持+文書化。

## 受け入れ条件

- `bash scripts/quality_check.sh` OVERALL PASS (fmt/clippy/test/complexity/baseline-gate/report-tests)
- CC16+ =0 (8件中7件改善、row_to_entryは11-15残存を許容・理由明記)
- `cargo test` 全緑 (176件維持・低下なし)、`cargo clippy -- -D warnings` PASS
- 意図的変更に合わせ baseline.json 更新 (README手順: quality_check→complexity_report.py baseline)
- 表示仕様・JSON形式不変 (ASCII統一、降順・再正規化禁止・NONE可など)

## 実装順序

1. T4/T5 (normalize: 独立・テスト豊富) → 2. T7 (discovery) → 3. T6+T8 (storage/operations) → 4. T2/T3 (renderers) → 5. T1 (main) → 6. quality_check→baseline更新→最終検証

## 検証 (Reviewer/QA相当)

- 差分監査: 振る舞い分岐の欠落・文言変更・順序変更がないか
- 品質: fmt/clippy/test/complexity/baseline-gate/report-tests + CC前後比較 (最大・平均・帯別・上位10)
