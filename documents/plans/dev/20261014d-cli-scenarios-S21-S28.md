# CLI コマンド別シナリオ仕様 S21–S28（v1.2 §3–§10 の完全化）

原典: `documents/plans/user/20261013c.md` §2–§10（一覧とケース表）、§12（成果物）。基準文書は v1.1（`20261013b.md`）。

形式は v1.1 §5 のスキーマ + v1.2 §12 の「観測ポイント」+ 実装根拠。用語は `20261014d-scenarios-S01-S10.md` の冒頭と同じ。

実測環境: HEAD `6819ac9`（v0.4.2）、`--help` のコマンドは 11 種 + 隠し `discover`。終了コードの実測値は `20261014d-cli-scenarios-C01-C12.md` に集約。

---

## S21 — `scan`

```text
scenario_id: S21
title: "scan: argument and output contract"
priority: P1
test_level: E2E

question: "scan が対象を表明し、引数・出力・非破壊性の契約を守るか"

preconditions:
  - "作業ディレクトリにジャーナルが無い（scan が作らないことの確認のため）"

inputs:
  - "scan <dir> / scan <file> / scan <存在しないパス> / scan（引数なし）/ scan --format json / scan --format rich"

expected（実装済み仕様。v1.1 S21 の「走査」期待は下記の再定義による）:
  - "scan はファイル集合を列挙しない。非 rich では 'Scanning: <path>' を stdout に出して終了コード 0（src/main.rs:278）"
  - "rich では 'KASEIFU / SCAN target: <path>' の 1 行（src/main.rs:271-276）"
  - "存在しないパス・ファイルを指定してもエラーにならない（実測: いずれも終了コード 0）"
  - "--format json でも JSON を出さない（実測: 'Scanning: /tmp' が stdout）"
  - "FS / ジャーナルへの書き込みなし（handle_scan は println! のみ。src/main.rs:268-280）"
  - "引数なしは clap が必須引数不足として終了コード 2、stderr に usage"

failure_conditions:
  - "scan がファイルを列挙し、列挙内容が実 FS と一致しない（再定義前の期待で書いた場合の誤検知）"
  - "存在しないパスで終了コード 1 を期待する（実装は 0）→ 期待が実装と不一致"
  - "--format json を期待する（未対応）"

observation_points:
  - "stdout の 1 行目と終了コード"
  - "ジャーナルファイルが作られないこと"
  - "走査相当の結果が必要なら inspect / discover / classify を使う（責務の分界点）"

evidence:
  - "src/main.rs:268-280（handle_scan = target banner only）、191-195（分岐）"
  - "src/main.rs:269（doc コメント 'no mutation, no discovery'）"
  - "Cargo.toml の walkdir は src/ で未使用（grep 0 件）= 再帰走査の実装が存在しない"

existing_test: "なし（未カバー。tests/user-test-plan.csv にも scan の行が無い）"
status: DESIGNED
```

ケース表（v1.2 §3 の再定義）:

| ケース | 入力 | 期待（現行契約） | 実装根拠 |
|---|---|---|---|
| S21-01 | 通常のディレクトリ | 'Scanning: <dir>' 1 行、終了 0 | `src/main.rs:269-280` |
| S21-02 | 空ディレクトリ | 同上（内容に依存しない） | 同上 |
| S21-03 | 存在しないパス | **エラーにならない（終了 0）**。走査を期待するなら S26/S27 へ | 実測 + `src/main.rs:269` |
| S21-04 | ファイルを指定 | 同上（型チェックなし） | 実測 |
| S21-05 | ネストしたディレクトリ | 再帰の概念が無い（`--recursive` 相当のオプションも存在しない） | `src/main.rs:57-58` |
| S21-06 | 隠しファイル | 列挙しないため除外規則も無い。隠しの扱いは S26（`read_directory`） | `src/platform/macos/filesystem.rs:7-33` |
| S21-07 | シンボリックリンク | 追跡しない（列挙なし） | 同上 |
| S21-08 | 権限不足 | 読まないのでエラーにならない（終了 0） | 実測 |
| S21-09 | 大量ファイル | 件数に依存しない（O(1) 出力） | `src/main.rs:268-280` |
| S21-10 | 走査中の削除 | 走査しないため競合しない | 同上 |

設計上の指摘 F-11: 原典 S21 の「走査」は現行の `scan` には存在しない。`scan` を本来の走査コマンドに育てる案は§8の設計提案側であり、本シナリオは現行契約を反証可能な形で固定する。

---

## S22 — `setup`

```text
scenario_id: S22
title: "setup: wizard, persistence order, interrupt"
priority: P1
test_level: E2E

question: "雛形設定の入力・保存・中断・再実行が正しく扱われるか"

preconditions:
  - "ユーザー TOML のパス: ~/Library/Application Support/kaseifu/config.toml（src/config/storage.rs:15-17）"
  - "Keychain サービス: com.routeflags.kaseifu.jev / account: default（src/auth/keychain.rs:11-14）"
  - "テストは一時 HOME またはテスト用サービスで行う（本番エントリを壊さない）"

inputs:
  - "setup --check-connection（あり / なし）"
  - "入力: 空 Enter（現在値採用）/ 不正 URL / 空モデル / 空キー / Ctrl+C"

expected:
  - "入力順は endpoint -> model -> API key（src/main.rs:696-705）"
  - "保存順は TOML 先、Keychain 後（src/main.rs:742-753）。Keychain 失敗時は TOML が更新済みの部分状態で終了する"
  - "空 Enter は現在値を維持（or_current）、空モデル・空キーはエラー（src/main.rs:713-740）"
  - "--check-connection 無しではネットワークを触らない（doctor.rs:143-151）、ありの場合は Reachable / Unreachable を 1 行で表示"
  - "終了後に `config show` で同じ effective 値が読める（CLI > env > TOML の優先順位で env が勝つ場合がある点は S24-09 に従う）"

failure_conditions:
  - "API キーが画面・ログ・TOML に残る（TOML には絶対に書かれない。loader のキー集合にキー項目が無い）"
  - "中断で TOML が壊れる（非アトミック書き込み）。実装は temp + rename なので、これは検出されたらバグ（src/config/loader.rs:207-219）"
  - "Keychain 失敗を「成功」として完了表示する"

observation_points:
  - "TOML のキー集合（jev.endpoint / jev.model のみが増える）"
  - "Keychain のエントリ有無（`auth status` は Keychain プロンプトでブロックし得る: C11 参照）"
  - "setup の終了後の doctor サマリ行（setup complete / complete with warnings）"

evidence:
  - "src/main.rs:643-705（handle_setup と入力）、742-753（apply_setup_inputs）、755-789（サマリ）"
  - "src/auth/prompt.rs（prompt_line / prompt_secret / confirm の境界）"
  - "src/config/storage.rs:12-22、src/config/loader.rs:200-228（アトミック保存）"

existing_test: "tests/v33_config_auth_test.rs:301-341（loader の保存/検証のみ。一部）"
status: DESIGNED
```

更新順序の帰結（設計上の指摘 F-10）: Keychain 失敗時は「TOML は新、Keychain は旧」になる。検出は `config show` と `auth status` の並置で行う。逆順（Keychain 先、TOML 後）に変えるのは設計提案側。

---

## S23 — `auth`

```text
scenario_id: S23
title: "auth: key lifecycle"
priority: P1
test_level: E2E

question: "認証情報の登録・確認・削除が安全に行われるか"

preconditions:
  - "テスト用サービス ID または MemoryStore を使用（本番エントリを破壊しない）"

inputs:
  - "auth status（未登録 / 登録済み / env 提供）"
  - "auth set-key（新規 / 既存ありで置換 / 確認拒否 / 空文字 / Ctrl+C）"
  - "auth remove-key（--yes あり / なしで確認拒否 / 未登録時）"

expected:
  - "未登録: 'API Key: Not configured' + 'Storage: -'、JSON は api_key_configured=false（src/main.rs:874-883）"
  - "env 由来: 'Configured' + 'Storage: Environment'（Keychain を触らずに判定）"
  - "Keychain 由来: 'Configured' + 'Storage: macOS Keychain'"
  - "set-key: 既存ありなら confirm('Replace the stored API key?', false) が先。拒否時は 'API key unchanged' で exit 0（src/main.rs:822-844）"
  - "空キーは 'API key must not be empty' で終了コード 1（src/main.rs:834-837）"
  - "remove-key: 未登録なら removed=false, reason=not-found で exit 0（src/main.rs:885-896）"
  - "いずれの出力にもキー値が出ない（ApiKey の Debug は ApiKey(REDACTED)）"

failure_conditions:
  - "キー値が表示される"
  - "確認拒否でキーが変わる"
  - "未登録の削除でエラー終了する（実装は正常終了）"

observation_points:
  - "出力の語彙（Configured / Not configured / Unknown）と Storage 列"
  - "Keychain エラー時の表示: 'Unknown' + エラー文字列（src/main.rs:878-882）"
  - "注意: Keychain がアクセス確認を出す環境では status がブロックし得る（実測: doctor が 120 秒で完了せず → 観測手順にタイムアウトを入れる）"

evidence:
  - "src/main.rs:807-924（auth 3 サブコマンド）"
  - "src/auth/keychain.rs:19-34,72-100（エラーに値を含まない）、102-138（MemoryStore）"
  - "src/config/resolver.rs:228-245（env 優先、Keychain 失敗はキーなし扱い）"

existing_test: "tests/v33_config_auth_test.rs:415-472, 207-236（一部）"
status: DESIGNED
```

---

## S24 — `config`

```text
scenario_id: S24
title: "config: values, validation and precedence"
priority: P1
test_level: E2E

question: "設定値の表示・変更・解決が一貫しているか"

preconditions:
  - "テスト用 HOME（本番 TOML を壊さないため）"

inputs:
  - "config show / config set <key> <value>（正常・未知キー・型不一致・範囲外・不正 URL・API キー）"
  - "優先順位: CLI --jev-endpoint / 環境変数 / .env / TOML / 既定"

expected:
  - "show は [jev] / [classifier.thresholds] / [operations] / [metadata.*] と 'API key: ...' を並べ、JSON でも同値を返す（src/main.rs:936-990）"
  - "set は未知キーを拒否（supported 一覧を添える）、API キー系キーは 'refused: ... use auth set-key'（src/config/loader.rs:332-352）"
  - "閾値は 0.0..=1.0 の範囲外を拒否（src/config/loader.rs:190-195）"
  - "endpoint は http/https 以外を拒否（src/config/loader.rs:175-181）"
  - "保存は validate 後に temp + rename（失敗時は旧値を保持。src/config/loader.rs:200-219）"
  - "優先順位: CLI > env（.env は env の代替）> TOML > 既定。キーは env > Keychain（src/config/resolver.rs:191-245,252-269）"
  - "注意（未接続）: operations.dry_run / operations.enable_undo / metadata.weather は解決されるが実行挙動に未接続（main.rs は CLI フラグのみ使用）。`[storage]` はユーザー TOML では未知キー扱いで拒否される"

failure_conditions:
  - "未知キー・不正値が保存される"
  - "優先順位の逆転（TOML が env / CLI に勝つ）"
  - "失敗時に既存ファイルが壊れる"
  - "dry_run=false を設定したのに organize が dry-run のまま（実装の未接続。設計上の指摘 F-12）"
observation_points:
  - "config show の行と JSON のキー集合"
  - "set のエラーメッセージ種別（unknown / invalid / refused）"
  - "ファイルの mtime と内容（失敗時に不変であること）"

evidence:
  - "src/config/loader.rs:238-330（SUPPORTED_KEYS、各 set 関数）、149-195（validate）"
  - "src/config/resolver.rs:115-153,191-269"
  - "src/main.rs:929-1000（config show / set）"

existing_test: "tests/v33_config_auth_test.rs:59-206, 237-414（一部: 実 CLI 未カバー）"
status: DESIGNED
```

---

## S25 — `doctor`

```text
scenario_id: S25
title: "doctor: environment diagnosis"
priority: P1
test_level: E2E

question: "環境の障害原因を識別できるか、副作用なく診断できるか"

preconditions:
  - "読み取り専用の確認のみ。--check-connection はネットワークを使う"

inputs:
  - "doctor（ローカルのみ）/ doctor --check-connection"
  - "ケース: 正常環境 / 設定ファイルなし / 設定破損 / キー未登録 / 不正 endpoint / 接続不可"

expected:
  - "チェックは常に 6 種: Platform / Configuration / Keychain / Jev Endpoint / Spotlight / Storage（src/doctor.rs:51-58）"
  - "行形式は '<ok|FAIL> <name>: <detail>'。結果は常に終了コード 0（fail しても異常終了しない。src/main.rs:1018-1038）"
  - "--check-connection なしではネットワークを触らない（Connectivity::NotTested = 'Jev connectivity: Not tested'）"
  - "--check-connection あり: 'Reachable' または 'Unreachable (<detail>)'（src/doctor.rs:143-168）"
  - "副作用: Storage チェックが app ディレクトリを作成する（create_dir_all）。それ以外の書き込みはない（src/doctor.rs:128-134）"
  - "注意: Keychain がアクセス確認を出す環境では、このコマンドがブロックし得る（実測: 本環境の doctor は 120 秒で完了せず。C11 の中断対象）"

failure_conditions:
  - "秘密値・実パスが診断行に出る"
  - "ネットワーク検査をフラグ無しで実行する（実装はしない。したらバグ）"
  - "キー確認のダイアログで無期限ブロックする（観測はタイムアウトを設定し、ハングを「検出」として記録）"

observation_points:
  - "診断行の集合と ok/FAIL の根拠（各 check の detail 文字列）"
  - "connectivity の 3 語彙（Not tested / Reachable / Unreachable）"
  - "実行前後の app ディレクトリ有無"

evidence:
  - "src/doctor.rs:46-65（diagnose の 6 チェック）、83-134、143-168"
  - "src/main.rs:1004-1038（doctor の表示）"
  - "src/config/storage.rs:19-29（app_dir）"

existing_test: "tests/v33_config_auth_test.rs:473-527（3 件。一部）"
status: DESIGNED
```

---

## S26 — `inspect`

```text
scenario_id: S26
title: "inspect: FileContext CLI contract"
priority: P1
test_level: E2E

question: "取得した FileContext が実ファイルの状態を正しく反映しているか"

preconditions:
  - "対象: 通常ファイル / PDF / JPEG / MP4 / ディレクトリ / symlink / 日本語名 / 破損画像"

inputs:
  - "inspect <path> / inspect --all <path> / inspect --verbose <path> / --format json|rich|auto"

expected:
  - "JSON のトップレベルキーは schema_version / target / directory_context / evidence（実測: schema_version = 'file-context-v1'）"
  - "非 rich 出力は JSON、rich 出力は ASCII パネル（枠線は +--/| ）"
  - "--all で nearby エントリ（既定 5）と evidence（既定 10）の制限が外れる（inspect.rs:17-21）"
  - "--verbose で DETAILS 層（完全パス、schema、生メタデータ）が加わる"
  - "ディレクトリを対象にすると directory_context.current がそのディレクトリになり、entries は最大 100 件（builder.rs:45 → filesystem.rs:7-13）"
  - "存在しないパスは 'Error: cannot resolve <path>' を stderr に出し終了コード 1（実測）、stdout は空"
  - "Spotlight 未索引 / xattr 無しでも evidence が空のまま正常終了する（mdls 失敗は空ベクタ。spotlight.rs:61-64）"
  - "MP4 は画像メタデータを付けず、evidence は Spotlight / xattr のみ（image_metadata.rs:52-60）"

failure_conditions:
  - "JSON キーの消失・型変更（schema_version 不一致）"
  - "存在しないパスで終了コード 0"
  - "entries が 100 件を超える（上限の破れ）"
  - "mdls 失敗で異常終了する"

observation_points:
  - "JSON のキー集合と値の型、path / filename のバイト一致"
  - "entries / evidence の件数（非 --all と --all の差分）"
  - "stderr の有無（正常時に何も出さない）"

evidence:
  - "src/main.rs:222-254（handle_inspect）、src/context/builder.rs:10-72"
  - "src/presentation/renderers/inspect.rs:17-21,325"
  - "src/platform/macos/spotlight.rs:33-65、src/platform/macos/xattr.rs"

existing_test: "tests/v31_render_test.rs:261-299 + tests/v32_image_metadata_test.rs:831-924（一部）"
status: DESIGNED
```

重複の整理（v1.2 §8 の指示）: S26-05（Spotlight 未索引）、S26-08（日本語名）、S26-09（破損画像）はそれぞれ S07 / S17 / S07 と共通ケース。重複実装を避けるため、観測は S07 / S17 の手順を再実行し、S26 は**CLI 契約（JSON・終了コード・件数上限）**に限定して判定する。

---

## S27 — `discover`（隠し・デバッグ専用）

```text
scenario_id: S27
title: "discover: candidate list contract"
priority: P1
test_level: E2E

question: "候補探索の結果が妥当で、禁止・重複・無関係な候補を適切に扱えるか"

preconditions:
  - "Spotlight が索引済みの実ファイル（または注入 search のライブラリ経路）"
  - "discover は `#[command(hide = true)]`（--help に出ないが実行可能。src/main.rs:80-95）"

inputs:
  - "discover <file> / discover --all / discover --verbose / --format json"

expected:
  - "JSON は CandidateDestination の配列（id / path / score / evidence_count / reasons）"
  - "ID は D1.. の連番で、スコア降順（同点は古い順、次にパス順。candidates.rs:89-113）"
  - "上限 50 件はランキング後に適用（mod.rs:39,110-117）"
  - "除外対象（システムパス / ライブラリ / キャッシュ / 隠し / 機械生成名）と自身の親は候補に現れない（S14）"
  - "候補 0 件は空配列 [] で終了コード 0"
  - "存在しないファイルは 'Error: cannot resolve <path>' で終了コード 1"
  - "同一入力の再実行で順位・スコアが同一（年齢ボーナスの浮動小数末桁を除く）"

failure_conditions:
  - "除外対象・自身の親が候補に現れる"
  - "50 件を超える返却、またはランキング前の切り捨て"
  - "同一入力で順位が非決定的に変わる"

observation_points:
  - "候補件数と D1..D5 の (path, score, reasons)"
  - "KASEIFU_DEBUG=1 の 'mdfind query' / 'Spotlight raw results' / 'Candidate destinations' 行"
  - "--format json でパースできること"

evidence:
  - "src/main.rs:457-471（handle_discover）、80-95（hide）"
  - "src/discovery/mod.rs:50-129、candidates.rs:89-116、similarity.rs:144-197（クエリ）"

existing_test: "tests/v042_discovery_target_scope_test.rs（8件）、tests/v042c_destination_exclusion_test.rs（9件）、tests/discovery_scoring_test.rs（7件）（一部）"
status: DESIGNED
```

---

## S28 — `watch`（P0）

```text
scenario_id: S28
title: "watch: start, stop and side effects"
priority: P0
test_level: E2E

question: "監視が正しく起動・停止し、イベントを安全に処理できるか"

preconditions:
  - "使い捨ての監視ディレクトリと Mock/実 Jev（副作用を抑える）"
  - "既定 WatchConfig（500ms / 1s / 10s / managed_paths = ジャーナル DB）"

inputs:
  - "watch --path <dir>（1 つ / 複数）/ watch --path <file> / watch（--path なし）"
  - "watch --path <非存在> / watch --path <dir> --execute / watch --path <dir> --dry-run"
  - "中断: Ctrl+C（処理中 / アイドル中）"
  - "イベント: 作成 / 連続書き込み / 移動後の自己イベント / 削除"

expected:
  - "起動時: 'watching <roots> (dry-run: true|false)' を stdout に 1 行（main.rs:402-417）"
  - "--path なしは clap が終了コード 2（実測）、--path <file> は 'Error: watch path is not a directory: <path>' で終了コード 1（実測。main.rs:373-377）"
  - "複数 --path は再帰監視（fsevents::spawn が RecursiveMode::Recursive。fsevents.rs:23-25）"
  - "--dry-run（既定）: 移動なし。イベントごとに 'dry-run <path> (would move to <dir>)' または 'review <path>' を出す（runner.rs:139-177）"
  - "--execute: AutoMove のみ移動し、'moved <src> -> <dst>' と operation id を出す（runner.rs:144-148）"
  - "重複・安定性・自己イベントの抑制は S06 / S28-11 と同じ経路（begin_processing -> record_self_operation -> finish）"
  - "1 ファイルの失敗は 'error <path>' を出してループ継続（runner.rs:176, 183-246）"
  - "Ctrl+C: 専用ハンドラが無く SIGINT で即時終了。終了コードはシェル慣例で 130。"
  - "整合性: 処理中に中断しても、rename 済みで mark_success 未達なら pending が残る（= S05 / R1 と同じ検出点）"

failure_conditions:
  - "監視プロセスが落ちる / 無限再起動ループ"
  - "dry-run でファイルが動く"
  - "同一イベントの二重処理"
  - "1 ファイルの失敗でループが停止する"
  - "中断後に FS とジャーナルの不一致を検出できない"

observation_points:
  - "ログ行の語彙（watching / ignored / detected / moved / dry-run / review / no move / error）"
  - "ジャーナル: 移動ごとの pending -> success 行"
  - "中断直後の pending の有無"

evidence:
  - "src/main.rs:363-417（handle_watch）、src/watch/runner.rs:183-246（run_loop_with）"
  - "src/watch/coordinator.rs:32-43（既定値）、src/platform/macos/fsevents.rs:14-27"
  - "src/operations/organize.rs:88-135（1 リクエストの処理経路）"

existing_test: "tests/phase6_watch_test.rs（11 件。一部: 実 CLI・実 FSEvents・Ctrl+C 未カバー）"
status: DESIGNED
```

ケース表の優先度: S28-01..07 = P0、S28-08..10 = P0（中断と整合性）、S28-11..12 = P0（S06 と重複、ただし CLI 経路での確認）。S28 は原典 v1.2 §2 で唯一の P0。
