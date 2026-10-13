# 既存テストとの対応表（実測・v1.1 §7-4 / §6 の検証）

原典 v1.1 §6 は「対応候補の整理であり、全ケースが既存テストで検証済みという意味ではありません」と留保している。本書はそれを**実測の事実**に置き換える。

- 実行: `cargo test --all-targets --all-features`（2026-10-13、HEAD `6819ac9`）
- 結果: **279 passed / 0 failed / 0 ignored**（lib 51 + 統合 228 + バイナリ別 0）
- テストはすべてライブラリ経路（`tests/*.rs` は `kaseifu::...` を呼ぶ）。**CLI プロセスを起てるテストは 0 件**（`assert_cmd` 依存なし）。

## 1. 3 値の定義

| 値 | 意味 |
|---|---|
| カバー済み | シナリオの主要な expected 行に対応するテストが存在し、緑である |
| 一部 | 機構・ライブラリ経路は緑だが、シナリオの expected の一部（CLI 経路、既定値、異常中断、実 FS E2E）が未検証 |
| 未カバー | 対応するテストが 1 件も無い |

## 2. テストバイナリ別インベントリ（実測件数）

| ファイル | 件数 | 主な対象 |
|---|---|---|
| `src/lib.rs`（unit） | 51 | destinations 規則 11 / scoring / search_terms / normalize / mock_jev / decision / presentation 部品 |
| `tests/phase4_jev_decision_test.rs` | 22 | 候補検証と confidence/margin 計算 |
| `tests/phase4_real_jev_adapter_test.rs` | 17 | HTTP アダプタ（スキーマ・エラー分類・プライバシー） |
| `tests/phase4_full_pipeline_test.rs` | 9 | Observe→...→decision の結合（No candidates / 未知 ID / 確率不正） |
| `tests/phase4_mock_jev_contract_test.rs` | 8 | MockJev の契約 |
| `tests/phase4_context_test.rs` | 2 | DecisionContext の写像 |
| `tests/phase5_execute_test.rs` | 29 | Policy / MovePlan / Journal / Execute / Undo |
| `tests/phase6_watch_test.rs` | 11 | debounce / inflight / stability / self / エラー分離 |
| `tests/v31_render_test.rs` | 11 | Rich 表示（classify / inspect） |
| `tests/v32_image_metadata_test.rs` | 38 | 画像メタデータとプライバシー |
| `tests/v33_config_auth_test.rs` | 33 | 設定優先順位 / Keychain / doctor |
| `tests/v042_discovery_target_scope_test.rs` | 8 | 画像クエリのスコープと上限 |
| `tests/v042c_destination_exclusion_test.rs` | 9 | 除外規則と集約 |
| `tests/v0401_discovery_fix_test.rs` | 5 | 検索語と Jev 送信マーカー |
| `tests/discovery_scoring_test.rs` | 7 | スコアリング |
| `tests/discovery_age_test.rs` | 4 | 年齢ボーナスと tie-break |
| `tests/presentation_test.rs` | 15 | コンポーネント表示・パス短縮・ASCII |
| `tests/candidate_test.rs` / `tests/context_test.rs` | 0 / 0 | 空のテストファイル（テスト関数なし） |

## 3. S01–S20 の対応表（関数名レベル）

| S | 判定 | 代表テスト（ファイル:行 / 関数名） | 未カバーな expected |
|---|---|---|---|
| S01 | 一部 | `tests/phase5_execute_test.rs:413-445 phase5_execute_move_moves_and_journals`、`:533-557 phase5_undo_restores_the_file_and_journals_the_undo` | 実 CLI（organize → undo）の通し、sha256 比較 |
| S02 | カバー済み | `tests/phase4_full_pipeline_test.rs:254 phase4_full_pipeline_unknown_candidate_rejected`、`:271 ..._arbitrary_path_rejected`、`tests/phase4_jev_decision_test.rs:56 unknown_candidate_d99_is_rejected`、`tests/phase4_real_jev_adapter_test.rs:253,270` | 実 CLI の exit 1 / stderr 表示 |
| S03 | カバー済み | `tests/phase5_execute_test.rs:77-128`（0.85/0.40、0.90/0.10、0.60/0.0、0.57/0.14、NONE/None）、`tests/v33_config_auth_test.rs:579 v33_user_thresholds_gate_classify_and_plan_together` | 0.849 / 0.950 の境界行は個別ケース無し、CLI 表示 |
| S04 | 一部 | `tests/phase5_execute_test.rs:249 phase5_plan_rejects_collision_and_preserves_files`、`:448 phase5_execute_move_refuses_collision_and_preserves_files` | CLI の '== REVIEW ==' 表示、sha256 |
| S05 | **未カバー** | なし（順序の要素のみ: `tests/phase5_execute_test.rs:270 phase5_history_records_before_the_mutation`、`:499 phase5_execute_move_permission_failure_preserves_the_source`） | 異常中断の全 8 点、pending 残留の検出 |
| S06 | 一部 | `tests/phase6_watch_test.rs:173,197,229,259,281,314` | 既定値（500ms/1s/10s）と実 FSEvents、CLI ログ |
| S07 | カバー済み | `tests/v32_image_metadata_test.rs:195,202,210,221,232,298,318,337,345,356,369,393` | 実ファイルでの E2E（インデックス状態依存） |
| S08 | 一部 | `tests/phase4_real_jev_adapter_test.rs:449,467,486,505,528,546,566,587`、`tests/phase4_full_pipeline_test.rs:290,309,327` | 503、空ボディ、実 15 秒タイムアウト、接続拒絶 |
| S09 | 一部 | `tests/phase5_execute_test.rs:395 phase5_dry_run_reports_without_touching_anything` | CLI のフラグ解釈（--dry-run / 既定）、watch の dry-run |
| S10 | カバー済み | `tests/phase5_execute_test.rs:499 phase5_execute_move_permission_failure_preserves_the_source` | root 実行時の挙動、CLI 表示 |
| S11 | カバー済み | `tests/phase5_execute_test.rs:448`（計画後の衝突）、`:483 phase5_execute_move_fails_when_destination_directory_is_gone` | 真の並行実行（スレッド/プロセス間競合） |
| S12 | カバー済み | `tests/phase5_execute_test.rs:569 phase5_undo_refuses_collision_and_preserves_everything` | CLI の 'undo failed: Collision' |
| S13 | 一部 | `tests/phase5_execute_test.rs:611 phase5_undo_is_not_repeatable`（単一 move のみ） | 複数 move 時の 2 回目、`undo --last N` のループ |
| S14 | 一部 | `tests/v042c_destination_exclusion_test.rs:108-357`（9 件）、`src/discovery/destinations.rs:180-370`（unit 11 件） | **`is_system_directory` の部分一致にテスト 0 件**、過剰除外の境界 |
| S15 | カバー済み | `tests/v32_image_metadata_test.rs:630,647,658,670`、`tests/v0401_discovery_fix_test.rs:387 v0401_jev_markers_respect_privacy_gating` | 実 API への送信講読 |
| S16 | 一部 | `tests/v33_config_auth_test.rs:118-236`（resolver / redaction）、`:415-472`（stores） | 実 Keychain の拒否・未登録の E2E、実 401 |
| S17 | 一部 | `src/normalize/tests/mod.rs test_japanese_filename_normalization` | パイプライン／CLI 全体の非 ASCII・空白（T79/T80 は手動未実施） |
| S18 | **未カバー** | なし | 大量ファイル・メモリ・上限の観測 |
| S19 | **未カバー** | なし | 線返し分類と閾値近傍の反転記録 |
| S20 | **未カバー** | なし（v0.4.2c 計画書の手動実測記録が前例） | 版間の観測セットと diff |

## 4. S21–S28 / C01–C12 の対応表

| ID | 判定 | 備考 |
|---|---|---|
| S21 | 未カバー | `scan` のテスト無し（CSV にも行が無い） |
| S22 | 未カバー | 対話ウィザード。loader の保存テストのみ（`tests/v33_config_auth_test.rs:301,327`） |
| S23 | 一部 | `tests/v33_config_auth_test.rs:415,431,443,462`（ストアの単体） |
| S24 | 一部 | `tests/v33_config_auth_test.rs:59-206,237-414`（resolver / loader / config set） |
| S25 | 一部 | `tests/v33_config_auth_test.rs:473,495,509`（doctor 3 件、ネットワークなし） |
| S26 | 一部 | `tests/v31_render_test.rs:261,285`（renderer）、`tests/v32_image_metadata_test.rs:831,851,879` |
| S27 | 一部 | `tests/v042_*`、`tests/v042c_*`、`tests/discovery_*`（ライブラリ経路） |
| S28 | 一部 | `tests/phase6_watch_test.rs` の 11 件（プロセス起動なし） |
| C01–C09、C11、C12 | 未カバー | CLI プロセスを起てるテストが存在しない |
| C10 | 未カバー | 空白入りパスの自動テストなし（手動 T80 も未実施） |
| C08 | 未カバー | 未実装（`completions` 無し） |

## 5. 原典 v1.1 §6 の検証結果

| 原典の記載 | 実測 | 判定 |
|---|---|---|
| `tests/phase5_execute_test.rs` = S01–S05、S09–S13 | 実際に S01/S04/S09–S13 をカバー。**S02・S03 はこのファイルに無い**（S02 は phase4 系、S03 は phase5 の policy テスト） | 補正（領域の割り当てを 1 列分離） |
| `tests/phase6_watch_test.rs` = S06、S18 | S06 は 11 件でカバー。**S18 の大量ファイルテストは存在しない** | 補正（S18 は未カバー） |
| `tests/v32_image_metadata_test.rs` = S07、S15 | 一致（38 件が該当） | 一致 |
| `tests/v33_config_auth_test.rs` = S16 | 一致（加えて S22–S25 の構成要素もカバー） | 一致（範囲拡大） |
| `tests/phase4_jev_decision_test.rs` = S02、S03、S08 | S02/S08 はカバーだが **S03（Policy）は含まない**（phase4 は decision までで Policy は Phase 5） | 補正 |
| `tests/user-test-plan.csv` = 全シナリオとの照合 | T01–T84 の手動チェックリスト。`Pass` 列は全行空（**未実施**）。`scan` の行が無く、MP4・`--format json` の行も無い | 未実施の手動リストとして扱う |

## 6. まとめ（未カバーの多い順）

1. **S05（異常中断）** — ファイル安全の中核だがテスト 0 件
2. **S18（大量ファイル） / S19（分類の非決定性） / S20（版間回帰）** — 観測設計のみ
3. **C01–C12 全体** — CLI プロセスレベルのテストが存在しない
4. **S13 の複数 move / S14 のシステムパス判定 / S17 の E2E** — 「一部」のうちリスクの高い 3 件
