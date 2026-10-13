# CLI 横断シナリオ仕様 C01–C12（v1.2 §11 の完全化）

原典: `documents/plans/user/20261013c.md` §11（横断シナリオ一覧）、§13（完了条件）。

対象: 公開 11 コマンド（`inspect` / `scan` / `organize` / `undo` / `classify` / `watch` / `setup` / `auth` / `config` / `doctor` / `help`）+ 隠し `discover`。サブコマンド（`auth` 3 種、`config` 2 種）にも適用する。

実測環境: HEAD `6819ac9`、バイナリはビルド済み `target/debug/kaseifu`。実測日 2026-10-13。終了コードは実測値、実測できない項目は「設計上の期待（未実測）」と明記する。

---

## C01 — `--help`

```text
scenario_id: C01
priority: P2
test_level: E2E

question: --help が実行可能なコマンド集合を正しく示すか

preconditions: なし（設定・ネットワーク非依存。Keychain を触らない）

inputs: --help / -h / <command> --help / help <command> / discover --help

expected:
  - "終了コード 0、出力は stdout（stderr 0 バイト）"
  - "トップレベルは 11 コマンド: inspect, scan, organize, undo, classify, watch, setup, auth, config, doctor, help"
  - "discover は --help の一覧に出ない（#[command(hide = true)]、src/main.rs:88）が、'discover --help' は exit 0 で usage を出す"
  - "'help undo' は exit 0 でサブコマンド usage を stdout に出す"
  - "グローバルオプション（--format / --jev-endpoint / --jev-model）が全サブコマンドで見える（clap global）"
  - "auth / config はサブコマンドを列挙する（set-key / status / remove-key、show / set）"

failure_conditions:
  - "ラ晝のコマンドが一覧にあるのに実行できない、または実行できるのに一覧に無い（discover は意図的例外）"
  - "補完追加（completions）後に一覧と補完候補が不一致になる"

observation_points:
  - "コマンド名列の集合（順序は clap の定義順）"
  - "exit code とストリーム"

evidence:
  - "src/main.rs:28-154（Cli / Commands / サブコマンド定義）、88（hide = true）"

existing_test: "なし（未カバー。tests/user-test-plan.csv T01/T04 が手動）"
status: DESIGNED
```

---

## C02 — `--version`

```text
scenario_id: C02
priority: P2
test_level: E2E

question: 版番号が 1 か所で正しく出るか

inputs: --version / -V / <command> --version

expected:
  - "'kaseifu 0.4.2' を stdout に出して終了コード 0（実測）"
  - "版はトップレベルのみ。'classify --version' は 'error: unexpected argument' で終了コード 2（実測）"
  - "版は Cargo.toml の version と一致（clap の箱）"

failure_conditions:
  - "版番号の不一致（Cargo.toml とリリース表記の差）"
  - "サブコマンドでも版が出るようになる（互換変更として記録が必要）"

observation_points:
  - "1 行目の文字列と exit code"

evidence:
  - "src/main.rs:29-30（command(name/version)）、Cargo.toml:3（version = 0.4.2）"

existing_test: "なし（未カバー。tests/user-test-plan.csv T02 が手動）"
status: DESIGNED
```

---

## C03 — 必須引数の欠落

```text
scenario_id: C03
priority: P1
test_level: E2E

question: 必須引数が無いとき、何が足りないか伝わるか

inputs: inspect / organize / scan / classify / discover / watch / config set / auth / 引数なしの kaseifu

expected:
  - "全て終了コード 2（clap の既定）、メッセージは stderr、stdout は 0 バイト（実測: inspect / config set / watch）"
  - "watch は '--path <PATHS>' を名指しする（required = true、src/main.rs:119-120）"
  - "auth はサブコマンド help を stderr に出す（実測: 700 バイト、exit 2）"
  - "引数なしの kaseifu は usage を出す（実測: exit 2）"

failure_conditions:
  - "必須引数欠落で終了コード 0 または 1"
  - "メッセージが stdout に出る（パイプ利用を壊す）"

observation_points:
  - "exit code と stderr の先頭行"

evidence:
  - "src/main.rs:44-154（引数定義）、186-220（dispatch 前に parse が失敗する）"

existing_test: "なし（未カバー。CSV T57 が手動）"
status: DESIGNED
```

---

## C04 — 未知の引数・オプション

```text
scenario_id: C04
priority: P1
test_level: E2E

question: 未知の指定が黙って無視されないか

inputs: foo（未知サブコマンド）/ classify --version / undo --last -1 / inspect --jsn（typo）/ organize <file> --execute --dry-run（相互排他）

expected:
  - "すべて終了コード 2。'error: unrecognized subcommand' / 'unexpected argument' / 'the argument --execute cannot be used with --dry-run'（実測: 後者は exit 2）"
  - "相互排他は clap の conflicts_with で表現される（src/main.rs:67,122）"
  - "typo は候補を提案しない場合もあるが、必ず非 0 で終わる"

failure_conditions:
  - "未知のオプションを無視して処理を続ける"
  - "相互排他が実行時に両方真になる（0 除算や誤った dry-run 解釈）"

observation_points:
  - "exit code と 'error:' 行"

evidence:
  - "src/main.rs:64-72,116-127（conflicts_with）、186-220"

existing_test: "なし（未カバー。CSV T03/T36 が手動）"
status: DESIGNED
```

---

## C05 — 不正な引数値

```text
scenario_id: C05
priority: P1
test_level: E2E

question: 値の型・列挙外の値が拒否されるか

inputs: --format xml / undo --last abc / undo --last 999999999999999999999 / --jev-endpoint 'not a url' / --jev-model '' / config set classifier.thresholds.auto_move_confidence 1.5

expected:
  - "clap 段: --format は [auto, rich, json] 以外を終了コード 2 で拒否（実測）"
  - "clap 段: --last の非数値は終了コード 2（実測）。usize の上限超過も同様（設計上の期待）"
  - "アプリ段: --jev-endpoint は 'invalid jev.endpoint URL' または scheme 検査で失敗（src/config/loader.rs:175-181）"
  - "アプリ段: config set の閾値は 0..=1 の範囲外を拒否（src/config/loader.rs:190-195）"
  - "--jev-model '' は 'invalid jev.model (must not be empty)'（src/config/loader.rs:183-188）"

failure_conditions:
  - "不正値が既定値に黙って置き換わる"
  - "不正値がそのまま保存される（TOML に書かれる）"

observation_points:
  - "エラー種別（clap の 'invalid value' とアプリの 'invalid ...' の別）"
  - "設定ファイルが変わらないこと"

evidence:
  - "src/config/loader.rs:149-195,308-315、src/main.rs:32-39,74-78"

existing_test: "tests/v33_config_auth_test.rs:280-300, 378-414（一部）"
status: DESIGNED
```

---

## C06 — 終了コード

```text
scenario_id: C06
priority: P0
test_level: E2E

question: 終了コードが「成功 / アプリエラー / 引数エラー」を区別できるか

preconditions: ハンドラや exit() の上書きが無いこと（grep: process::exit / ExitCode は 0 件）

inputs: 実測した 8 例（表）

expected:
  - "0 = コマンドの目的を果たした（診断の FAIL 行・undo の 'nothing left to undo' も含む）"
  - "1 = anyhow エラー（main が Result を返して終了）。例: 'cannot resolve', 'watch path is not a directory'"
  - "2 = clap の引数エラー（未知・不足・不正値・相互排他）"
  - "130 = SIGINT（設計上の期待。未実測）"

failure_conditions:
  - "引数エラーで 1 を返す（CI の分岐が壊れる）"
  - "FS 異常（移動失敗・衝突）で非 0 を返す（実装は 0 + 表示で通知）→ 現行仕様の明示が必要"
  - "成功なのに非 0を返す"

observation_points:
  - "各コマンドの exit code と最終行"

evidence:
  - "src/main.rs:185-220（main は Result を返す。clap が先行）"

existing_test: "なし（未カバー。テストはライブラリ呼び出しのみで CLI プロセスを起てない）"
status: DESIGNED
```

実測結果（2026-10-13）:

| 入力 | 終了コード | stdout | stderr | 備考 |
|---|---|---|---|---|
| `--version` | 0 | 'kaseifu 0.4.2' | 0 バイト | |
| `--help` | 0 | usage | 0 バイト | |
| `help undo` | 0 | usage | 0 バイト | |
| `discover --help` | 0 | usage | 0 バイト | 隠しコマンドでも help は出る |
| `foo`（未知） | 2 | 0 バイト | 'error: unrecognized subcommand' | |
| `classify --version` | 2 | 0 バイト | 'error: unexpected argument' | 版はトップレベルのみ |
| `organize <f> --execute --dry-run` | 2 | 0 バイト | 'cannot be used with' | 相互排他 |
| `watch`（--path なし） | 2 | 0 バイト | 'required arguments were not provided' | |
| `inspect`（引数なし） | 2 | 0 バイト | '<PATH>' 不足 | |
| `config set`（引数なし） | 2 | 0 バイト | '<KEY>' 不足 | |
| `auth`（サブコマンドなし） | 2 | 0 バイト | サブコマンド help | |
| `--format xml scan /tmp` | 2 | 0 バイト | 'invalid value' | |
| `undo --last abc` | 2 | 0 バイト | 'invalid digit found in string' | |
| `undo --last -1` | 2 | 0 バイト | 'unexpected argument' | |
| `watch --path <file>` | 1 | 0 バイト | 'Error: watch path is not a directory' | アプリ検証 |
| `inspect <存在しないパス>` | 1 | 0 バイト | 'Error: cannot resolve' | アプリ検証 |
| `undo --last 0` | 0 | （なし） | 0 バイト | ジャーナルを作成（CWD 相対） |
| `scan <存在しないパス>` | 0 | 'Scanning: ...' | 0 バイト | 検証しない（S21） |

---

## C07 — stdout / stderr の分離

```text
scenario_id: C07
priority: P1
test_level: E2E

question: 正常出力と診断・エラーが分離されているか

inputs: 正常系（inspect --format json / --help / --version）と異常系（存在しないパス）を stdout / stderr 別々に捕獲

expected:
  - "正常出力と help は stdout のみ（実測: stderr 0 バイト）"
  - "エラーは stderr のみ（実測: 'Error: cannot resolve ...' と Rust の backtrace 形式なしの 'Caused by:' 行）"
  - "--verbose の診断（Target / Candidates / Jev response）は stderr（src/main.rs:522-571）"
  - "JSON を出すのは inspect / discover / classify と emit_report 系（setup / auth / config / doctor）のみ。organize / undo / scan / watch は --format json でも人間向けテキスト（実測: scan）"

failure_conditions:
  - "エラーが stdout に混ざる（JSON パースを壊す）"
  - "自動化向け出力（JSON）に診断行が混入する"

observation_points:
  - "各ストリームのバイト数と先頭行"

evidence:
  - "src/main.rs:222-233,457-471,499-505,610-626（emit_report）、522-575（--verbose は eprintln!）"

existing_test: "なし（未カバー）"
status: DESIGNED
```

---

## C08 — Shell Tab 補完

```text
scenario_id: C08
priority: P2
test_level: E2E

question: 補完候補が実行可能コマンドと一致するか

preconditions: HEAD `6819ac9` に `completions` サブコマンドは存在しない（実測 --help + grep clap_complete 0 件）。並行タスクが追加中

inputs: 補完生成コマンド（追加後の名前）/ shell への source / Tab 入力（kaseifu <TAB> / kaseifu classify --<TAB>）

expected（設計上の期待。未実装のため現時点では必ず未達）:
  - "生成される候補集合 = --help の 11 コマンド + 隠し discover の扱いが生成側で決定されていること"
  - "サブコマンド（auth / config）とその値を補完できること"
  - "--format の値（auto / rich / json）を補完できること"
  - "権限のないディレクトリ・秘密情報を補完候補に出さないこと"

failure_conditions:
  - "補完候補と実行可能コマンドの不一致（追加後に --help と異なる）"
  - "補完生成が設定ファイル / Keychain を読む（副作用・プロンプト）"

observation_points:
  - "zsh / bash の補完関数の候補集合"
  - "completions の --help と exit code"

evidence:
  - "現行: src/main.rs:44-154 に completions が無い。grep 'completion|clap_complete' = 0 件"

existing_test: "なし（未カバー・未実装）"
status: DESIGNED（未実装領域）
```

---

## C09 / C10 — 日本語・Unicode / スペースを含むパス

```text
scenario_id: C09 / C10
priority: P1
test_level: E2E

question: 非 ASCII と空白を含むパスで引数が正しく渡るか

inputs:
  - "C10: inspect '~/Desktop/test/THCO-COA-Non-Detect Updated.pdf' --format json（実測: exit 0、1904 バイトの JSON）"
  - "C09: 日本語名ファイル（例: ~/Desktop/test/日本語ファイル.md）での inspect / classify / scan"
  - "両方: 移動・復元（S17 / S01 の手順を非 ASCII 名で実行）"

expected:
  - "終了コード 0。JSON の target.path / target.filename が入力とバイト一致（U+FFFD が現れない）"
  - "mdls / mdfind は Command の個別引数で呼ばれ shell を経由しないため、空白・非 ASCII で分割されない（実測: 空白入り PDF で成功）"
  - "検索語は空白で語分割され、非 ASCII 語は is_query_safe（Unicode alphanumeric 許可）で保持される"
  - "移動後のジャーナルにも同じバイト列のパスが記録される（to_string_lossy は UTF-8 正当パスで損失なし）"

failure_conditions:
  - "文字化け（U+FFFD）または引数分割による誤パス解決"
  - "NFD / NFC の取り違えで「存在するのに見つからない」"

observation_points:
  - "JSON の path 文字列と実ファイルのバイト比較"
  - "KASEIFU_DEBUG=1 の mdfind クエリ行"

evidence:
  - "src/context/builder.rs:10-27、src/platform/macos/spotlight.rs:52-65,119-131、src/discovery/similarity.rs:177-197"

existing_test: "src/normalize/tests/mod.rs の test_japanese_filename_normalization（一部。CSV T79/T80 が手動）"
status: DESIGNED
```

---

## C11 — Ctrl+C などの中断

```text
scenario_id: C11
priority: P1
test_level: E2E

question: 中断でデータと履歴が壊れないか、中断を検出できるか

preconditions: 中断ハンドラは実装されていない（grep: signal / ctrlc / SIGINT 0 件）

inputs:
  - "watch --path <dir> --execute を走らせ、アイドル中 / 処理中（Jev 待ち・rename 直前・rename 直後）に SIGINT"
  - "organize <file> --execute を Jev 待ちで中断"
  - "Keychain プロンプト待ちでブロックするコマンド（doctor / auth status / config show）を SIGINT"

expected:
  - "プロセスは即時終了（SIGINT 既定）。シェルから見た終了コードは 130（設計上の期待。128 + 2）"
  - "rename 前に中断: ファイルは無事、ジャーナル 0 件"
  - "rename 後・mark_success 前に中断: ファイル移動済み + pending 1 件（S05 の検出点）"
  - "Keychain プロンプト待ちの中断: 設定ファイル・ジャーナルは不変"
  - "いずれの場合も再実行後に「pending が残っている」ことを SQL で検出できる"

failure_conditions:
  - "中断後にファイルも履歴も残らない（未記録のミューテーション）"
  - "中断が無視されて処理が続行する（暗黙の遅延）"
  - "プロンプト待ちで無期限にブロックし、CI として終わらない（実測: doctor が 120 秒で完了せず）"

observation_points:
  - "終了コードと最終ログ行"
  - "ジャーナルの pending 行の有無（R1/R2）"
  - "プロセスが消えたこと（pgrep）"

evidence:
  - "src/main.rs:185-187（ハンドラなし）、src/watch/runner.rs:215-245（ループは SIGINT で中断される）"
  - "src/operations/mover.rs:93-99、src/storage/history.rs:93-109"

existing_test: "なし（未カバー）"
status: DESIGNED
```

---

## C12 — 秘密情報の非表示

```text
scenario_id: C12
priority: P0
test_level: E2E

question: どの出力経路でも秘密値が漏れないか

inputs:
  - "JEV_API_KEY を環境変数で与えて: classify --verbose / organize / doctor / config show / auth status"
  - "--verbose の stderr（候補、確率分布、ターゲット）"
  - "エラー時: 401 / 不正 JSON（レスポンス本文を 200 文字まで表示する）"

expected:
  - "API キーはどの出力にも現れない（ApiKey の Debug は ApiKey(REDACTED)、src/config/resolver.rs:30-34）"
  - "キーの状態は語彙のみ（Configured / Not configured / Unknown）で、値は出さない"
  - "--verbose は候補 ID / パス / 確率分布を出すが、キー・トークンは出さない（src/main.rs:546-567）"
  - "GPS 座標は stderr でも生値ではなく 'GPS available (masked)'（src/context/image_metadata.rs:374-380）"
  - "HTTP エラーの本文スニペットは先頭 200 文字（場合によってはプロバイダのメッセージを含み得るが、リクエスト側のキーは含まれない。src/classifier/jev.rs:609-611）"

failure_conditions:
  - "キーがログ・エラー・JSON に現れる"
  - "'Unavailable(<keyring error>).’ のようなエラーに値が混入する"
  - "GPS の生座標がログに出る"

observation_points:
  - "全出力に対する 'key-like' 文字列の検査（環境変数の値を grep し、ヒット 0 件を確認）"
  - "JSON の api_key_configured は真偽値のみ"

evidence:
  - "src/config/resolver.rs:14-34、src/auth/keychain.rs:19-34,94-100、src/doctor.rs:6（秘密値なしの宣言）、src/models/file_context.rs:88-105（location 型）"

existing_test: "tests/v33_config_auth_test.rs:207-236、tests/v32_image_metadata_test.rs:759-780（一部）"
status: DESIGNED
```

---

## カバレッジの要約（未カバー領域）

| 項目 | 現状 | 理由 |
|---|---|---|
| CLI プロセスレベルのテスト | **未カバー** | `tests/` はライブラリ呼び出しのみ（`assert_cmd` 依存なし）。C01–C07・C11–C12 の多くは未実施 |
| `--format json` の organize / undo / scan / watch | **実装なし** | 実測と `src/main.rs` の分岐で確認。C07 の未カバー項目 |
| Tab 補完（C08） | **未実装** | `completions` が存在しない。並行タスク待ち |
| Ctrl+C の各中断点（C11） | **未カバー** | ハンドラ無し。S05 と同じ検出点を使う |
| 動画（MP4 / MOV） | **シナリオ無し** | 画像判定の対象外（R7 / F-11）。イベント・大容量ファイルの観測が未設計 |
| Keychain プロンプト遷移（S23/S25/C11） | **未確認** | 実環境で `doctor` が 120 秒で完了しなかった（キー確認の可能性）。ハング・タイムアウトを観測項目に追加済み |
