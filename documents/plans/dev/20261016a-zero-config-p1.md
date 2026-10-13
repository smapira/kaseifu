# ゼロコンフィグ P1（ZC-5 / ZC-6 / ZC-7 / ZC-8 / ZC-12 / ZC-13）実装計画

- 起票: 2026-10-16
- 原典: `documents/plans/user/20261013d.md`（ゼロコンフィグ化 改善計画 v1.0、§5・§8・§9、§11 実装順序、§12 完了条件）
- 前提: P0（ZC-1 / ZC-2 / ZC-3 仕上げ / ZC-4）は v0.6.0（`3134876`）として着地済み。
  参考: `documents/plans/dev/20261014e-zero-config-p0.md`、`tests/v052_zero_config_cli_test.rs`、`tests/v052_zero_config_key_test.rs`。**P0 の挙動を壊さない**
- 対象: `src/main.rs`, `src/doctor.rs`（ZC-8 で変更しない判断は §3.6）, `src/presentation/renderers/{generic,classify,organize}.rs`,
  新規 `src/presentation/renderers/explain.rs`, `src/presentation/renderers/select.rs`,
  新規 `src/operations/select.rs`, `src/operations/{plan,mover,mod}.rs`, `src/classifier/pipeline.rs`,
  `README.md`, `CHANGELOG.md`, 新規テスト 4 ファイル
- 対象外（実装しない）: ZC-9 / ZC-10 / ZC-11（P2・アンインストール）、将来の `--to <dir>`（ZC-14 予約）。
  P1 のスコープは ZC-5 → ZC-6 → ZC-7 → ZC-8 → ZC-12 → ZC-13 の直列のみ
- 版数接頭辞: 新規テストは **`v070_*`**（本バッチはヘルプ見直しと対話選択を含むユーザー向け機能追加であり、
  次のマイナー（0.7.0）として着地する想定。既存 v051/v052 は 0.6.0 に同梱された前例に倣う）
- 制約: 原典 §10 の非機能要件（秘密情報・ドライラン既定・ASCII のみ・TUI 禁止・NO_COLOR/非TTY 準拠・
  既存 JSON キーを削除しない・`discover` はデバッグ専用・`KASEIFU_KEY_SOURCE` の意味不変・新規依存なし・
  新規 CC16+ なし・実ユーザーの `~/Library/Application Support/kaseifu/`・`~/Desktop/`・旧 `./data/`・
  実 Keychain をテストで読み書きしない）。commit / push / tag / `git add` は親が実施
- ベースライン実測（2026-10-13、worktree `kaseifu`、HEAD `3134876`、clean）:
  `cargo test --all-targets --all-features` = **387 passed / 0 failed**（P0 計画書の 385 に加え親バッチの
  コミット分を含む）。`cargo fmt --all -- --check` = 0 violations
- 確認手段: serena / context7 / codebase-memory-mcp / graphify は本環境で未接続のため、LSP 相当の直接読解と
  `grep`（`src/main.rs` / `src/presentation/**` / `src/operations/**` / `src/classifier/{pipeline,jev}.rs` /
  `src/discovery/**` / `src/doctor.rs` / `tests/**` / `scripts/quality_check.sh`）で実コードを確認した

## 1. 背景と現状実測（変更前）

| ID | 現状（実測・コード根拠） | 影響 |
|---|---|---|
| ZC-5 | `generic::policy_view`（`src/presentation/renderers/generic.rs:47-68`）は閾値の**定数**（0.85 / 0.40 / 0.60）だけを文言に使い、実効閾値（user TOML 上書き）を表示しない。AUTO MOVE 直下（例: 0.840）の近接表示は存在せず、`--explain` フラグも無い（`grep -- '--explain' src/` = 0 件）。classify の非 Rich 出力は `decision` の JSON そのもの（`src/main.rs:605`） | 原典 §1 問題 B（0.85 → 0.840 の反転）で「なぜ REVIEW なのか」が分からない |
| ZC-6 | `--help` のトップレベル（`src/main.rs:32-46`）に policy 3 値の説明・閾値・`--execute` 必須の記載は無い。`kaseifu policy -h` は未定義サブコマンドのエラーになる | 概念が見えないため `policy` というコマンドを探してしまう（原典 §1 補足） |
| ZC-7 | `handle_organize`（`src/main.rs:417-425`）は Rich 以外で `render_text` を出すだけ。`organize --format json` は JSON にならず、decision / policy / outcome を取り出す手段が無い（原典 §8 F-07 の「未対応フラグの提示」と同一） | 自動化・検証がテキストパース頼み |
| ZC-8 | README に対象拡張子（`is_image_file`: jpg/jpeg/heic/heif/hif/tif/tiff/png/webp、`src/context/image_metadata.rs:52-60`）と動画（MP4/MOV）非対応の記載が無い。`--help` にも無い | 「動画を入れたのに何も起きない」ずれ |
| ZC-12 | `--help` の全出力に内部情報が残る: `classify` = "(Phase 4)" + "Observe -> Normalize -> Discover -> Jev -> validated JevDecision"（`src/main.rs:101-104`）、`organize` = "decision -> policy -> plan -> (dry-run or move)"（:64）、`watch` = "(Phase 6)"（:115）、`inspect` = "Build and display FileContext JSON"（:50）、`--verbose` = "Show the DETAILS layer (full paths, schema, raw metadata)"（:56-57）、`scan` = "Scan a directory"（:61、実態はバナー出力のみ :379-390）、`--format json` が undo/scan/watch で機能しないのに `[possible values: auto, rich, json]` を表示（グローバル引数 :35-36） | ユーザーの操作に無関係な内部語がヘルプの主役になっている |
| ZC-13 | `organize` の引数は `file` / `--execute` / `--dry-run` のみ（`src/main.rs:68-76`）。人間が移動先を選ぶ経路は存在しない（原典 §1 問題 E） | Jev の提案を人間が上書きできない |

### 既存テストへの影響（意図的変更のみ）

- `tests/v052_zero_config_cli_test.rs::v052_help_still_lists_every_command` は「全コマンドが列挙され、`discover` が現れない」ことを固定している。
  ZC-12 の文言変更後も**この契約は維持**する（コマンド一覧は不変、`discover` は非表示のまま。監査で確認）。
- `tests/organize_output_test.rs` / `tests/v31_render_test.rs` は既存シグネチャの `render_panel` / `render_text` を呼ぶ。
  ZC-5 は**既存シグネチャを変えず**、閾値・`--explain` を受ける `*_with` 系を追加して main だけが新経路を使う（既存の期待値テストは不変）。
- `tests/v052_zero_config_key_test.rs` は `acquire` 契約のテスト。ZC-13 は `acquire` / `resolve_for_jev_command` を**呼ばない**経路として実装し、無変更。

## 2. 振る舞い（BDD シナリオ / 具体例）

### C1. ZC-5: 閾値近傍の可視化と `--explain`

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C1-1 | 正常系（近接 / confidence） | `confidence=0.840, margin=0.500, 閾値 default` → REVIEW。POLICY セクションに `near auto move: 0.010 more confidence would reach AUTO MOVE` が追加される（既存行は不変） |
| C1-2 | 正常系（近接 / 両不足） | `confidence=0.840, margin=0.380` → `near auto move: 0.010 more confidence and 0.020 more margin would reach AUTO MOVE` |
| C1-3 | 境界（窓の外） | `confidence=0.780`（0.070 不足）→ 近接行は出ない。`confidence=0.850`（到達）→ 近接行は出ない（既に AUTO MOVE） |
| C1-4 | 正常系（`--explain`） | EXPLANATION セクションに `Confidence` / `Margin` / `Thresholds` / `Verdict` / `Destination` の 5 行。実効閾値（user TOML 上書き値）を使う |
| C1-5 | JSON（classify `--explain`） | 既存キーを保ったまま `explanation` オブジェクトを追加（`confidence` / `margin` / `thresholds` / `policy` / `destination_id` / `destination_path` / `near_auto_move` / `confidence_gap` / `margin_gap`）。`--explain` 無しの JSON は**バイト単位で不変** |
| C1-6 | JSON（organize `--explain`） | ZC-7 の organize JSON に `explanation` キーを追加（同じ構造） |
| C1-7 | 無影響 | `thresholds` 未指定（既定）の近接表示は window 定数 0.05 のテストで固定。既存の classify/organize 表示テスト（0.36/0.90/0.70 系）は不変 |

### C2. ZC-6: policy の概念の明示（`--help`）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C2-1 | 正常系 | トップレベル `-h` / `--help` に `AUTO MOVE` / `REVIEW` / `LEAVE` の意味と既定閾値（0.85 / 0.40 / 0.60）、`kaseifu config show` で実効値が読める旨、`--execute` を付けない限り動かない旨が ASCII で出る |
| C2-2 | 整合 | 独立コマンド `policy` は追加しない（`kaseifu policy` は従来どおり未知コマンド） |
| C2-3 | 禁止語 | 上記文言に `Phase`・内部型名・パイプライン語を含まない（C5 の全ヘルプ走査で固定） |

### C3. ZC-7: `organize --format json`

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C3-1 | 正常系（dry run / AUTO MOVE） | `organize <file> --format json` が 1 個の JSON オブジェクトを出し、`decision`（既存 `JevDecision` 形）/ `policy`（`auto_move` 等）/ `candidate_path` / `outcome.type = "would_move"` + `destination` / `mode = "dry_run"` を取れる |
| C3-2 | 正常系（`--execute`） | `outcome.type = "moved"` + `operation_id` + `destination`。ジャーナルに記録される |
| C3-3 | 異常系（REVIEW / LEAVE） | `outcome.type = "unplanable"` + `kind`（`not_auto_move` 等）+ `reason`（人間向け文）。ファイルは動かない |
| C3-4 | 互換 | プレーン表示（非 JSON）は従来の `== Mode ==` 形式のまま不変。JSON は追加のみ（既存キー無し） |
| C3-5 | 実測 | ローカル HTTP モック Jev + PATH 注入の偽 `mdfind` で決定論的に実行し、パース結果をテストで固定 |

### C4. ZC-8: 動画の扱いの明示

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C4-1 | ドキュメント | README に「対象: 画像 9 拡張子（実装の `is_image_file` と一致）」「対象外: 動画（mp4/mov 等）。撮影日時・GPS は読まず Jev にも送らない」が明記される |
| C4-2 | ヘルプ | トップレベル `--help` にも同じ対象/対象外が 1 段落で出る（内部構造の変更なし） |
| C4-3 | 監査 | 拡張子リストが `is_image_file` と一致することを README とテストで固定（`config`/`doctor` に新しい検査を足さない: P0 の 9 チェックは不変） |

### C5. ZC-12: ヘルプをユーザーの操作に必要な情報だけにする

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C5-1 | 全出力走査 | トップレベル + 可視の全サブコマンド（`auth` / `config` の入れ子含む）+ 隠し `discover` の `-h` と `--help` の全出力に `Phase`・内部型名（`FileContext` / `NormalizedFileContext` / `JevDecision` / `DecisionContext` / `CandidateDestination` / `OrganizeReport` / `MovePlan` / `PlanError`）・パイプライン語（`Observe ->` 等）・`decision -> policy` が現れない |
| C5-2 | フラグの実在 | `--format` のヘルプが「`json` を実際に返すコマンド」を列挙し、`scan` / `undo` / `watch` はテキストのみと明記する。`scan` の説明は実態（バナーのみ）に合わせる |
| C5-3 | 安全性 | ドライラン既定・`--execute` 必須・鍵経路（Keychain / `KASEIFU_KEY_SOURCE` / `.env`）の情報がヘルプから読み取れる |
| C5-4 | 互換 | トップレベル `--help` は全コマンドを列挙したまま（`v052_help_still_lists_every_command` 緑）。`discover` は README・help・補完・案内に現れない |
| C5-5 | ASCII | 全ヘルプが ASCII（em dash なし。既存の clap 出力を含め走査） |

### C6. ZC-13: 対話的な移動先選択（`organize <file> --select`）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C6-1 | 正常系（TTY / 選択 → 計画） | 候補が番号付きで表示（ID・パス・score・関連ファイル数）。番号入力 → `[MANUAL]` 付きの移動計画（From / Destination / 根拠）を表示。既定はドライランで何も動かない。exit 0 |
| C6-2 | 正常系（TTY / `--execute --yes`） | 計画表示 → 移動実行 → `moved` 表示（operation id と undo 案内）。ジャーナルに `candidate_id` NULL・confidence/margin NULL の `move` 行（人間指定であることの区別） |
| C6-3 | 正常系（TTY / `--execute` 確認） | 確認プロンプト `Move this file? [y/N]`。`n` / 空入力で中止（何も変更せず exit 0） |
| C6-4 | 中断 | `q` で何も変更せず exit 0。SIGINT（Ctrl+C 相当）で終了し、ファイル・ジャーナルに変更が残らない |
| C6-5 | 異常系（非TTY） | 対話を開始せず非0で終了。`--format json` での候補列挙と将来の `--to <dir>` を案内（`discover` は案内しない） |
| C6-6 | 正常系（JSON） | `--select --format json` は対話せず候補一覧（id / path / score / evidence_count）を JSON で出す（`discover` と同じ流儀） |
| C6-7 | 境界（不正入力） | 範囲外・非数値は再入力を促す（クラッシュ・変更なし） |
| C6-8 | 安全（除外規則） | `~/Library/**`・`Caches`・`/var/**` 等の除外パスは候補に出ない。計画層でも拒否（`build_manual_move_plan` が `DestinationExcluded`） |
| C6-9 | 安全（衝突） | 同名ファイルが存在する移動先は計画段階で拒否（上書きしない） |
| C6-10 | Undo 整合 | 選択した移動が `undo` で戻り、ジャーナルは move + undo の 2 行。`doctor` の `Journal` 行と矛盾しない |
| C6-11 | 鍵不要 | `KASEIFU_KEY_SOURCE=env` + 鍵未設定 + TTY で候補表示 → 選択 → 移動まで動作。鍵プロンプトは出ず、Jev への HTTP リクエストも発生しない（Jev を構築しない構造。`JEV_API_URL` は閉ポートを指す） |

## 3. 設計判断（決定と理由）

### 3.1 ZC-5: 近接表示と `--explain`

- 新規 `src/presentation/renderers/explain.rs` に `Explanation`（confidence / margin / 実効 `Thresholds` / 判定 / 解決先 / `confidence_gap` / `margin_gap` / `near_auto_move`）を置き、
  `near_line()`（表示文）・`to_json()`（構造化）・`verdict()` を提供する。近接窓は定数 `NEAR_MOVE_WINDOW = 0.05`（原典の例 0.80〜0.85 を覆う）。
  「近接」= AUTO MOVE 未満で、不足しているギャップが**すべて** 0 より大きく 0.05 以下。
- 既存シグネチャは不変（`render_panel` / `render_text` は既定閾値・explain 無しで従来経路）。main は `render_panel_with` / `render_text_with`（閾値 + explain）を使う。
- `--explain` の JSON は既存オブジェクトへの**追加キー** `explanation` のみ（既存キーの削除・変更なし。`--explain` 無しは完全に不変）。
  classify の非 Rich 出力は `decision` JSON そのものなので、`--explain` 時のみ `explanation` を足す。
- 「あと◯」の閾値は**実効値**を使う（`policy_view` の既存文言は既定値のまま。DECISION/POLICY セクションの既存出力を変えないため。EXPLANATION が実効値の正典）。

### 3.2 ZC-6: ヘルプ本文（`after_help`）

- `Cli` に `#[command(after_help = ...)]` を追加（`-h` と `--help` の両方に出す）。3 値の意味と既定閾値、`kaseifu config show` で実効値を見る導線、`--execute` 必須を 4 行で示す。
- 閾値をヘルプに**動的注入しない**理由: clap の derive 経路（`Cli::parse`）で動的 help を作るには match を手組みする必要があり、`--help` が user TOML 読み込みに依存して失敗し得る。既定値 + `config show` 導線で受入基準（閾値と意味が読み取れる）を満たす。
- 独立コマンドは追加しない（原典どおり）。

### 3.3 ZC-7: organize JSON

- `organize_view::json_report(report, source, execute_requested) -> serde_json::Value` を追加し、main の `format == Json` 分岐で `serde_json::to_string_pretty` を出す。
- 形: `{ "source", "mode", "decision", "policy", "candidate_path", "outcome": {"type", ...} }`。`outcome.type` は `moved` / `would_move` / `refused` / `unplanable`（+ `kind` = `not_auto_move` 等の機械語 + `reason` 人間文）/ `move_failed`。
- `decision` は既存の `JevDecision` serde、`policy` は既存の `DecisionAction` serde（`rename_all = snake_case`）。新しい独自スキーマを発明しない（`inspect` / `classify` / `discover` の流儀に一致）。
- プレーン表示は不変（既存 `render_text` をそのまま）。`--format json` は「JSON を出す」に変更（原典 ZC-7 の要求）。

### 3.4 ZC-8: 対象/対象外の明示（README + `--help`）

- README に「What kaseifu classifies today」節を追加（対象 9 拡張子 = `is_image_file` と一致、動画は対象外・メタデータも送らない）。
- 既存の Spotlight 節に、`mdfind`（Discovery の検索）の索引反映が非同期で、書き込み直後のファイルは出ないことがある（実測 約 4 秒）ことを 1〜2 行で明記（R6。**新しい ZC 番号は振らない**。既存文の「about 4 s measured」を残しつつ `mdfind` と明示）。
- `doctor` の検査数（9）は変えない（P0 テストが固定）。対象/対象外は `--help` の after_help で示す。

### 3.5 ZC-12: ヘルプ文の書き換えと決定事項

- `scan` は**残す**（互換）が、説明を実態に合わせる（バナー出力のみ・スキャン未実装）。非表示化はしない（既存スクリプトの互換優先。判断をここに記録）。
- `--format` の help を「`json` が効くコマンド」の列挙 + `scan` / `undo` / `watch` はテキストのみ、に変更（`discover` の語は help に出さない: 既存テスト `!contains("discover")` を守るため）。
- `--execute` / `--dry-run` は相互排他と既定をそれぞれ 1 行で明示。`completions` は「`--install` は zsh のみ、他シェルは stdout へ印刷」を 1 行で明示。
- 内部型名・`Phase`・パイプライン語を doc コメントから除去（表示される文言のみ。内部 doc コメントは残してよい）。

### 3.6 ZC-13: コマンド形と安全設計

**決定: `kaseifu organize <file> --select`（推奨案）を採用。** 理由: 既存 `organize <file>` の挙動を一切変えず、対話モードを明示フラグに閉じ込められる（代替の `select` サブコマンドは新コマンド面を増やすだけで、`--select` と同じフローを呼ぶなら重複表示・重複 help になる）。真の入れ子 `organize select` は位置引数 `FILE` と共存不可のため不採用（原典どおり）。

- **Jev は呼ばない（鍵の有無に関わらず）**: 原典 C6 の「Jev は呼ばない（鍵不要で動作する）」を優先し、候補の列挙は Discovery のみ。
  したがって「Jevの提案」行は出さない（例示ではなく制約「Jev の選択は参考表示のみ」を、Jev の選択が決定に一切使われない形で満たす。決定権は常に人間）。
  この判断を §7 に記録し、Reviewer の確認対象にする。
- フロー: 非 JSON は TTY 必須 → `pipeline::discover_candidates`（新規公開: observe → normalize → discover、Jev 無し）→ メニュー表示（stderr にプロンプト、stdout にメニュー）→ 入力を `operations::select::parse_select_command` で解釈（`q` / 番号 / 再入力）→ `plan::build_manual_move_plan` → 計画表示 → `--execute` は `--yes` 以外で確認 → `mover::execute_manual_move`。
- **ジャーナルの人間指定の区別**: `execute_manual_move` は `candidate_id = NULL` / `confidence = NULL` / `margin = NULL` で記録する（原典の例に一致。Jev の confidence/margin を持たない事実をそのまま表す）。`undo` は `entry.candidate_id`（NULL 可）をそのまま引き継ぐため変更不要。`doctor` の Journal 行（件数・pending）とも矛盾しない。
- **除外規則の再利用**: 計画層 `build_manual_move_plan` が `discovery::is_excluded_destination` を呼び、`PlanError::DestinationExcluded` で拒否（表示は人間向け文）。
  候補は Discovery の集約ゲートで既に除外済みだが、選択肢に出ないこと + 選んでも拒否されることを二重に担保する。
- **衝突**: 既存の `Collision` 検査を共用（部分関数 `plan_destination` に抽出し、`build_move_plan_with` と `build_manual_move_plan` の両方が同じ検査を通る）。
- 表示: `[MANUAL]` タグを計画表示に付ける。ASCII のみ、Rich/plain の既存部品を再利用。
- `--explain` は `--select` と排他（clap `conflicts_with`。対話選択に Jev 判定が無いため）。

## 4. タスク一覧（実装順・直列厳守）

`src/main.rs`・`README.md`・`CHANGELOG.md` が衝突するため 1 項目ずつ完了・検証してから次へ進む。

| # | タスク | 主なファイル | 期待する振る舞い（受け入れ） | テスト |
|---|---|---|---|---|
| T1 | ZC-5: `explain.rs` 新設、近接行 + EXPLANATION セクション + JSON `explanation`（classify / organize） | `src/presentation/renderers/{explain,generic,classify,organize}.rs`, `src/main.rs` | C1-1〜C1-7 | 新規 `tests/v070_zero_config_p1_explain_test.rs`（レンダラ単体 + モック Jev CLI） |
| T2 | ZC-6: トップレベル `after_help`（policy 3 値・閾値・`--execute`） | `src/main.rs` | C2-1〜C2-3 | 新規 `tests/v070_zero_config_p1_help_test.rs` |
| T3 | ZC-7: `organize --format json`（`json_report`） | `src/presentation/renderers/organize.rs`, `src/main.rs` | C3-1〜C3-5 | 新規 `tests/v070_zero_config_p1_organize_json_test.rs`（モック Jev + 偽 mdfind） |
| T4 | ZC-8: README 対象/対象外 + R6（`mdfind` 約 4 秒）+ after_help に scope 段落 | `README.md`, `src/main.rs`, `CHANGELOG.md` | C4-1〜C4-3 | help テストに scope 走査を追加 |
| T5 | ZC-12: 全 help 文言の書き換え（Phase / 型名 / パイプライン語 / scan / format / completions / 排他） | `src/main.rs` | C5-1〜C5-5 | `tests/v070_zero_config_p1_help_test.rs`（全出力走査） |
| T6 | ZC-13: `operations/select.rs` + `renderers/select.rs` + `build_manual_move_plan` + `execute_manual_move` + `pipeline::discover_candidates` + `--select` / `--yes` | `src/operations/{select,plan,mover,mod}.rs`, `src/presentation/renderers/select.rs`, `src/classifier/pipeline.rs`, `src/main.rs` | C6-1〜C6-11 | 新規 `tests/v070_zero_config_p1_select_test.rs`（PTY + 非TTY + JSON） |
| T7 | ドキュメント: README 使用例（`--select` / `--explain` / `organize --format json`）/ CHANGELOG `[Unreleased]` | `README.md`, `CHANGELOG.md` | — | 目視 + help テスト |
| T8 | 品質ゲート: fmt / clippy / test / `quality_check.sh` OVERALL PASS、検収（Reviewer / QA issue） | `reports/quality/`（生成物は管理外） | C1〜C6 の実測 | 実測 |

### 品質ゲート（C7）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C7-1 | テスト | 既存 387 + 新規が全緑。**テスト数を減らさない** |
| C7-2 | 複雑度 | 新規 CC16+ なし、既存最大 CC 15 を悪化させない |
| C7-3 | 依存 | 追加クレート・追加依存なし（テストは std + 既存 dev 依存 `libc` / `serde_json` のみ） |
| C7-4 | 安全 | テストは隔離 HOME・一時ディレクトリ（`CARGO_TARGET_TMPDIR` と `std::env::temp_dir()`）・注入経路（PATH 上の偽 `mdfind` / ローカル HTTP モック）のみ。実 Keychain・実ジャーナル・`~/Desktop` を読み書きしない |

## 5. 受け入れ基準（実測で示す）

1. classify / organize の AUTO MOVE 直下で近接表示が出て、`--explain` が confidence / margin / 実効閾値 / 解決先を示す（レンダラ単体 + CLI 実測）。
2. トップレベルヘルプから 3 値の意味・閾値・`--execute` 必須が読み取れる。
3. `organize --format json` を実行し、decision / policy / outcome をパースで取得できる（モック Jev 実測）。
4. README とヘルプに対象（画像 9 拡張子）と対象外（動画）が明記され、README の拡張子リストが `is_image_file` と一致するテストがある。
5. 全ヘルプ出力（トップレベル + 全サブコマンド + 入れ子、`-h` / `--help`）に禁止語が 0 件。表示フラグは機能するものだけ（しないものは明記）。
6. PTY で候補表示 → 番号選択 → 計画 → `--execute --yes` で移動 → `undo` で復元 → `doctor` / journal と整合。`q` / SIGINT で変更なし。非TTY は非0。除外候補は出ない。鍵未設定で動作。
7. `cargo fmt --check` / `cargo clippy -D warnings` / `cargo test --all-targets --all-features` / `bash scripts/quality_check.sh` OVERALL PASS。

## 6. リスクと対策

| リスク | 対策 |
|---|---|
| PTY テストの候補が Spotlight 実索引に依存する | PATH 注入の偽 `mdfind`（決定論的）で候補を供給。候補ディレクトリは除外規則に掛からない `CARGO_TARGET_TMPDIR` 配下に作る（`std::env::temp_dir()` は `/var/**` のため除外される） |
| Ctrl+C が PTY の前景プロセスグループに届かない | PTY ハーネスは `libc::kill(child, SIGINT)` で SIGINT を直接送る（Ctrl+C 相当）。加えて `q` 経路を別テストで固定 |
| `organize --format json` が既存の text 前提の利用を壊す | ZC-7 の要求そのもの（原典 F-07）。`--format auto/rich` の挙動は不変。テストで両方を固定 |
| ヘルプ文言変更で完了スクリプト/既存 help テストが落ちる | 完了スクリプトはバージョンマーカー付きで自動再生成。既存 help テストは「コマンド列挙 + discover 非表示」のみ固定で文言非依存（監査済み） |
| 新規コードの CC16+ | 小さな関数に分割（select の入力解釈・メニュー生成・計画表示を分離）。`quality_check.sh` の baseline-gate で実測 |
| 対話フローがテストでハング | 全ての子プロセス実行にデッドライン（既存 `run_with_deadline` / `run_in_pty` を踏襲） |

## 7. 設計判断の記録（Reviewer 確認対象）

1. **ZC-13 は Jev を呼ばない**（鍵の有無に関わらず）。原典の「Jev は呼ばない」を優先し、「Jev の選択は参考表示のみ」は Jev の選択が出力に一切使われない形で満たす。
2. **ZC-13 のコマンド形は `organize <file> --select`**（§3.6）。
3. **`scan` は非表示化せず実態に合わせる**（互換優先）。
4. **ヘルプの閾値は既定値 + `config show` 導線**（動的注入は `--help` を設定ファイル依存にするため不採用）。
5. **`--format json` の非対応コマンドは help に明記**（フラグ撤去はグローバル引数のため不可）。
6. **R6（Spotlight 非同期・約 4 秒）は新規 ZC 番号を振らず README に明記**（既存の Spotlight 節を具体化）。

## 8. 検収記録

| 工程 | 成果物 | 判定 |
|---|---|---|
| Plan Architect | `issues/20261016-plan-architect-zero-config-p1.md` | 承認（F1〜F7 を計画に反映済み） |
| Reviewer | `issues/20261016-reviewer-zero-config-p1.md` | APPROVE（blocking 0。MINOR/NIT は記録のみ） |
| QA | `issues/20261016-qa-zero-config-p1.md` | PASS（quality_check.sh OVERALL PASS / 428 passed / CC>=16 0 / 新規依存 0 / 隔離監査 OK） |

### 完了時の実測サマリ（2026-10-16）

- テスト: 既存 387 → **428 passed / 0 failed**（新規 41: explain 14 / help 8 / CLI 6 / select 13）。
- `bash scripts/quality_check.sh`: **OVERALL: PASS**（rustfmt / clippy / test / complexity / baseline-gate / report-tests / coverage 85.05% report-only）。
- ZC-5: 近接行（窓 0.05・実効閾値）+ `--explain`（EXPLANATION 5 行 / JSON `explanation`）。既存表示・JSON は不変。
- ZC-6: トップレベル help に 3 値の意味・既定閾値・`config show` 導線・`--execute` 必須。
- ZC-7: `organize --format json` 実測（decision / policy / outcome をパースで取得。3 形態をテスト固定）。
- ZC-8: README + help に対象 9 拡張子 / 動画対象外 / R6（`mdfind` 約 4 秒）。README リストは `is_image_file` と機械照合。
- ZC-12: 36 ヘルプ出力の禁止語 0 件（テスト固定）、`--format` の対応範囲明記、`scan` / `completions` / 排他フラグの説明を実態に一致。
- ZC-13: PTY で候補表示→選択→計画→`--execute --yes` 移動→`undo` 復元→`doctor` 整合。`q` / SIGINT / 確認辞退は変更 0。非TTY は非 0。除外候補は不出現 + 計画層拒否。鍵未設定・ネットワークなしで動作（Jev 呼び出しなし）。

## 9. 実測エビデンス（追記式）

### T1: ZC-5（2026-10-16）

- `cargo test --test v070_zero_config_p1_explain_test` = **9 passed / 0 failed**
  （近接行の文言・窓 0.05 の境界・実効閾値の反映・EXPLANATION 5 行・既存 `render_panel` / `render_text` の完全一致・JSON 追加キー）
- `cargo test --all-targets --all-features` = **401 passed / 0 failed**（ベース 387 + 14）
- CLI 実測（モック Jev + 偽 mdfind・隔離 HOME・confidence 0.840 / margin 0.500）:
  ```text
  == Policy ==
    [REVIEW] REVIEW
    below auto-move thresholds; human review required (review confidence >= 0.60)
    near auto move: 0.010 more confidence would reach AUTO MOVE
  == Explanation ==
    Confidence   0.840 (auto move needs >= 0.850)
    Margin       0.500 (auto move needs >= 0.400)
    Thresholds   auto move: confidence >= 0.850 and margin >= 0.400; review: confidence >= 0.600
    Verdict      REVIEW
    Destination  D1 - ./target/tmp/manual-evidence/dest-alpha
  ```
- `--explain` 無しの organize テキストには `near auto move` 行以外の変更が無い（既存テスト緑 + `historical_entry_points_keep_their_exact_output`）。

### T2: ZC-6

- `cargo test --test v070_zero_config_p1_help_test` = **1 passed**（`-h` / `--help` の両方で AUTO MOVE / REVIEW / LEAVE・既定閾値 0.85/0.40/0.60・`kaseifu config show` 導線・`--execute` 必須・「policy はコマンドではない」を固定。ASCII のみ）

### T3: ZC-7

- `cargo test --test v070_zero_config_p1_cli_test` = **6 passed**（全て実 CLI + モック Jev + PATH 注入の偽 mdfind + 隔離 HOME）
- `organize --format json`（REVIEW 例、実測）:
  ```json
  {
    "candidate_path": ".../dest-alpha",
    "decision": { "action": {"move_to": "D1"}, "confidence": 0.84, "destination_id": "D1", "margin": 0.49999999999999994 },
    "mode": "dry_run",
    "outcome": { "kind": "not_auto_move", "reason": "policy refused: automatic move not allowed", "type": "unplanable" },
    "policy": { "review": "D1" },
    "source": ".../inbox/vacation-note-2026.txt"
  }
  ```
- テストで固定した 3 形態: dry-run AUTO MOVE = `outcome.type=would_move` + destination / `--execute` = `type=moved` + operation_id + ジャーナル 1 行（pending 0）/ REVIEW = `type=unplanable` + `kind=not_auto_move` + reason。
- プレーン表示は不変（既存 `render_text` テスト緑）。
- 注記: `--explain` 指定時の JSON は serde_json のキー順（アルファベット順）になる（値・キーは完全に保持。既定出力は従来の構造体順のまま不変）。

### T4: ZC-8

- `cargo test --test v070_zero_config_p1_help_test` の `readme_scope_list_matches_the_image_predicate` が README の拡張子リスト（9 個）を機械的に抽出し、`is_image_file` と一致することを固定（`.mp4` / `.mov` は false）。
- README に `Spotlight indexing is asynchronous` / `may not appear in \`mdfind\`` / `4 s measured on an idle machine` の 3 アサーションで R6 を固定（新規 ZC 番号なし）。
- `--help` の `Scope:` 段落を `help_states_the_image_scope_and_video_exclusion` で固定。`doctor` の 9 チェックは不変（既存テスト緑）。

### T5: ZC-12

- `every_help_output_avoids_internal_phases_types_and_pipeline_words`: トップレベル + 可視 11 コマンド + 入れ子（`auth` 3 / `config` 2）+ 隠し `discover` の計 18 対象× `-h`/`--help` = 36 出力を走査。禁止語（`Phase` / 型名 8 種 / パイプライン語 3 種 / `DETAILS layer`）0 件、全出力 ASCII。
- `v052_help_still_lists_every_command`（全コマンド列挙 + `discover` 非表示）と `completion_test`（zsh スクリプトに `discover` 0 件）緑。
- 変更前→後（例）: `classify` = "(Phase 4) / Observe -> Normalize -> ... / validated JevDecision" → "Decide where a file belongs without moving anything."、`inspect` = "Build and display FileContext JSON." → "Show what kaseifu observes in a file (read-only)."、`--verbose` = "Show the DETAILS layer (...)" → "Include full paths, the report schema and raw metadata."、`scan` = "Scan a directory." → "Print the target banner for a directory (scanning is not implemented yet)."
- `--format` のヘルプが「`json` は inspect / classify / organize / doctor / setup / auth / config / completions --install、`scan` / `undo` / `watch` は常にテキスト」を明記（`format_help_states_which_commands_support_json`）。
- `--execute` / `--dry-run` の排他、`completions` の zsh 限定、`setup` / `auth` の鍵経路をテストで固定。

### T6: ZC-13

- `cargo test --test v070_zero_config_p1_select_test` = **10 passed / 0 failed**（非TTY 案内・JSON 候補列挙・JSON+execute 拒否・計画層の除外/衝突・NULL ジャーナル + undo・PTY 4 本）。
- 実 PTY ログ（手動、隔離 HOME・偽 mdfind・鍵なし・閉ポートの JEV_API_URL）:
  ```text
  +-- * KASEIFU / SELECT ------+
  | Target: ./vacation-note-2026.txt |
  |   1) D1  .../dest-alpha  score 0.74  (1 related file) |
  |   2) D2  .../dest-beta   score 0.72  (1 related file) |
  | q) cancel (nothing changes) |
  Select a destination number (or q to cancel): 1
  +-- * KASEIFU / SELECT [MANUAL] ---+
  | From         ./vacation-note-2026.txt |
  | Destination  .../dest-alpha/vacation-note-2026.txt |
  | Basis        D1 - .../dest-alpha (score 0.74, 1 related file; you selected it) |
  | Mode         execute mode |
  +-- * KASEIFU / SELECT [MANUAL] / MOVED ---+
  | Operation    op-87130-1791914639452635000 |
  | Undo         kaseifu undo reverses this move |
  ```
  exit 0 / ジャーナル生成 / `q` ・SIGINT ・確認辞退は変更 0 / `undo` で復元 + `doctor` の `ok Journal: ... 2 operation(s); no pending`。
- 候補に出ない除外例: `~/Library/Application Support/SeedStore`、`/var/folders/...`、`Eagle.library/...`、タイムスタンプ名フォルダ（テストで固定）。計画層は `DestinationExcluded` で拒否。
- 秘密・鍵: 鍵プロンプト 0 回、`.env` 生成 0 件、Keychain 不参照（env モード）、JEV_API_KEY の出力 0 件。

### T7: ドキュメント

- README: 「What kaseifu classifies」節（対象 9 拡張子・動画対象外）、Spotlight 節の R6 明文化、`--explain` / `organize --format json` / `--select` の使用例。
- CHANGELOG `[Unreleased]` に Added / Changed（P1 の 6 項目）。

### T8: 品質ゲート

- `bash scripts/quality_check.sh` = **OVERALL: PASS**（rustfmt 0 / clippy 0 / cargo-test 428 passed, 0 failed / complexity PASS / baseline-gate PASS / report-tests PASS / coverage 85.05% report-only）。
- CC: 687 関数、CC<=10 = 683（**99.4%**）、CC 11-15 = 4、CC>=16 = **0**、最大 15（ベースライン不悪化）。`main` は 16 → 2（`dispatch` 抽出）。`dispatch`（CC 14）は warn-only の “needs review”（ゲート非阻害）。
- 新規テスト 41 件（387 → 428）。新規依存 0。

