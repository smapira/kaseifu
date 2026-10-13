# ゼロコンフィグ P0（ZC-1 / ZC-2 / ZC-3 仕上げ / ZC-4）実装計画

- 起票: 2026-10-14
- 原典: `documents/plans/user/20261013d.md`（ゼロコンフィグ化 改善計画 v1.0、§4 P0）
- 前提（前バッチ、未コミット）: `documents/plans/dev/20261015b-journal-safety-config-honesty.md`
  （ジャーナル絶対パス化 R5 / `Legacy Journal` 検出 R5 / pending 検出 R1・R2 / 設定の正直さ F-05・F-12）。**revert しない**
- 対象: `src/main.rs`, `src/auth/mod.rs` + 新規 `src/auth/acquire.rs`, `src/doctor.rs`,
  `src/storage/history.rs`（変更なし予定）, 新規テスト 2 ファイル,
  `tests/v051_journal_path_test.rs`（ZC-3 表示変更に伴う意図的更新）, `README.md`, `CHANGELOG.md`
- 対象外（次バッチ以降）: ZC-5〜ZC-13（P1/P2/アンインストール/ヘルプ見直し/対話選択）。**実装しない**
- 制約: 秘密情報を出力・ログ・JSON・テストに含めない / 実ユーザーの `~/Library/Application Support/kaseifu/`・
  `~/Desktop/`・旧 `./data/`・実 Keychain をテストで読み書きしない（隔離 HOME と注入経路のみ）/ 新規依存なし /
  新規 CC16+ なし / `KASEIFU_KEY_SOURCE`（`auto`/`env`/`keychain`、デバッグ既定 `env`）の意味を変えない /
  `discover` はデバッグ専用のまま / `--format json` の既存キーを削除しない（追加のみ）/ commit・tag・push は親が実施
- ベースライン実測（2026-10-14、worktree `kaseifu`、前バッチ未コミット込み）:
  `cargo test --all-targets --all-features` = **367 passed / 0 failed**
- 確認手段: serena / graphify / codebase-memory-mcp は本環境で未接続のため、LSP 相当の直接読解と `grep`
  （`src/main.rs` / `src/doctor.rs` / `src/auth/prompt.rs` / `src/config/*` / `src/storage/history.rs` / `tests/*`）で実コードを確認した

## 1. 背景と現状実測（変更前）

| ID | 現状（実測・コード根拠） | 影響 |
|---|---|---|
| ZC-1 | 鍵未解決時の `require_jev_api_key`（`src/main.rs:628-641`）は `env`/`keychain` のみ即エラー、`auto` は素通り。TTY 判定・対話入力は無い。マスク入力は `auth::prompt::prompt_secret`（`src/auth/prompt.rs:17-21`）に既存。`.env` への**書き込み**機構は存在しない（読取は `dotenvy::from_path_iter`、`src/config/resolver.rs:358`） | 初回ユーザーが「no API key is configured」で行き止まり（原典 §1 問題 C） |
| ZC-2 | `Cli.command` は必須（`src/main.rs:33-44`）。引数なしは clap の usage エラー（exit 2）。`main` は `Completions`/`Doctor` 以外で `completion::refresh_if_stale` を実行（`src/main.rs:210-217`） | 引数なしで次に何をすべきか伝わらない（原典 §1 問題 D） |
| ZC-3 | ジャーナルの絶対パス化と `Legacy Journal` 検出は前バッチで実装済み（`src/storage/history.rs:26-37`, `src/doctor.rs:257-273`）。現在の文言は `found {} (old CWD-relative journal; not read, not migrated; current journal: {})` で、行数・削除可否の案内が無い | 「0 操作なら削除してよい」が分からず、旧ファイルが残り続ける |
| ZC-4 | `CheckResult` は `name`/`ok`/`detail` のみ（`src/doctor.rs:28-36`）。FAIL のうち次コマンドを併記するのは `Completion` と（間接的に）`API Key`（auto のみ `run kaseifu setup`）だけで、`Configuration` / `Jev Endpoint` / `Spotlight` / `Storage` / `Journal` / `Legacy Journal` / `Platform` は対応手段を明示しない | FAIL 行が行き止まりになる |

### 既存テストへの影響（意図的変更）

- `tests/v051_journal_path_test.rs:189-191` は旧文言 `not read, not migrated` を固定している。ZC-3 の表示変更（読み取り専用の行数確認 + 削除可否）に伴い、**同テストを意図的に更新**する（ファイル不変の検証はそのまま維持）。
- `tests/key_source_cli_test.rs:251-264` は `no API key is configured` と `KASEIFU_KEY_SOURCE=env` の語を固定している。ZC-1 の非TTY案内はこの**文言を維持**したまま案内を追加する（テスト変更なし）。

## 2. 振る舞い（BDD シナリオ / 具体例）

### C1. ZC-1: 鍵が無いときでも使い始められる

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C1-1 | 正常系（TTY / `env` モード） | `KASEIFU_KEY_SOURCE=env`・鍵未設定・TTY で `classify <file>` → プロンプト表示 → 入力 → `./.env` に保存（無ければ 0600 で作成、既存 `JEV_API_KEY=` 行は置換、他行は保持）→ 以降の処理（判定表示）まで到達。**Keychain 呼び出し 0 回** |
| C1-2 | 正常系（TTY / `auto`・`keychain`） | 保存先は macOS Keychain（既存 `auth set-key` と同一経路 `SecretStore::set`）。保存後に再解決して同じ値が使われる |
| C1-3 | 異常系（非TTY） | ハングせず終了コード非0。案内に `kaseifu setup` と鍵なしで使える `inspect` / `doctor` を含む。`discover` は含めない |
| C1-4 | 異常系（空入力） | 保存しない・処理を続けない（exit 非0、`.env`・Keychain に書き込みなし） |
| C1-5 | 異常系（中断） | 入力読取が失敗/中断したら何も保存しない（保存は読取成功後だけ） |
| C1-6 | 秘密の非漏洩 | 入力した鍵の値が stdout / stderr / JSON / エラー文に現れない（テストで grep 0 件） |
| C1-7 | 境界（`watch`） | 対話はウォッチ開始**前**に完了し、実行中にプロンプトを出さない（`handle_watch` の順序で固定） |
| C1-8 | 境界（`env` の非接触） | `env` モードでは読み書きとも Keychain に触れない（注入ストアの呼び出し回数 0 で証明） |

### C2. ZC-2: 素の `kaseifu` を案内表示に

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C2-1 | 正常系（TTY / 非TTY） | 引数なしで終了コード 0。概要1行 + `classify`/`organize`/`doctor` と用途 + 「`--execute` を付けない限りファイルは動きません」+ 鍵未設定なら `setup`（または ZC-1 の対話）を案内 |
| C2-2 | 互換 | `kaseifu --help` / `-h` は従来どおり全コマンドを表示（終了 0） |
| C2-3 | 無副作用 | `data/` を作らずネットワークも触らない。完了ファイル更新（`completion::refresh_if_stale`）も引数なしでは実行しない |
| C2-4 | 鍵状態の表示 | 鍵あり: `configured (...)`、鍵なし: `not configured; run kaseifu setup ...`、判定不能（TOML 破損等）: `could not be checked; run kaseifu doctor` |
| C2-5 | 表示規約 | ASCII のみ。`NO_COLOR` / 非TTY 準拠。`--format json` でも副作用なし（構造化出力は追加キーのみ） |

### C3. ZC-3（仕上げのみ）: 旧ジャーナルの案内

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C3-1 | 0 行の旧ジャーナル | `FAIL Legacy Journal: found data/kaseifu.db (... 0 operation(s) ... safe to delete ...)` を表示。ファイルは変更・削除されない |
| C3-2 | N>0 行の旧ジャーナル | 行数を表示し、移行しない・削除しない（自動処理なし）ことを明示 |
| C3-3 | 読めない旧ジャーナル | `could not read ...` を表示（行数は出さない）。ファイルは不変 |
| C3-4 | 存在しない | 従来どおり `ok Legacy Journal: no data/kaseifu.db in the current directory` |
| C3-5 | 無干渉 | 検査前後で旧ジャーナルのバイト列が同一。新しいジャーナルも作らない |

### C4. ZC-4: `doctor` を「今すぐ使えるか」の1枚に

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C4-1 | FAIL 行のヒント | 各 FAIL 行に次アクションを併記（例: `(next: kaseifu setup)`、`(next: kaseifu completions zsh --install)`）。検査の意味・名称・既存 detail 語は変えない |
| C4-2 | 正常環境 | 全 ok・終了コード 0・`Ready: yes` |
| C4-3 | 異常環境 | 1 枚の verdict 行（`Ready: no (N check(s) failed)`）と FAIL 行の next が出る |
| C4-4 | `env` モード + 鍵未設定 | 対話を開始せず即時完了（ハングなし、`FAIL API Key: ...` に `(next: kaseifu setup)`） |
| C4-5 | JSON | 既存キー `checks[].{name,ok,detail}` / `connectivity` / `endpoint` を維持し、`checks[].next`・`ready`・`failing` を**追加** |
| C4-6 | ジャーナル検査 | 前バッチの `Journal` / `Legacy Journal` 検査を再追加しない（検査数は 9 のまま） |

## 3. 設計判断（決定と理由）

### 3.1 ZC-1 の実装位置と注入境界

- 対話入力は既存の `auth::prompt::prompt_secret` をそのまま使う（**新しい入力機構を作らない**）。
- 取得〜保存のロジックは新規 `src/auth/acquire.rs` に置き、`SecretStore`（既存 trait）と「秘密を読む関数」を注入可能にする。
  これにより **TTY なしで env モードの「`.env` 保存 + Keychain 非接触」をテストで証明**できる（実物のプロンプトは `main` の薄いラッパのみ）。
- 保存先は `KeySourceMode::keychain_allowed()` で分岐: `auto`/`keychain` → Keychain、`env` → プロジェクトローカル `.env`。
  判定は既存の `keychain_allowed()` を再利用し、モードの意味（`env` は Keychain 非接触）を変えない。
- 保存後は `resolve` をやり直して「正規の経路で解決された鍵」を使う（メモリ上の値を直接使わない。env モードは `.env` を再読、auto/keychain は Keychain を再読）。
- `.env` 書き込みは既存の「一時ファイル + rename」規約（`src/config/loader.rs:210-238`）に合わせ、作成/置換後は 0600 に固定する
  （既存 0644 の `.env` に秘密を書くと世界読み取り可能になるため。他行はそのまま保持）。
- 非TTY の案内文は既存の `require_jev_api_key` の語（`no API key is configured` / `KASEIFU_KEY_SOURCE=env` 等）を維持し、
  末尾に「鍵なしで使えるコマンド（`inspect` / `doctor`）」と `kaseifu setup` を追加する。`discover` はデバッグ専用（`--help` 非表示）のため含めない。
- 空入力は保存前に拒否。中断（読取エラー）も保存前に落ちる。**保存は入力成功後だけ**。

### 3.2 ZC-2 の実装

- `Cli.command` を `Option<Commands>` にし、`None` を案内表示として処理する（`--help` の全コマンド表示は clap のまま維持）。
- 案内は既存の `emit_report`（Rich パネル / プレーン / JSON）と同じ流儀で出し、**終了コード 0**。
- 副作用ゼロのため、引数なしでは `completion::refresh_if_stale` を呼ばない（既存の `Completions`/`Doctor` 除外に `None` を追加）。
- 鍵の状態は `doctor` と同じ「TOML は `unwrap_or_default()`、Keychain は resolver 経由」で読む（書き込みなし）。

### 3.3 ZC-3 は表示のみ（再実装しない）

- `check_legacy_journal` の detail に読み取り専用 `history::inspect_journal` の結果（行数 or 読取不可）を足す。
  `0 operation(s)` のときだけ「安全に削除してよい」旨を出す。**kaseifu 自身は絶対に削除・移動・上書きしない**（文言にも明記）。
- ジャーナルの絶対パス化・`Journal` 検査・pending 検査は前バッチの実装をそのまま使う（重複実装しない）。

### 3.4 ZC-4 の FAIL ヒント

- `CheckResult` に `next: Option<String>` を追加し、**FAIL のときは必ず Some**（`fail()` の引数として必須にする＝コード上で漏れない）。
- 表示は FAIL 行の末尾に ` (next: <ヒント>)`、JSON は `checks[].next`（null = ok）。既存キーは削除しない。
- ヒントは「次に打つコマンド」を基本とし、コマンドで直らない事象（macOS 以外 / ジャーナル読取不可 / pending の人間判断）は、
  その旨と最後に実行する確認コマンドを含める。検査の `ok` 判定・名称・既存 detail 語は変えない。
- 追加で verdict 行（`Ready: yes` / `Ready: no (N check(s) failed)`）を先頭に出す（「今すぐ使えるか」の 1 枚）。
- 検査の追加はしない（9 チェックのまま）。

## 4. タスク一覧（実装順）

| # | タスク | 主なファイル | 期待する振る舞い（受け入れ） | テスト |
|---|---|---|---|---|
| T1 | `auth/acquire.rs` 新設: `save_key_to_dotenv`（0600・置換・他行保持・原子書換）/ `prompt_and_store_key`（モード分岐 + 注入読取）/ `non_tty_guidance` | `src/auth/{mod,acquire}.rs` | C1-1〜C1-6, C1-8 | 新規 `tests/v052_zero_config_key_test.rs`（注入経路 + 一時 `.env`） |
| T2 | `main`: Jev コマンド（`classify`/`organize`/`watch`）の鍵取得を「解決 → 無ければ TTY 対話 / 非TTY 案内エラー」に置換。`watch` は開始前 | `src/main.rs` | C1-1〜C1-3, C1-7 | 新規 CLI テスト（非TTY / PTY）+ 手動実測 |
| T3 | `main`: `command: Option<Commands>` と引数なし案内（exit 0・refresh しない・鍵状態表示） | `src/main.rs` | C2-1〜C2-5 | 新規 `tests/v052_zero_config_guidance_test.rs`（隔離 HOME、`data/` 非生成、`--help` 互換） |
| T4 | `doctor`: `CheckResult.next` 追加 + 全 FAIL にヒント + verdict 行 + JSON 追加キー | `src/doctor.rs`, `src/main.rs`（emit） | C4-1〜C4-6 | 新規/既存テスト更新 |
| T5 | `doctor`: `Legacy Journal` の行数表示（読み取り専用）と 0 行案内 | `src/doctor.rs` | C3-1〜C3-5 | `tests/v051_journal_path_test.rs` 更新 + 新規 |
| T6 | ドキュメント: README（Quick Start / Doctor / 鍵経路）+ CHANGELOG `[Unreleased]` | `README.md`, `CHANGELOG.md` | — | 目視 |
| T7 | 品質ゲート: `bash scripts/quality_check.sh` OVERALL PASS、CC16+ なし、`git add` で新規ファイルを tracked 化 | `reports/quality/`（生成物は管理外） | C5-1〜C5-4 | 実測 |

### 品質ゲート（C5）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| C5-1 | テスト | 既存 367 + 新規が全緑（既存の意図的更新は ZC-3 表示の 1 箇所のみ） |
| C5-2 | 複雑度 | 新規 CC16+ なし（baseline.json の更新は不要な見込み） |
| C5-3 | 依存 | 追加クレート・追加依存なし |
| C5-4 | 安全 | テストは隔離 HOME・一時ディレクトリ・注入経路のみ。実 `~/Library/Application Support/kaseifu/`・`~/Desktop/`・旧 `./data/`・実 Keychain を読み書きしない |

## 5. 受け入れ基準（実測で示す）

1. 鍵未設定・TTY で `classify <file>` が対話入力 → 保存 → 判定表示まで到達（実測）。`env` モードでは `.env` に保存され、Keychain アクセス 0 回（注入経路で証明）。
2. 鍵未設定・非TTY ではハングせず、案内文つきで非0終了。
3. 入力した鍵が出力・ログ・JSON に現れない（grep 0 件）。
4. 引数なし `kaseifu` が終了コード 0 で案内表示、`data/` もネットワークも触らない。`--help` は従来どおり。
5. `doctor` の各 FAIL 行に次コマンドが併記され、正常環境では全 ok・終了 0。`env` モード + 鍵未設定でも即時完了。
6. 旧ジャーナル検出の表示に「0 操作なら削除してよい」旨が含まれ、ファイルは変更されない（実測）。
7. `bash scripts/quality_check.sh` OVERALL PASS、既存テスト全緑、新規 CC16+ なし、新規依存なし。
8. 各要件の振る舞いテストを追加。

## 6. リスクと対策

| リスク | 対策 |
|---|---|
| PTY テストが環境依存（対話経路） | PTY は `libc::openpty`（既存依存）で自己完結に作り、`cfg(target_os = "macos")`。判定表示は候補 0 件でも `NONE`/`LEAVE` に到達する（Jev を呼ばない安全経路）ことを利用 |
| `--format json` 実行時にプロンプトが stdout に混ざる | 既存 `prompt.rs` の挙動を変えない（再利用優先）。ZC-1 の追加情報（保存先の通知）は stderr に出し、stdout の機械可読性を追加で壊さない |
| 引数なし経路が Keychain を読む（release `auto`） | 読み取りのみで書き込みなし。`env` モードでは Keychain 非接触（resolver の既存保証）。テストは debug バイナリ（既定 `env`）で実行 |
| 既存の `Legacy Journal` 文言テストが落ちる | ZC-3 の表示変更は意図的。テストを新文言へ更新し「ファイル不変」の検証は維持 |
| `.env` の権限変更（0600）がユーザーの既存運用と衝突 | 秘密を書く以上 0600 に固定し、保存通知に「mode 0600」を明記。他行は保持 |

## 7. 検収記録

| 工程 | 成果物 | 判定 |
|---|---|---|
| Plan Architect | `issues/20261014-plan-architect-zero-config-p0.md` | 指摘 F1（`.env` 往復整合）/F2（再解決でモード保証）/F3（PTY 自己完結）/F4（JSON 追加キー）/F5（tracked 化）を反映済み。承認 |
| Reviewer | `issues/20261014-reviewer-zero-config-p0.md` | 承認。注意点 3 件（旧ジャーナル N>0 はコマンドなしの行動文 / 空 `JEV_API_KEY=` が Keychain をマスクする既存仕様 / auto の鍵状態表示は Keychain 読みのみ） |
| QA | `issues/20261014-qa-zero-config-p0.md` | PASS（quality_check.sh OVERALL PASS / 385 passed / 新規 CC16+ 0 / 依存追加なし / 隔離監査 OK） |

### 完了時の実測サマリ（2026-10-14）

- テスト: 既存 367 → **385 passed / 0 failed**（新規 18）。
- `bash scripts/quality_check.sh`: rustfmt / clippy / test / complexity / baseline-gate / report-tests / coverage すべて PASS。
- ZC-1 実測: PTY で `classify` → `Jev API key:` → `.env`（0600）→ `POLICY`/`LEAVE` パネル、exit 0、鍵の echo/出力 0 件。非TTY は `classify`/`watch` とも exit 1 + 案内。
- ZC-2 実測: 引数なし exit 0、`data/`・完了スクリプト・stderr の副作用なし。`--help` は全コマンド表示のまま。
- ZC-3 実測: 0 行 = `safe to delete`、N>0 行 = 保持案内、読取不可 = `could not read it`。sha256/size/mode 前後同一。
- ZC-4 実測: FAIL 3 行すべて `(next: ...)` 付き、正常環境は `Ready: yes` + 9 ok + exit 0。
