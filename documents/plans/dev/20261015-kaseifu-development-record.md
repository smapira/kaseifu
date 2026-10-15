# kaseifu development chronological record

Generated: 2026-10-15 JST.

Purpose: source record for a development blog. This file records dates, work items, commits, design documents, completed issues, verification notes, release tags, and known model/agent information. It does not interpret the work.

## Known model / agent records

| Date/time (JST) | Source | Model / agent | Scope |
|---|---|---|---|
| 2026-10-15 | Delta thread system metadata | GPT-5.5 | Parent agent for implementation, verification, commit, release, and this record. |
| 2026-10-15 | Delta subagent acceptance | `product-manager` profile using `opencode-go/go/muse-spark-1.3-contributor` | Coordinated BDD implementation for remaining M1-M4 backlog. |
| 2026-10-15 | Delta subagent failure | `reviewer` profile using `opencode/zen/muse-spark-1.3-contributor-free` | Overall review request failed because the account could not use the model. |
| 2026-10-15 | Source/docs | Not recorded | Commit authorship and design documents generally do not record model identity. |

## Release and verification checkpoints

| Date/time (JST) | Record | Evidence |
|---|---|---|
| 2026-10-12T18:40:48+09:00 | `30dff8a` docs: finalize 0.2.0 changelog for release | tag: v0.2.0, feat/v3.1-ascii-chrome |
| 2026-10-13T16:43:25+09:00 | `303df2d` chore: release v0.3.0 | tag: v0.3.0 |
| 2026-10-13T17:09:31+09:00 | `2bbe310` chore: release v0.4.0 | tag: v0.4.0 |
| 2026-10-13T19:35:17+09:00 | `0e65c58` fix: keep image availability markers out of discovery search terms | tag: v0.4.1 |
| 2026-10-13T21:11:52+09:00 | `6819ac9` chore: parse .env with dotenvy; release v0.4.2 | tag: v0.4.2 |
| 2026-10-13T23:31:04+09:00 | `5511c72` chore: release v0.5.0 | tag: v0.5.0 |
| 2026-10-14T02:01:23+09:00 | `3134876` chore: release v0.6.0 | tag: v0.6.0 |
| 2026-10-14T03:14:14+09:00 | `d436451` chore: release v0.7.0 | tag: v0.7.0 |
| 2026-10-14T04:55:50+09:00 | `9be5f71` chore: release v0.8.0 | tag: v0.8.0 |
| 2026-10-14T21:39:17+09:00 | `440719c` chore: release v0.9.0 | tag: v0.9.0 |
| 2026-10-15T17:02:05+09:00 | `224a3e7` chore: release v0.10.0 | HEAD -> main, tag: v0.10.0, origin/main, origin/HEAD |
| 2026-10-15 | v0.10.0 release verification | `cargo test --all-targets --all-features` = 873 passed / 0 failed; `bash scripts/quality_check.sh` = OVERALL PASS. |
| 2026-10-15 17:02-17:03 | User installed v0.10.0 with `cargo install --path .` | Replaced `~/.cargo/bin/kaseifu` and `kaseifu-bench` from v0.9.0 to v0.10.0. |
| 2026-10-15 17:08+ | User retried `cargo install --path . --locked` | Lock-resolved install started; Cargo warned `yoke-derive v0.8.3` in `Cargo.lock` is yanked; no source change recorded. |

## Git commit chronology

| Date/time (JST) | Commit | Record | Refs |
|---|---|---|---|
| 2026-09-22T19:02:27+09:00 | `bae83a9` | feat: initialize semantic file organizer |  |
| 2026-09-22T19:13:37+09:00 | `4e38dd0` | docs: add design md |  |
| 2026-09-22T19:52:07+09:00 | `7a499bf` | feat: complete phase 1 file observation |  |
| 2026-09-23T06:00:54+09:00 | `8b274ae` | feat: complete Phase 2 Normalize with tests * implement allowlist approach for xattr normalization   - only process com.apple.FinderInfo (safe binary attrs like     com.apple.macl are silently skipped) * normalize filename: remove noise words + unfamiliar terms * normalize metadata: filter snapshot date (recents,library,screenshot) * add 14 unit tests (all passing) |  |
| 2026-09-23T21:44:24+09:00 | `ce6459c` | feat: complete Phase 3 Discovery pipeline |  |
| 2026-09-24T02:07:38+09:00 | `52055d1` | feat: implement Phase 4 Jev decision validation with TDD |  |
| 2026-09-25T17:12:55+09:00 | `2dd5299` | feat: complete Phase 4 Jev integration contract and production mock |  |
| 2026-09-25T20:52:26+09:00 | `ff51a91` | fix: stabilize Phase 4 Jev mock contract |  |
| 2026-09-25T21:35:07+09:00 | `c9eca32` | feat: protect Phase 4 full decision pipeline with integration tests |  |
| 2026-09-25T21:44:30+09:00 | `deddba1` | fix: harden Phase 4 decision domain and replace hollow tests |  |
| 2026-09-25T21:57:36+09:00 | `b4ea919` | feat: add Phase 4D real Jev HTTP adapter |  |
| 2026-09-25T22:07:58+09:00 | `2140019` | feat: wire DecisionContext from normalized context and discovery results |  |
| 2026-09-25T22:27:13+09:00 | `8be43f3` | feat: complete Phase 4 live Jev decision pipeline |  |
| 2026-09-25T22:43:05+09:00 | `ae73350` | feat: add Phase 5 safe execution with journal and undo |  |
| 2026-09-25T22:55:49+09:00 | `1a37b83` | feat: automate pipeline from FSEvents with debounce, dedupe and self-event suppression |  |
| 2026-09-29T04:17:26+09:00 | `b275d34` | feat: rename to kaseifu and improve discover pipeline |  |
| 2026-09-29T04:19:08+09:00 | `5ab4a83` | docs: add CHANGELOG.md for v0.1.0 |  |
| 2026-09-29T04:30:40+09:00 | `602abe2` | docs: add user test plan checklist (84 cases) |  |
| 2026-10-12T16:28:34+09:00 | `be915eb` | feat: Discovery scoring v2 and Rich CLI dashboard | origin/feat/v2.0-discovery-rich-cli, feat/v2.0-discovery-rich-cli |
| 2026-10-12T16:36:25+09:00 | `d3629c3` | feat: align discover panel, relative paths with emphasized last segment |  |
| 2026-10-12T16:40:22+09:00 | `a9b7482` | feat: show full relative paths using real terminal width | origin/feat/v2.0-discover-layout, feat/v2.0-discover-layout |
| 2026-10-12T17:35:37+09:00 | `30a8ca7` | feat: directory age adjustment with older-first tie-break | origin/feat/v2.0-date-adjustment, feat/v2.0-date-adjustment |
| 2026-10-12T18:07:31+09:00 | `6910fa0` | feat: v3.1 Classify and Inspect Rich output | origin/feat/v3.1-classify-inspect, feat/v3.1-classify-inspect |
| 2026-10-12T18:31:23+09:00 | `e8f860a` | feat: ASCII-only dashboard chrome for CJK-font terminals | origin/feat/v3.1-ascii-chrome |
| 2026-10-12T18:40:48+09:00 | `30dff8a` | docs: finalize 0.2.0 changelog for release | tag: v0.2.0, feat/v3.1-ascii-chrome |
| 2026-10-13T01:31:53+09:00 | `37493d3` | docs: move docs to documents and add user plans |  |
| 2026-10-13T03:21:29+09:00 | `29e649a` | docs: add README hero artwork with credit |  |
| 2026-10-13T03:23:38+09:00 | `0dda4ac` | chore: untrack local tooling dirs |  |
| 2026-10-13T03:33:58+09:00 | `cc512bd` | chore: untrack opencode.json |  |
| 2026-10-13T15:29:39+09:00 | `c9278a0` | feat: v3.4 quality gates (fmt/clippy/complexity/baseline/CI) |  |
| 2026-10-13T15:52:45+09:00 | `3a97787` | refactor: reduce high-CC functions (max 44->12, CC16+ 3->0) |  |
| 2026-10-13T16:38:29+09:00 | `96363f3` | feat: v3.3 user config and Keychain auth (setup/auth/config/doctor) |  |
| 2026-10-13T16:43:25+09:00 | `303df2d` | chore: release v0.3.0 | tag: v0.3.0 |
| 2026-10-13T17:06:39+09:00 | `7bd93df` | feat: v3.2 image metadata and context-aware classification |  |
| 2026-10-13T17:09:31+09:00 | `2bbe310` | chore: release v0.4.0 | tag: v0.4.0 |
| 2026-10-13T19:35:17+09:00 | `0e65c58` | fix: keep image availability markers out of discovery search terms | tag: v0.4.1 |
| 2026-10-13T19:49:54+09:00 | `ac494e7` | chore: hide the debug-only discover command from help and README |  |
| 2026-10-13T21:11:52+09:00 | `bf033de` | fix: scope image discovery and refuse unusable destinations |  |
| 2026-10-13T21:11:52+09:00 | `6819ac9` | chore: parse .env with dotenvy; release v0.4.2 | tag: v0.4.2 |
| 2026-10-13T23:31:04+09:00 | `792b8d9` | docs: add debug scenario design deliverables (S01-S28, C01-C12) |  |
| 2026-10-13T23:31:04+09:00 | `3d2374b` | feat: zsh completions, organize output polish, and Keychain/.env key sources |  |
| 2026-10-13T23:31:04+09:00 | `5511c72` | chore: release v0.5.0 | tag: v0.5.0 |
| 2026-10-14T00:41:46+09:00 | `ccb229e` | fix: detect interrupted moves, stabilize the journal path, and make settings honest |  |
| 2026-10-14T00:41:46+09:00 | `7d8f775` | docs: add the zero-config improvement plan |  |
| 2026-10-14T01:36:49+09:00 | `5c22455` | fix: send interactive prompts to stderr so stdout stays parseable |  |
| 2026-10-14T01:37:03+09:00 | `21e3499` | feat: zero-config first run (key prompt, bare-command guidance, doctor verdict) |  |
| 2026-10-14T02:01:23+09:00 | `3134876` | chore: release v0.6.0 | tag: v0.6.0 |
| 2026-10-14T03:13:47+09:00 | `46520ec` | feat: zero-config P1 (threshold transparency, organize JSON, --select, help rewrite) |  |
| 2026-10-14T03:14:14+09:00 | `d436451` | chore: release v0.7.0 | tag: v0.7.0 |
| 2026-10-14T04:55:34+09:00 | `278279e` | feat: zero-config finish-up (uninstall, watch vocabulary, README quick start) |  |
| 2026-10-14T04:55:50+09:00 | `9be5f71` | chore: release v0.8.0 | tag: v0.8.0 |
| 2026-10-14T12:36:55+09:00 | `a5eed65` | feat: TREC derived-D2D benchmark harness (kaseifu-bench) |  |
| 2026-10-14T12:37:13+09:00 | `f2150a5` | feat: Foundation Models FFI bridge and semantic provider (AI rename phases 0-2) |  |
| 2026-10-14T12:37:18+09:00 | `f20e909` | test: debug scenarios W0+W1 with plan and review records |  |
| 2026-10-14T13:40:31+09:00 | `ca98e19` | feat: AI rename naming rules and document rename (phases 3-4) |  |
| 2026-10-14T13:51:10+09:00 | `9162217` | fix: rename hardening against model-output artifacts and prompt injection |  |
| 2026-10-14T15:49:49+09:00 | `0cff3c0` | fix: bound rename stems to 80 characters at a word boundary |  |
| 2026-10-14T16:04:31+09:00 | `c1a8d99` | fix: use underscores instead of spaces in rename stems |  |
| 2026-10-14T20:23:25+09:00 | `6d668b4` | fix: treat only real capture signals as image capture time |  |
| 2026-10-14T20:24:13+09:00 | `b9b7e68` | feat: AI rename phase 5 image rename (text-equivalent path) |  |
| 2026-10-14T21:17:29+09:00 | `085042f` | feat: add Apple Vision perception to image rename (phase 5b) |  |
| 2026-10-14T21:39:17+09:00 | `440719c` | chore: release v0.9.0 | tag: v0.9.0 |
| 2026-10-15T00:02:40+09:00 | `4d94c40` | feat: AI rename phase 6 video rename through AVFoundation and Vision |  |
| 2026-10-15T01:51:40+09:00 | `e6104eb` | feat: AI rename phase 7 folder rename from child contents |  |
| 2026-10-15T04:19:25+09:00 | `ad5b0a4` | feat: AI rename phase 8 batch rename over a folder |  |
| 2026-10-15T04:51:06+09:00 | `f519188` | feat: measure the phase 9 numbers with kaseifu-bench perf |  |
| 2026-10-15T05:01:31+09:00 | `8e64bf8` | docs: record the remaining open tasks as one issue file each |  |
| 2026-10-15T05:20:03+09:00 | `f96a4a9` | docs: record the first real-media session in the open issues |  |
| 2026-10-15T05:35:21+09:00 | `c863262` | fix: retry a timed-out generation once and stop leaking internal words |  |
| 2026-10-15T05:36:17+09:00 | `27a818e` | docs: transcribe the measured output of the timeout fix |  |
| 2026-10-15T16:55:54+09:00 | `f74c41f` | feat: implement the remaining backlog (M1 JSON, M2 media, M3 workflow, M4 perf) |  |
| 2026-10-15T16:55:58+09:00 | `11df921` | docs: close out the M1-M4 backlog issues and milestones |  |
| 2026-10-15T17:02:05+09:00 | `224a3e7` | chore: release v0.10.0 | HEAD -> main, tag: v0.10.0, origin/main, origin/HEAD |

## Development plan records

| File | Title |
|---|---|
| `documents/plans/dev/20261013a-v34-quality.md` | v3.4 実装計画書（dev） |
| `documents/plans/dev/20261013b-cc-refactor.md` | CC リファクタリング計画 (BDD) — 2026-10-13 |
| `documents/plans/dev/20261013c-v33-config-auth.md` | v3.3 implementation plan (dev) - user-facing config and auth |
| `documents/plans/dev/20261013d-organize-output-plan.md` | organize 出力改善 4件 BDD 計画書 |
| `documents/plans/dev/20261013e-key-source-plan.md` | APIキー取得経路の明示分離（KASEIFU_KEY_SOURCE）実装計画 |
| `documents/plans/dev/20261014a-v0401-discovery-terms-fix.md` | v0.4.1 パッチ修正 — v3.2 Discovery 汚染（可用性マーカーの検索語漏れ）BDD 計画書 |
| `documents/plans/dev/20261014b-v042-discovery-photo-scope.md` | v0.4.2 Discovery 候補品質改善（対象種別スコープ + 集約後切り捨て）BDD 計画書 |
| `documents/plans/dev/20261014c-v042c-discovery-destination-policy.md` | v0.4.2c Discovery 候補「移動先になれないディレクトリ」排除（規則ベース）BDD 計画書 |
| `documents/plans/dev/20261014d-cli-coverage-map.md` | CLI カバレッジ表（コマンド×シナリオ、サブコマンド・オプション） |
| `documents/plans/dev/20261014d-cli-scenarios-C01-C12.md` | CLI 横断シナリオ仕様 C01–C12（v1.2 §11 の完全化） |
| `documents/plans/dev/20261014d-cli-scenarios-S21-S28.md` | CLI コマンド別シナリオ仕様 S21–S28（v1.2 §3–§10 の完全化） |
| `documents/plans/dev/20261014d-debug-scenario-design.md` | kaseifu デバッグシナリオ設計 成果物 v2（S01–S28 / C01–C12 統合） |
| `documents/plans/dev/20261014d-existing-test-coverage.md` | 既存テストとの対応表（実測・v1.1 §7-4 / §6 の検証） |
| `documents/plans/dev/20261014d-scenarios-S01-S10.md` | 検証シナリオ仕様 S01–S10（v1.1 §7-2/3 の完全化） |
| `documents/plans/dev/20261014d-scenarios-S11-S20.md` | 検証シナリオ仕様 S11–S20（v1.1 §4 の一覧から完全仕様化） |
| `documents/plans/dev/20261014e-zero-config-p0.md` | ゼロコンフィグ P0（ZC-1 / ZC-2 / ZC-3 仕上げ / ZC-4）実装計画 |
| `documents/plans/dev/20261014f-ai-rename-p0.md` | Apple Foundation Models 統合（AI リネーム基盤）Phase 0-2 実装計画 |
| `documents/plans/dev/20261014g-trec-benchmark-d2d.md` | kaseifu TREC Benchmark（D2D 関連ファイル選択の Hit@1 評価基盤）実装計画 |
| `documents/plans/dev/20261014h-debug-scenarios-w0w1.md` | Debug scenarios Wave W0+W1 plan (20261014h) |
| `documents/plans/dev/20261014i-ai-rename-p34.md` | AIリネーム Phase 3（Naming Rule Engine）＋ Phase 4（Document Rename）実装計画 |
| `documents/plans/dev/20261014j-ai-rename-p5.md` | AIリネーム Phase 5（Image Rename）実装計画 |
| `documents/plans/dev/20261014k-ai-rename-p5b-vision.md` | AIリネーム Phase 5b（Vision 知覚層の追加）実装計画 |
| `documents/plans/dev/20261014l-ai-rename-p6.md` | AIリネーム Phase 6（Video Rename）実装計画 |
| `documents/plans/dev/20261015a-completions-zsh-plan.md` | kaseifu zsh 補完の実装と配布（v0.4.3 予定 / BDD 計画） |
| `documents/plans/dev/20261015b-journal-safety-config-honesty.md` | ジャーナル安全性と設定の正直さ（R1/R2/R5/R8/F-05/F-12）実装計画 |
| `documents/plans/dev/20261015c-ai-rename-p7.md` | AIリネーム Phase 7（Directory Rename）実装計画 |
| `documents/plans/dev/20261015d-ai-rename-p8.md` | AIリネーム Phase 8（Batch Rename）実装計画 |
| `documents/plans/dev/20261015e-ai-rename-p9-perf.md` | AIリネーム Phase 9（Performance / Quality）実装計画 |
| `documents/plans/dev/20261015j-backlog-bdd-plan.md` | Backlog BDD implementation plan (M1-M4, 8 issues) |
| `documents/plans/dev/20261016a-zero-config-p1.md` | ゼロコンフィグ P1（ZC-5 / ZC-6 / ZC-7 / ZC-8 / ZC-12 / ZC-13）実装計画 |
| `documents/plans/dev/20261016b-zero-config-finish.md` | ゼロコンフィグ 仕上げバッチ（ZC-11 / ZC-9 / ZC-10）実装計画 |
| `documents/plans/dev/v3.1-classify-inspect-plan.md` | v3.1 Classify/Inspect Rich Output - implementation plan |
| `documents/plans/dev/v3.2-image-metadata-plan.md` | v3.2 Image Metadata — BDD Plan (Phase 1-7, Phase 8 excluded) |

## Completed issue records

| File | Title |
|---|---|
| `documents/plans/dev/issues/done/20261015-done-batch-undo-scope.md` | 残タスク: 一括 Undo がバッチ単位で固定されない（`undo --last N`） |
| `documents/plans/dev/issues/done/20261015-done-cache-name-refusal.md` | 残タスク: 利用者のフォルダ名 `Cache` / `Caches` が改名できない |
| `documents/plans/dev/issues/done/20261015-done-debug-scenarios-w2-w5.md` | 残タスク: デバッグシナリオ W2〜W5 の実装 |
| `documents/plans/dev/issues/done/20261015-done-generation-timeout.md` | 完了: 生成のタイムアウトを 1 回再試行し、文言を利用者向けにした |
| `documents/plans/dev/issues/done/20261015-done-json-for-undo-scan-watch.md` | 残タスク: `undo` / `scan` / `watch` の `--format json` |
| `documents/plans/dev/issues/done/20261015-done-model-output-degeneracy.md` | 残タスク: モデル出力の退化（ファイル名の音写・型名の出力）への一般防御 |
| `documents/plans/dev/issues/done/20261015-done-noise-name-overmatch.md` | 残タスク: `is_noise_file` の部分一致による過剰除外 |
| `documents/plans/dev/issues/done/20261015-done-ocr-image-text.md` | 残タスク: 画像内文字の認識（OCR）を `text` 配列として追加 |
| `documents/plans/dev/issues/done/20261015-done-pdf-child-excerpt-quality.md` | 残タスク: フォルダ命名の品質（子が PDF のとき） |
| `documents/plans/dev/issues/done/20261015-done-perf-measurement-gaps.md` | 残タスク: Phase 9 の測定の残り（コールドスタート・大規模計画・セッション効果） |
| `documents/plans/dev/issues/done/20261015-done-release-tagging.md` | 残タスク: リリースタグ（Phase 6〜9 が未タグ） |
| `documents/plans/dev/issues/done/20261015-done-rename-to-directory.md` | 残タスク: ZC-14 `--to <dir>`（明示した移動先を指定する） |
| `documents/plans/dev/issues/done/20261015-done-unicode-normalization-conflicts.md` | 残タスク: 一括の衝突検出が Unicode 正規化差を畳まない |

## Waiting-user / external records after v0.10.0

| File | Title |
|---|---|
| `documents/plans/dev/issues/waiting-user/20261015-waiting-local-checkout-pull.md` | 残タスク: 本家チェックアウトの更新（`~/OS/home/Codes/github.com/smapira/kaseifu`） |
| `documents/plans/dev/issues/waiting-user/20261015-waiting-real-media-quality.md` | 残タスク: 実写真・実動画での命名品質評価 |
| `documents/plans/dev/issues/waiting-user/20261015-waiting-snapshot-cleanup.md` | 残タスク: スナップショットの削除判断 |
| `documents/plans/dev/issues/waiting-user/20261015-waiting-trec-corpus-evaluation.md` | 残タスク: TREC 本評価（コーパス入手が前提） |

## v0.10.0 user smoke record

| Date/time (JST) | Command / item | Record |
|---|---|---|
| 2026-10-15 17:03 | `cargo install --path .` | Installed `kaseifu v0.10.0`; replaced `kaseifu` and `kaseifu-bench` under `~/.cargo/bin`. |
| 2026-10-15 17:04 | `kaseifu classify 2026-07-02_木製通路を歩く大人.MP4` | 0 destinations found; decision `NONE`; policy `LEAVE`. |
| 2026-10-15 17:04 | `kaseifu discover 2026-07-02_木製通路を歩く大人.MP4` | No candidates found. |
| 2026-10-15 17:04 | `kaseifu classify IMG_2741.jpeg` | 50 destinations found; top `./bk` 94%; policy `AUTO MOVE`. |
| 2026-10-15 17:04 | `kaseifu organize IMG_2741.jpeg` | Dry run; refused because `./bk/IMG_2741.jpeg` already exists. |
| 2026-10-15 17:04 | `kaseifu organize bk/IMG_2741.jpeg` | Dry run; refused because `./IMG_2741.jpeg` already exists. |
| 2026-10-15 17:05 | `kaseifu organize bk/vape_ad_20260913_v2_final.MP4` | Dry run; candidate `~/Desktop/stash/movie`; confidence 0.590, margin 0.330; policy `LEAVE`. |
| 2026-10-15 17:08 | `cargo install --path . --locked` | Cargo warned that `yoke-derive v0.8.3` in `Cargo.lock` is yanked; build output shown through `icu_collections v2.3.0`. |

## Notes on source certainty

- Commit date/time and subject rows come from `git log --date=iso-strict --reverse --all` in the `kaseifu` worktree.
- Plan and issue rows come from markdown files under `documents/plans/dev/` and `documents/plans/dev/issues/` at `v0.10.0` plus the current record-file addition.
- Model / agent rows are included only where the Delta transcript exposed the model or agent profile. Other rows are marked not recorded.
