# AIリネーム Phase 7（Directory Rename）実装計画

## 0. 位置づけと進め方

- 原典（読み取り専用・編集禁止）: `documents/plans/user/20261014a.md`（AIリネーム開発指示書 v2.0）§5.4・§6・§8.4・§9・§10・§11.2・§11.3・§12・§13.5・§14 Phase 7・§15（#3 / #14）。
- 前提: Phase 6 は着地済み（HEAD `4d94c40` = 作業ツリーの起点）。計画は `documents/plans/dev/20261014l-ai-rename-p6.md`。本ラウンドはフォルダを「子の内容から名前を作る対象」として扱い、文書・画像・動画の既存経路を一切変えない。
- 進め方: BDD タスク分解 -> plan-architect レビュー（issue 記録）-> implementer -> reviewer -> QA -> 検収。commit / push / tag はしない（親スレッドが実施）。
- スキル運用の注記: 本環境のスキルカタログに `/researcher` `/plan-architect` `/implementer` `/reviewer` `/qa` が無いため（Phase 3+4・5・5b・6 と同一条件）、各工程の役割定義（`.github/agents/{researcher,plan-architect,implementer,reviewer,qa}.agent.md`）の手順を直接なぞって実施し、根拠を本計画書と issues に残す。
- ツール利用実績: `read_file` で `src/rename/{service,document,image,video,rules,validate,store,mod}.rs` / `src/semantic/{contract,provider,fake,bridge}.rs` / `src/context/{builder,mod,image_metadata}.rs` / `src/models/file_context.rs` / `src/normalize/{mod,noise,metadata}.rs` / `src/discovery/destinations.rs` / `src/operations/{undo,mover}.rs` / `src/storage/history.rs` / `src/platform/macos/filesystem.rs` / `src/main.rs` / `swift/kaseifu_fm_bridge.swift` / `tests/v095_ai_rename_p6_video_test.rs` / `tests/v070_zero_config_p1_help_test.rs` / `scripts/quality_check.sh` / `README.md` を確認、`terminal` でベースライン再実測（729 passed）・現行バイナリの `rename <dir>` 実挙動・合成フォルダの `mdls`/`stat`・`$TMPDIR` の実パス・git 状態、`grep` で層依存（`crate::discovery` の呼び出し元、`src/discovery` の import）・`FileSnapshot`/`RenamePlan`/`NameProblem` の使用箇所・`is_noise_file`/`DirectorySummary` の使用箇所を実測。LSP / serena / context7 / codebase-memory-mcp / graphify は本環境に無いため read_file + grep + terminal で代替した。

## 1. ベースライン（実測・2026-10-15）

- HEAD: `4d94c40`（`feat: AI rename phase 6 video rename through AVFoundation and Vision`）。`git status --short` は `.gitignore` の 1 ファイルのみ（末尾改行の差分。本ラウンドでは触らない）。
- `cargo test --all-targets --all-features` = **729 passed / 0 failed / 4 ignored**（exit 0、ログ `/tmp/kb-p7/baseline-test.log`）。
- 環境実測: macOS 26.4.1（Build 25E253）、Swift 6.3.3、Apple Intelligence 有効、`rust-code-analysis-cli` 導入済み。`$TMPDIR` = `/var/folders/7t/xt8rcl791xzb8qnjnrhbx1tr0000gn/T/...`（**この事実が D8 の判断根拠**）。
- テスト版数接頭辞: `tests/` は `v090`〜`v095` が使用済み。新規結合テストは **`v096`**。

### 1.1 現行 `rename <dir>` の実挙動（実測）

```text
$ kaseifu rename "tree/新しいフォルダ"
Error: tree/新しいフォルダ has no extension; rename needs txt, md, pdf or html   (exit 1)
```

- 合成フォルダ（`invoice_001.txt` / `invoice_002.txt` / `contract.txt` / `payment.txt`）でも同じ。**提案は出ず、モデルも呼ばれない**（`src/rename/document.rs:64-67` の extension 無し分岐、`src/rename/document.rs:64-67` の文言は画像・動画が既に対象の今は虚偽でもある）。
- `rename`（引数なし）の guidance は 4 行（usage / documents / images / videos）でフォルダに触れていない（実測）。

### 1.2 フォルダの観測で使える実信号（合成フォルダで実測）

| 信号 | 実測 |
|---|---|
| Spotlight | `mdls <dir>` は `kMDItemFSCreationDate = 2026-10-14 15:22:56 +0000` / `kMDItemContentType = "public.folder"` を返す（= フォルダにも作成日時がある。`date_source: created` が honest に成立する） |
| ファイルシステム | `stat` で `birth` / `mtime` / `ctime` が取れる（`created` / `modified` の控え） |
| 子の内容 | 子ファイルは既存の文書リーダ（32 KiB / `T:` 12 件）でそのまま読める（`read_bounded` + `extract_signals` はパスだけを取る） |
| 子の一覧 | `crate::platform::macos::filesystem::read_directory(path, max_entries)` が `DirectoryEntry { name, kind }` を返す（`EntryKind` は File/Directory/Symlink/Other。**symlink は辿らない**） |

## 2. 利用する土台の実測再確認（file:line）

| 土台 | 実測 | Phase 7 での使い方 |
|---|---|---|
| `src/semantic/contract.rs:41-61` | `TargetKind::Directory` は **Phase 0 から存在**（`as_str` も `directory` を返す） | 契約の追加は `input.directory` のみ。`target_type` の分岐を配線する |
| `src/semantic/contract.rs:83-102` | `FmInput`（`image` / `perception` / `video` を skip 直列化） | `directory: Option<FmDirectoryFacts>` を**追加のみ**で足す（既存キー削除・型変更なし） |
| `src/semantic/provider.rs:230-274` | `build_request` と `target_kind`（画像・動画以外はすべて `Document`） | `is_dir()` を 1 分岐だけ足して `TargetKind::Directory` を返す。`directory_facts` 写像を同じ流儀で追加 |
| `swift/kaseifu_fm_bridge.swift:473-568` | `kfBuildPrompt`（`filename:` を無条件に出す、画像・動画の gate は `target_type` 比較） | `target_type == "directory"` の分岐だけを足す。文書・画像・動画のプロンプトは**バイト不変**（既存 v093/v095 の byte テストで固定） |
| `src/rename/service.rs:250-266` | `plan_rename` の dispatch（symlink -> image -> video -> document） | 先頭に「symlink -> **directory** -> image -> video -> document」を足す（`foo.png` という名前の**フォルダ**はフォルダとして扱う = ファイルシステム種別が名前より優先） |
| `src/rename/service.rs:693-768` | `finish_plan`（`{stem}.{ext}` を組み立て、`check_filename` で拡張子不変を検査） | フォルダは拡張子が無いので**専用の `finish_directory_plan`** を足す（`check_directory_name` + 拡張子の付与なし）。共通の後段（`assemble_plan`）は再利用 |
| `src/rename/validate.rs:53-104,124-170` | `check_filename` は拡張子一致を要求、`snapshot`/`check_source_stable` は `metadata.is_file()` を要求（ディレクトリは `SourceNotAFile`） | 名前検査を `check_name_shape`（空・`.`/`..`・区切り・長さ）へ分解し、`check_directory_name` を追加。ディレクトリは **(dev, ino) の同一性**で TOCTOU 判定する `DirectorySnapshot` + `recheck_directory_before_execute` を追加 |
| `src/rename/document.rs:54-80,83-106,163-202` | `classify_document` / `read_bounded`（32 KiB）/ `extract_signals`（`T:` <= 12）/ `contains_prompt_injection` | 子の内容はこの 3 つをそのまま再利用（子 1 件あたりの上限は既存のまま）。extension 無しの文言だけ修正 |
| `src/normalize/noise.rs:4-68` | `DirectorySummary { total_entries, file_count, directory_count, counts_map }` と `DirectorySummary::new(&[DirectoryEntry])`、`is_noise_file`、`noise_filter`。**`noise.rs` の外に呼び出し元が無い**（実測: `is_noise_file`/`noise_filter`/`directory_summary` の参照は `noise.rs` 内のみ。`src/benchmark/scatter.rs:161` は同名の別関数） | 子の集計はこの型を**再利用**する（第三の summary を作らない）。除外判定もこの `is_noise_file` を再利用 |
| `src/models/file_context.rs:67-85,229-233` | `NormalizedFileContext` と `DirectorySummary { parent_directory, child_count }`（正規化が **親ディレクトリ**の件数を入れる。`src/normalize/mod.rs:79-82`） | 既存型の意味は変えない。`directory: Option<DirectoryFacts>` を追加（add-only） |
| `src/context/builder.rs:38-51` | `read_directory(&current, 100)` は **target の親**を読む（フォルダ target では兄弟であって子ではない） | 子は新モジュールで読む（親の観測はそのまま残す = `{parent}` と既存挙動が不変） |
| `src/platform/macos/filesystem.rs:7-33` | `read_directory(path, max_entries)`（`take(max_entries)`、`file_type()` は symlink を辿らない） | そのまま再利用（上限だけ 200 を渡す） |
| `src/discovery/destinations.rs:79-91` | `is_excluded_destination` は「destination 適格性の単一権威」。内訳は bundle / macOS Library / system root / hidden / cache / system area / generated name の 7 規則 | 規則のうち **bundle・Library・system area・cache/temp・hidden の 5 つを `src/context/directory.rs` へ移し**、`destinations` と `rename` の両方が使う（1 規則 1 実装） |
| `src/discovery/*.rs` の import | `src/discovery/` は `crate::models` と `crate::discovery` しか import しない（`crate::context` 参照 0）。逆に `crate::discovery::is_excluded_destination` は `src/operations/plan.rs:109`・`src/benchmark/{gate,cli}.rs` から既に呼ばれている | 依存の向きは context を最下層に置くのが正しい（rename -> discovery の新規依存を作らない = Phase 6 の predicate 移動と同じ構図） |
| `src/operations/undo.rs:38-110` | `undo_last_move` は「最後の成功・未 undo の move」を取り、`moved_file.is_file()` を要求（**ディレクトリは `SourceNotAFile` で undo 不可**）、`original_path.exists()` なら `Collision`、成功時に `mark_success` + `mark_undone` | ディレクトリを受け入れる 1 分岐を足すだけ。二重 undo 防止と上書き禁止は既存のまま |
| `src/storage/history.rs:110-181,193-211` | `record_pending` は mutation の**前**、`mark_success`/`mark_failure` が後。`last_successful_move` は `operation='move' AND result='success' AND undone_at IS NULL` を rowid 降順で 1 件 | フォルダの rename も同じ `move` 行（新しい操作種別を作らない）。undo の順序が「新しい順」であることが D7 の根拠 |
| `src/main.rs:1240-1249,1189-1233` | `reject_foreign_rename_target`（画像・動画以外は `classify_document` で拒否）、`emit_rename_report`（`video:`/`perception:` 行と `--format json` の 10 キー） | フォルダを受理し、`directory:` 行と JSON の `directory` を追加（add-only） |
| `tests/v070_zero_config_p1_help_test.rs:168-227` | 全 help 面を自動走査（禁止語 = `Phase` などの内部語、ASCII のみ） | `rename --help` の文言追加はこの走査を通す（内部 Phase 語を出さない） |
| `scripts/quality_check.sh:57-120` | fmt / clippy / test / complexity / baseline-gate / self-test / coverage | 変更しない（ゲートとして使う） |

## 3. スコープ

### 3.1 対象（今回実装する）

- **フォルダ解析（§5.4）**: 子ファイル名・子フォルダ名・ファイル種別の分布・代表ファイルの内容・ディレクトリ構造（1 階層）・親フォルダ名を、上限付きで集める。
- **フォルダ命名**: 子の内容から `{title}` / `{category}` を得て、既存の命名規則（P01-P12・テンプレート・`--preset`）をそのまま適用して、拡張子を付けないフォルダ名を提案する。
- **空フォルダ / 内容が混在**: 提案を保留（`status: held` + 理由）。内容ベースの命名を無理に作らない。
- **安全性**: 危険な対象（システム根・bundle・Library・cache/temp・隠し・cwd とその祖先など）を実行前に拒否。TOCTOU 再確認（ディレクトリ版）。
- **Journal / Undo / 親子整合（§10・§13.5 DIR07/DIR08・§15 #14）**: フォルダの rename を `move` 行として記録し、`undo` で戻せるようにする。親フォルダのリネーム後も子要素の履歴が解決することを不変条件として固定し、解決できない場合は正直に拒否する。
- **表示と文言**: `rename <dir>` の dry-run / `--execute` / `undo` / `--format json`（追加のみ）、`--help` と guidance、README、extension 無しの虚偽メッセージの修正。
- テスト: `tests/v096_ai_rename_p7_directory_test.rs`（合成ツリーのみ・隔離 HOME・一時 dir）と、触ったモジュールの単体テスト。
- 実機検証: 合成フォルダで dry-run / `--execute` / `undo` を実行し、**フォルダ自身の名前ではなく子の内容に追従すること**を before/after で示す。

### 3.2 対象外（着手しない）

- Phase 8（一括・再帰）、Phase 9（性能・品質評価）、OCR（`VNRecognizeTextRequest`）、TREC ベンチマーク、`--to <dir>`（ZC-14）。
- 文書・画像・動画の既存挙動の変更（プロンプトのバイト、`--format json` の既存キー、既存テストの assert）。
- 無制限の再帰解析（§5.4 の禁止事項）。孫の内容抽出。
- 新規 Rust crate、新規設定ファイル、別 DB、独自キャッシュ、外部プロセス。
- フォルダ内の子を「まとめて一括リネーム」すること（Phase 8）。
- フォルダ自身の `xattr` / Spotlight 属性を根拠にした命名（今回は子の観測のみ）。

## 4. 設計判断（選択肢と結論。黙って決めない 10 点）

### D1. フォルダ事実の置き場（`NormalizedFileContext` 拡張 vs 新リーダモジュール）

- 選択肢A（採用）: **新モジュール `src/context/directory.rs`（上限付きリーダ + パス形状規則）** と、`NormalizedFileContext.directory: Option<DirectoryFacts>`（`skip_serializing_if` / `default`）の 1 フィールド追加。集計の型は既存の **`normalize::noise::DirectorySummary`** をそのまま使う（`DirectoryFacts` は summary ではなく「summary + 上限付きサンプル」の入れ物）。
  - 根拠: 事実は `build_request` が見る `NormalizedFileContext` に乗らないと契約へ届かない。読み取り（`read_dir`）は context 層の仕事で、パス形状規則は `discovery` と共用するので最下層に置くのが正しい（Phase 6 が動画 predicate を `src/context/video.rs` に置いたのと同じ構図。実測: `src/discovery/` は `crate::context` を import していないが、逆方向の rename -> discovery の新規依存は作らない）。
- 選択肢B（不採用）: `models::file_context::DirectorySummary { parent_directory, child_count }` を拡張/転用する。この型は「**target の親**ディレクトリ」を表し、`NormalizedFileContext` の直列化（`inspect` など）に出る。意味を変えると既存出力が変わり、「親の件数」と「子の件数」が 1 つのフィールドに混ざる。
- 選択肢C（不採用）: `src/rename/folder.rs` に自前のリーダと summary を置く。第三の summary 型を作ることになり、`build_request` に事実を届ける経路も別に作る必要がある。
- 帰結: 完全リテラル（`src/normalize/mod.rs:117`、`src/discovery/search_terms.rs:117,145`）に `directory: None` を足すだけ（Phase 6 の `video` と同じ 3 箇所。実測で他に無し）。

### D2. 子のサンプリング（件数・深さ・除外・上限の担保）

- 深さ: **子 1 階層のみ**。再帰は 0（§5.4「無制限の再帰解析は禁止」。深さ 1 はそもそも再帰が無い）。孫以降は名前も読まない。DIR05（ネストしたフォルダ）は「子フォルダの**名前**が証拠になること」「孫を読まないこと」で固定する。
- 件数: **`MAX_DIRECTORY_ENTRIES + 1 = 201` 件**を読み（上限に当たったことを検出するため 1 件超過を読む。plan-architect A2）、保持は 200 件。モデルへ渡す名前は `MAX_DIRECTORY_FILES = 12` / `MAX_DIRECTORY_FOLDERS = 8` 件、各種別 `MAX_DIRECTORY_EXTENSIONS = 8` 種、1 名前 `MAX_CHILD_NAME_CHARS = 48` 文字。
  - 200 の根拠: 既存の親読み取りは 100 件（用途が違う）で、子の列挙は上限内で 1 ms 未満（実測）。201 件目が現れたら `truncated = true` を立て（保持は 200 件）、プロンプトは「at least N entries」と正直に言う（**件数を偽らない**）。
  - 名前予算の根拠（plan-architect A4）: `swift/kaseifu_fm_bridge.swift:64` のプロンプト上限は 2000 文字で末尾から切られる。名前を 12 x 48 + 8 x 48 = 960 文字、種別を 8 x 16 = 128 文字に抑え、残りを既存行とフォルダ向け指示行に充てる。
- 種別の分布: **読み取った全エントリ**（上限 200）からファイルの拡張子を数える（名前サンプル 12 件から数えると 200 件フォルダで過少申告になる。plan-architect A3）。子フォルダは分布に含めない（「ファイル種別の分布」の字義どおり）。
- 順序: **名前でソート**してから上下限を適用（`read_dir` の順序は不定なので、決定的にする。テストと再現性のため）。
- 除外: 既存の **`normalize::noise::is_noise_file`** を再利用（`.DS_Store` / `.localized` / `node_modules` / `target` / `thumbs.db`）。`.git` のような隠し子は**除外しない**（名前は証拠であり、折りたたみはしない。symlink 子は名前だけ採り、辿らない = `read_directory` の `file_type()`）。
  - 既知のトレードオフ（受容しテストで固定）: `is_noise_file` は `ends_with` で判定するため `my-target` のような子も落ちる。上限付きサンプル 1 名の欠落であり、既存の単一権威を再利用する方を優先する。
  - symlink とその他の子は file/directory の**どちらにも数えない**（`noise::DirectorySummary::new` の既存規則どおり）。
- 選択肢（不採用）: 深さ 2（子フォルダの中まで）— 件数の上限が掛け算になり、`read_dir` の回数が読めない。全件列挙 — 大規模フォルダで破綻（DIR04 の趣旨に反する）。

### D3. 代表の子の内容（既存の有界リーダの再利用）

- 対象: 子のうち `EntryKind::File` かつ拡張子が `classify_document` の受理するもの（txt / md / markdown / html / htm / pdf）を、**名前順で先頭 `MAX_CONTENT_CHILDREN = 3` 件**。symlink は対象外（辿らない）。
- 読み取り: `read_bounded(child, MAX_DOCUMENT_BYTES)` + `extract_signals(kind, &bytes)` を**そのまま再利用**（子 1 件 = 32 KiB / `T:` 12 件）。
- 統合: 子ごとの `T:` を**長い順に上位 4 件**に絞ってから（コンテナの語彙より文を優先するため。§9.6）、**ラウンドロビン**で交互に取り、合計 `MAX_FOLDER_TEXT_SIGNALS = 12` 件に制限する（1 件の子が予算を独占しない）。既存の `contract::bound_signals`（20 件）が最終上限として残る。
- 読めない子: その子だけスキップして続行する（§12「ファイル読み取り失敗 = 対象をスキップ」の「対象」は子ファイル）。**選択した全子が失敗しても保留にしない**（名前と種別の分布は残るので、内容が無いこと自体は欠落値として扱い、捏造しない）。フォルダ自体が読めない（`read_dir` 失敗）場合は `RenameError::Unreadable` で fail-fast（1 対象コマンドなので「スキップ」= 何もせず理由を出す）。
- 非文書の子（画像・動画）: 内容は読まない（画像・動画の中身はそれぞれの経路の領分。フォルダ解析で ImageIO / AVFoundation を呼ばない = 知覚の二重実行をしない）。
- コスト: 子 3 件 x 32 KiB の読みと抽出（実測を §9 に記録）。
- 選択肢（不採用）: 全子の内容（予算が子の数に比例）、サイズ上位 3 件（候補ごとに `stat` が要り、長いログ 1 本に偏る）、OCR / 画像知覚（スコープ外）。

### D4. 契約とプロンプト（`fm-rename-v1` の add-only）

- 選択肢A（採用）: `input.directory` を**任意**で追加。`schema_version` は `fm-rename-v1` のまま（既存キーの削除・型変更なし。§3.4 の「未知フィールドを安全に処理」規約は両側に既にある）。

```json
"directory": {
  "total_entries": 4, "file_count": 3, "directory_count": 1, "truncated": false,
  "files": ["contract.pdf", "invoice_001.pdf", "invoice_002.pdf"],
  "folders": ["archive"],
  "extensions": [{"extension": "pdf", "count": 2}, {"extension": "xlsx", "count": 1}]
}
```

- 子の**テキスト**は新しいキーを作らず、既存の `semantic_signals`（`T:`）に載せる（プロンプトの `signals:` 経路が文書とフォルダで 1 本になる）。Swift 側はフォルダのときだけ「この signals はフォルダ内のファイルからの抜粋である」と 1 行添える。
- **フォルダ自身の名前は request から落とす**（plan-architect A1、実装で範囲を拡大: §9.6）: `src/normalize/mod.rs:54-68` は常に `F:{stem}` signal を積み、Spotlight の `kMDItemTitle` はフォルダではその名前そのものなので `M:`/`K:` も名前を運ぶ。そのままだとフォルダ名が `signals:` 行としてモデルに届き、D4 の「`filename:` を出さない」が無意味になる。フォルダ経路では **観測由来の signal をすべて落とし、子から読んだ抜粋だけ**を載せる。フォルダ名の prompt-injection 検査は `context.target.filename` を使う既存検査で**そのまま残す**（signals には依存しない）。
- Swift の `kfBuildPrompt`（フォルダ分岐のみ）: `filename:` 行を**出さない**（フォルダ自身の名前は入れ替え対象であって証拠ではない。命名が子の内容に追従することをプロンプト構造で担保する）+ フォルダ向けの**指示行を `folder_files:` より前に**置き + `folder_files:` / `folder_folders:` / `folder_types:` / `folder_size:`（`truncated` のとき「at least」）。指示行は「フォルダの現在名は証拠ではない」「内容が混在する場合は共通テーマを作らず title を空にする」の 2 行（2000 文字上限で末尾が切られても指示が残る配置）。`kfRunGeneration` に `directoryInstructions` を追加。
- `target_kind` の配線: `provider::target_kind` に `is_dir()` 分岐（variant は既存）。`build_request` に `directory: directory_facts(context)`。
- 選択肢B（不採用）: `input.folder` を別名で作り、子のテキストを `input.folder.signals` に分離する。プロンプト分岐と bound が二重になる。
- 選択肢C（不採用）: `fm-rename-v2`。破壊的変更が無いので差分に見合わない。
- `schemas/fm-rename-v1.json`: `input.directory` を任意プロパティとして追記（required に入れない）。

### D5. フォルダの命名日付

- 結論: **フォルダ自身の作成日時 -> 更新日時 -> 現在時刻**（`date_source` は `created` / `modified` / `now` をそのまま表示）。EXIF も本文日付も無い（フォルダには撮影も本文も無い）。`now` は既存規則どおり `now` と表示し、**フォルダの日付であるかのように偽らない**。
- 実装: `DateMode::Directory` を追加し `resolve_date(created, modified, None, None)` に対応させる（`DateMode::Document` と実質同じ経路だが、意図を型で固定しテストで守る）。
- 選択肢（不採用）: (a) 日付を使う規則をフォルダでは保留にする — 既定規則 P02（`{date}_{title}`）が常に保留になり、`kaseifu rename <dir>` が既定で何も提案できなくなる。(b) 最も新しい子・最も古い子の日付 — 子の日付をフォルダの日付として提示することになる（不誠実）。(c) 子の文書から拾った日付 — 同じ問題に加え、子ごとに食い違う（§6.4 の「曖昧な場合はユーザー確認」の範囲外）。(d) 親フォルダの日付 — target の日付ではない。
- 既知の限界（§9 に記録）: フォルダの mtime は子の追加・削除で動くため「そのコレクションが最後に変わった時刻」を意味する。作成日時は Spotlight（`kMDItemFSCreationDate`）由来で、索引が無い環境では `modified` に落ちる。

### D6. 空フォルダと内容の混在

- 空（除外後の子が 0 件。`.DS_Store` だけのフォルダも含む）: **モデルを呼ぶ前に**決定的に `Held` + 理由「the folder is empty (no files or folders inside); proposal withheld」。
- 内容が混在: モデルに「共通テーマが無ければ title を空にせよ」と指示し、空 title は既存 §6.5 の経路で `Held` + フォルダ向け理由「no title could be drawn from the folder contents (mixed or unclear); proposal withheld」。Rust 側に「混在」判定の閾値は**置かない**。
  - 選択肢（不採用）: 拡張子エントロピーなどの決定論的判定 — 原典 §5.4 の例（`invoice_001.pdf` / `invoice_002.pdf` / `contract.pdf` / `payment.xlsx`、pdf と xlsx の混在）は**命名されるべき**ケースであり、閾値はこの良い例を落とし得る。混在の判断は「共通テーマを作らない」という指示に従うモデルの領分で、判断できないときは空 title = 保留になる（捏造より安全側）。
- 読めない子しか居ない場合は保留にしない（名前と種別が証拠として残る）。

### D7. Journal と Undo、親子の履歴解決（本ラウンドの核）

- 前提の実測: `undo` は「最後の成功・未 undo の `move`」を rowid 降順で選び（`storage/history.rs:167-181`）、CLI はそれを最大 N 回繰り返す（`main.rs:864-878`）。つまり **undo は常に新しい順**で、祖先の rename 行は、その下で記録された行より必ず先に戻される。
- 結論（採用する設計）:
  1. **フォルダの rename は既存の `move` 行 1 行**として記録する（新しい操作種別・新しいテーブル・スキーマ変更なし）。
  2. **`undo` はディレクトリを受け入れる**: `undo.rs:66` の `is_file()` 要求を「ファイルまたはディレクトリ（symlink は拒否）」に広げる。
  3. **子の履歴は「新しい順 undo」の不変条件で解決する**: 親を rename した後でも、親の行が先に戻されて親の名前が復元されるため、子の行が記録したパスはそのとき実在する。この不変条件を DIR07 / DIR08 のテスト（子 -> 親、親 -> 子の両順序、execute -> undo の往復）で固定する。
  4. **Journal の行は書き換えない**。解決できない場合は**正直に拒否**する（記録されたパスが存在しなければ、そのパスを名指しして何も動かさない）。メッセージには「kaseifu の外で親フォルダが改名された可能性」と `doctor` への導線を添える。
- 選択肢と失敗モード:
  - A（不採用）**rename 時に子孫行を書き換える**: (a) 履歴が「実際にどこにあったか」を語らなくなる（監査性の喪失）、(b) 祖先の rename を undo したときに書き換えを逆方向に戻す仕組みが要り、忘れると子の行が存在しない名前を指す、(c) 書き換えの途中失敗で journal が半端な状態になる（`move` 行は 1 行なので原子性を保てるが、N 行の書き換えは保てない）、(d) `organize` / `watch` が記録した他人の行まで書き換える（影響範囲の拡大）。
  - B（不採用）**undo 時に祖先の rename 行を通してパスを解決する**: (a) 上記の不変条件により CLI の経路では到達しない（= 到達しないコードを足すことになる。実測: `undo` の選択は常に新しい順）、(b) 到達し得るのは「祖先の行が `pending` のまま親だけ動いた」場合だが、**未確認の行を根拠に mutation を行うのは Fail-Open**（in-flight の操作と競合し得る）、(c) kaseifu の外（Finder 等）の改名はどの行にも説明できないので、解決は結局できず「正直な拒否」に落ちる。
  - C（採用）**不変条件で解決し、説明できない場合だけ正直に拒否する**: 上記 1-4。
- 壊さないこと: 二重 undo 防止（`undone_at IS NULL` の選択と `mark_undone`）と上書き禁止（`original_path.exists()` で `Collision`）は変更しない。ディレクトリでも同じ 1 行・同じ順序（record -> rename -> mark）を使う。

### D8. 安全性の拒否（対象がフォルダのとき）

- 拒否規則（`src/context/directory.rs::refuse_reason(dir, home, cwd) -> Option<DirectoryRefusal>`。**純関数・I/O なし**なのでテストで cwd を差し替えられる）:
  1. ボリューム根（`/`）と OS の根そのもの（1 コンポーネント: `/System` `/Library` `/Applications` `/Users` `/Volumes` `/private` `/usr` `/bin` `/sbin` `/var` `/tmp` `/etc` `/opt` `/dev` `/cores` `/home`）
  2. ホームディレクトリ自身とその祖先
  3. カレントディレクトリ（cwd）とその祖先（走っている場所を動かさない）
  4. bundle / アプリ管理コンテナ（`Foo.app` / `X.library` 等。`destinations` から移す規則）
  5. macOS の Library ツリー（`/Library`、`<home>/Library`、`<volume>/Library` の上位 3 コンポーネント以内）
  6. OS 管理領域（`.Trash` / `.Trashes` / `.Spotlight-V100` / `.fseventsd` / `.DocumentRevisions-V100` / `.TemporaryItems`）
  7. cache / temp 領域（コンポーネント名 `cache` / `caches` / `temporaryitems` / `*.cache`）
  8. 隠しフォルダ（自身の名前が `.` で始まる）
- 判定の入力は **`std::fs::canonicalize` したパス**（失敗時は与えられたパスに落とす）と `RefusalEnv { home, cwd }`（工程側で `dirs::home_dir()` / `std::env::current_dir()` から作る。`src/config/storage.rs:32` と同じ home の取り方）。相対パス（`rename tree/新しいフォルダ`）でも規則 2・3 が正しく効く（plan-architect A5）。
- **destination 規則との意図的な差**（記録）: `is_excluded_destination` は system root の**部分木全体**（`/var/**`、`/tmp/**`）を拒否する。これをそのまま rename の対象へ適用すると、macOS 自身の `$TMPDIR`（実測 `/var/folders/7t/.../T/...`）配下や CI / ワークフローの一時フォルダがすべて拒否され、機能が実質使えなくなる（テストも隔離 HOME を一時領域に置けなくなる）。そこで **rename 対象には「根そのもの」だけを拒否し、根の中のユーザーのフォルダは許す**。destination 側の規則は一切変更しない（`/var/**` は destination としては従来どおり拒否）。
- 単一権威の保ち方: 規則 4-8 は destination 権威と**同一の規則**なので、実装を `src/context/directory.rs` へ移し、`src/discovery/destinations.rs` がそれを呼ぶ（destinations は自前の「system root 部分木」「生成名」規則を残す = 7 規則の内訳は不変）。実測根拠: `src/discovery/` は `crate::context` を import しておらず（依存の向きは context -> 無し）、`is_excluded_destination` は既に `operations`/`benchmark` から呼ばれている。
- その他の拒否（既存の仕組みを再利用）: symlink の対象（`is_symlink`）、destination が既に存在（`Collision`）、親が使えない（`ParentMissing` / `ParentNotADirectory`）、読み取り専用（`ReadOnly`）、対象の消失・入れ替わり（`SourceMissing` / `SourceNotADirectory`）。
- TOCTOU: ファイルは既存の (size, mtime) 比較。**フォルダは (dev, ino) の同一性**で判定する（サイズと mtime は子の追加・削除で正当に動くため、それを「変更された」と見なすと正当な rename を拒否してしまう。逆に (size, mtime) は「同じフォルダか」という本当の問いに答えない）。mtime が動いただけのフォルダは実行を拒否しない（プロポーザルの根拠は子の内容であり、実行時の安全性は「同じオブジェクトか」で決まる）。

### D9. 変わるユーザー可視面

- `rename <dir>`（dry-run 既定 / `--execute` / `undo`）と `--format json`（`directory` を追加。既存キーは削除も型変更もしない）。
- `--help`（`Commands::Rename` の doc コメント）と `rename` の guidance: フォルダを受理すること、拒否される対象（システム・bundle・Library・cache・隠し・cwd）を ASCII で明記。内部 Phase 語は出さない（`tests/v070_zero_config_p1_help_test.rs` の自動走査を通す）。
- `--patterns`: 変更なし（`{device}` を含む P11 はフォルダでは既存の missing-value 理由で保留 = 撮影デバイスを捏造しない）。
- `README.md`: rename 節にフォルダの段落（子の内容から命名・サンプリング上限・保留条件・拒否条件・undo と親子整合）を追加。
- `src/rename/document.rs` の extension 無しメッセージ: フォルダと画像・動画を含む現実に合わせて修正（虚偽の除去）。

### D10. テスト用フィクスチャ

- **合成フォルダのみ**、`std::env::temp_dir()` 配下 + 隔離 HOME（`KASEIFU_NO_COMPLETION_UPDATE=1`）。symlink フィクスチャはテスト自身のディレクトリ内に作り、テスト内で消す。CI で安全（`#[cfg(unix)]` の symlink は `std::os::unix::fs::symlink`）。
- 実ユーザーのフォルダ（`~/Documents` / `~/Desktop` / `~/Library/Application Support/kaseifu/`）、実 Keychain、実補完ディレクトリ、実ファイルは**読み書きしない**。リポジトリ内にフィクスチャを置かない（テストソース以外は何も増やさない）。
- 実機検証（§9）も隔離 HOME + 一時 dir + 合成ツリーのみ。
- 選択肢（不採用）: リポジトリ内の `tests/fixtures/directory/` に実フォルダを置く — フォルダのサンプリングは「その場の名前と内容」を読むだけなのでバイナリフィクスチャは不要で、git に空フォルダを持てない問題も避けられる。

### D11. ラウンド分割の検討（分割しない）

- 検討: 「フォルダの計画・rename・undo」と「§14 の親子関係の整合性」を 2 ラウンドに分ける案。
- 結論: **分割しない**。理由: (a) §10 / #14 の要求は D7 の不変条件（新しい順 undo）で満たされ、実装コストは `undo.rs` の 1 分岐 + テストである、(b) フォルダの undo が動かないまま「フォルダの rename が完成」と報告することになり、受け入れ基準 #3 と #14 が同一ラウンドで閉じない、(c) 分けた場合、2 ラウンド目は `undo` の 1 分岐とテストだけになり、round としての検収が成立しない。
- 帰結: 本ラウンドで DIR01〜DIR08 を一通り実装・検証する。

## 5. BDD シナリオ（振る舞いと具体例）

### フォルダ境界（`src/context/directory.rs` / `src/rename/directory.rs`）

- F1 読み取りと上限（DIR01 / DIR04）: 子 4 件のフォルダ -> `DirectoryFacts { total_entries: 4, file_count: 4, directory_count: 0, truncated: false }` + `files` 4 件（ソート済み）+ `folders` 空 + `extensions` = `pdf 2, xlsx 1`（件数降順・同数は名前順）。14 件の子 -> `files` は 12 件で打ち切り（`total_entries` は 14 のまま）、`extensions` は 14 件全部から数える。250 件の子 -> `total_entries: 200` / `truncated: true`（201 件目を読んで検出）。
- F1b 名前の長さ: 65 文字の子名 -> `files` に入る名前は 48 文字に切られ、切られたことは長さで判定できる（件数と分布は影響を受けない）。
- F2 除外（DIR03）: `.DS_Store` / `node_modules` / `Thumbs.db` はサンプルから落ち、`.git` と `my-target` は **前者は残り後者は落ちる**（既存 `is_noise_file` の `ends_with` 仕様を固定）。symlink 子は名前だけ採り、file / directory のどちらにも数えない。
- F3 深さ（DIR05）: 子フォルダの中にファイルがあっても、それを読まない（孫の名前も `T:` も現れない）。子フォルダの**名前**は `folders` に入る。
- F4 代表の内容（DIR01）: 子 4 件（すべて文書）-> 名前順で先頭 3 件を読み、`T:` は子をラウンドロビンで混ぜて合計 12 件以内。読めない子（権限なし / ディレクトリに差し替え）はその子だけスキップし、他の子の `T:` は残る。
- F5 分類と拒否（SAFE03 / §5.4）: `classify_directory(<dir>)` は OK。`<file>` は理由付きで拒否。`refuse_reason` が `/`、`/System`、`/tmp`、`<home>`、`<home>/Library/...`、`Foo.app`、`.hidden`、cache 配下、cwd 自身と cwd の祖先で `Some(...)`、`<home>/Documents/新しいフォルダ`・`/tmp/scratch/新しいフォルダ`・`/var/folders/xx/yy/T/新しいフォルダ` で `None`（D8 の差を固定。固定の `RefusalEnv` を渡す = 実行環境に依存しない）。
- F6 名前形状: `check_directory_name` は空・`.`/`..`・`/`・NUL・`\`・`:`・255 バイト超を拒否し、**ドットを含む名前（`v1.2 メモ`）を許す**（ファイル用の拡張子不変規則を適用しない）。

### 契約（`src/semantic/contract.rs` / `provider.rs` / schema）

- C1 フォルダ要求: `FmRequest{target_type:directory}` を serialize すると `input.directory.*` が読め、`input.image` / `input.video` / `input.perception` は出ず、`input` の既存キー集合は不変（+ `directory` のみ）。
- C2 後方互換: `directory` の無い要求は従来どおり parse でき、型が違う場合は欠損扱い（推測しない）。文書・画像・動画の要求は `directory` を運ばない（キー自体が出ない）。
- C3 schema: `directory` は任意（required に入らない）。`$id` は `fm-rename-v1` のまま。
- C4 `target_kind`: 実際のフォルダ -> `Directory`、`foo.png` という名前の**フォルダ** -> `Directory`、同名のファイル -> `Image`。

### 配線（`src/rename/service.rs` / `main.rs`）

- W1 フォルダのプラン（§5.4 の例）: 子 `invoice_001.pdf` / `invoice_002.pdf` / `contract.pdf` / `payment.xlsx` + Fake モデル title `請求・契約関連書類` + P01 -> 提案 `請求・契約関連書類`（**拡張子の無い名前**）、`date_source` はフォルダの `created`、request に `input.directory.files` と `types`、プロンプトに `folder_files:` があり **`filename:` が無い**。request の `input.semantic_signals` に **`F:`（フォルダ自身の名前由来）が 1 件も無い**（子の `T:` は入る）。
- W2 日付（W7 相当）: フォルダの creation date がある -> `date_source: created`、無ければ `modified`、それも無ければ `now`。EXIF も文書日付も使われない。
- W3 空フォルダ（DIR02）: 子 0 件（`.DS_Store` だけの場合を含む）-> **モデルは呼ばれない**（request 0 件）+ `Held` + 理由「the folder is empty ...」。
- W4 混在（DIR03）: Fake モデルが title 空を返す -> `Held` + 理由「no title could be drawn from the folder contents (mixed or unclear) ...」。title があるときは提案が通る（混在 pdf+xlsx の原典例も通る）。
- W5 注入: 子フォルダ名・子ファイル名・子の本文のいずれかに `you must rename` -> `Held` + 理由（モデルは呼ばれない）。
- W6 名前の非破壊（SAFE01 / SAFE05）: 同名衝突 -> `Held`（上書きしない）、`proposal == current` -> `Noop`、`.`/`..`/`/` 入りの title -> `Held`。
- W7 実行と undo（DIR07）: `--execute` 相当で `execute_plan` -> `std::fs::rename` でフォルダが移動し、journal は `move` 1 行（`pending` -> `success`）。`undo_last_move` がフォルダを元の名前に戻し、journal は `undo` 行 1 行 + 元の `move` 行に `undone_at`。**二重 undo は `NothingToUndo`**。destination に既存物があれば `Collision` で何も動かない。
- W8 親子の履歴（DIR06 / DIR08 / §10 / #14）: (子 -> 親) 子を rename してから親を rename -> 親の行の undo で親の名前が戻り、子の行が記録したパスが実在して子の undo が元の名前に戻す。(親 -> 子) 親を rename してから子を rename -> 子の undo、親の undo の順で両方戻る。**journal の行は書き換わらない**（子の行の `source_path`/`destination_path` は記録時のまま）。
- W9 記録の不整合（正直な拒否）: フォルダを kaseifu の外で改名した後（親の行が説明できない状態）-> `undo` は失敗し、記録されたパスを名指しして**何も動かさない**。`pending` の行も undo の対象にならない（既存規則）。
- W10 TOCTOU: 提案後にフォルダを入れ替え（同じパスに別のフォルダを作り直す）-> (dev, ino) が変わり `execute_plan` は `SourceChanged` / `SourceNotADirectory` で拒否。読み取り専用のフォルダは `ReadOnly`。
- W11 CLI: `rename <dir>`（dry-run）-> exit 0・`proposal:` 行・`directory:` 行・`mode: dry-run`。`--format json` は既存 10 キーを保ったまま `directory` を追加。`rename <dir> --execute` -> `status: executed` + `operation_id`。`undo` -> `restored: <元の名前>`。拒否対象（`/tmp` 自身、`.hidden` など）は fail-fast で理由を出す。
- W12 非退行: `note.txt` のプロンプトに `directory` キーも `folder_files:` 行も現れず、`IMG_1234.png` / `clip.mp4` の既存出力は不変（既存 v092/v093/v095 の assert を維持）。

### Swift 側

- S1 フォルダのプロンプト: `target_type == "directory"` + `input.directory` -> `folder_files:` / `folder_folders:` / `folder_types:` / `folder_size:`（`truncated` のとき `at least`）が出て、`filename:` は出ず、「内容が混在する場合は title を空にせよ」の行が出る。最大サンプル（12 名 x 48 文字 + 8 名）を入れても、**指示行と `folder_files:` 行が両方 2000 文字以内に残る**（末尾切れの影響を受けない配置）。
- S2 バイト不変: 文書・画像・動画のプロンプトは HEAD と完全一致（`target_type == "directory"` 以外では新規行が発火しないことを文字列比較で固定）。
- S3 命令: `kfRunGeneration` の instructions が directory のときだけ `directoryInstructions` になる。

### 非破壊（全ラウンド共通）

- N1: 既存 729 テストが全て通る（変更は完全リテラル 3 箇所 + 完全リテラル `FmInput` の既存 4-5 箇所 + 文言テストのみ、assert は不変）。
- N2: `cargo fmt --check` 違反 0、clippy 警告 0、**新規 CC>=16 なし**、既存最大 CC（15）を悪化させない。
- N3: 出力 ASCII のみ（提案名の非 ASCII を除く）・em dash なし・TUI なし・NO_COLOR / 非 TTY 準拠。`--format json` は追加のみ。help に内部 Phase 語を出さない。
- N4: 新規テストは隔離 HOME・一時 dir・注入のみ。実ユーザーのフォルダ（`~/Documents` / `~/Desktop` / `~/Library/Application Support/kaseifu/`）・実 Keychain・実補完 dir を読み書きしない。実ファイルの移動は一時 dir 内の合成ツリーのみ。秘密情報を混ぜない。

## 6. タスク一覧（実装順・直列）

- T1 `src/context/directory.rs`（新規）: `MAX_DIRECTORY_ENTRIES = 200`（読みは +1 件）/ `MAX_DIRECTORY_FILES = 12` / `MAX_DIRECTORY_FOLDERS = 8` / `MAX_DIRECTORY_EXTENSIONS = 8` / `MAX_CHILD_NAME_CHARS = 48`、`DirectoryFacts`（summary + files + folders + extensions + truncated）、`read_directory_facts(path)`（`read_directory` 再利用 + ソート + `is_noise_file` 除外 + 上限 + truncated 検出。extensions は読んだ全ファイルエントリから算出）、`extensions_of(entries)`、`refuse_reason(dir, &RefusalEnv)` と `DirectoryRefusal` / `RefusalEnv` と Display。F1〜F3 / F5。`src/context/mod.rs` に `pub mod directory;`。
- T2 `src/models/file_context.rs`: `DirectoryFacts`（serde。summary は `normalize::noise::DirectorySummary`）+ `NormalizedFileContext.directory: Option<DirectoryFacts>`。`src/normalize/mod.rs:117` と `src/discovery/search_terms.rs:117,145` に `directory: None`。
- T3 `src/discovery/destinations.rs`: 規則 4-8（bundle / Library / system area / cache-temp / hidden）を `crate::context::directory` の関数へ委譲（挙動不変。既存テストはそのまま）。
- T4 `src/rename/directory.rs`（新規）: `classify_directory`（非ディレクトリを理由付きで拒否）、`refuse_rename`（ホームと cwd を渡す薄いラッパ）、`DirectoryError`。F5。`src/rename/mod.rs` に `pub mod directory;`。
- T5 `src/rename/service.rs`: `plan_directory_rename`（classify -> canonicalize + refuse -> observe -> 子の観測 -> 空なら保留 -> 選択規則 -> 子の内容（T:）-> 注入検査 -> analyze -> 日付 -> `finish_directory_plan`）。フォルダ経路では **`F:` signal（target 自身の名前由来）を request に載せる前に落とす**（plan-architect A1）。`DirectoryReport`（summary + サンプル件数 + types）と `RenamePlan.directory` / `PlanFacts.directory`、`DateMode::Directory`、`plan_rename` の dispatch 追加、`execute_plan` のディレクトリ分岐。W1〜W10。
- T6 `src/rename/validate.rs`: `check_name_shape` + `check_directory_name`、`DirectorySnapshot { device, inode }` + `snapshot_directory` + `check_directory_stable` + `recheck_directory_before_execute`、`NameProblem::SourceNotADirectory`。F6 / W10。
- T7 `src/rename/document.rs`: extension 無しメッセージの修正（フォルダと画像・動画を含む現実に合わせる）。
- T8 `src/semantic/contract.rs`: `FmDirectoryFacts` + `FmInput.directory`（任意・skip）+ テスト。C1〜C3。
- T9 `src/semantic/provider.rs`: `target_kind` の `is_dir()` 分岐、`directory_facts` 写像。C4。
- T10 `schemas/fm-rename-v1.json`: `input.directory` の明文化（任意）。
- T11 `swift/kaseifu_fm_bridge.swift`: `kfBuildPrompt` のフォルダ分岐（`filename:` を出さない、`folder_*` 行、混在時の指示）と `directoryInstructions`。S1〜S3。
- T12 `src/operations/undo.rs`: ディレクトリの undo を受理（`is_file() || is_dir()`）、`UndoError` に `Display` を追加して事実に合う文言にし（ファイル・フォルダのどちらでもない / 記録されたパスが無い）、`apply_single_undo` の `{:?}` を `{}` に切り替える。W7 / W9。
- T13 `src/main.rs`: `reject_foreign_rename_target` がフォルダを受理、`Commands::Rename` doc と guidance の更新、`RenameReport.directory` + `directory_json` + `directory:` 行、`apply_single_undo` のメッセージ。W11。
- T14 テスト: `tests/v096_ai_rename_p7_directory_test.rs`（F4・C1〜C4・W1〜W12・S1〜S2）+ 各モジュールの単体テスト。
- T15 README 更新: rename 節にフォルダの段落（子の内容から命名・上限・保留・拒否・undo と親子整合）。
- T16 実機検証（テストに含めない。§9 に記録）: 合成フォルダの dry-run / `--execute` / `undo`、配線前（HEAD `4d94c40`）との同一入力比較（= フォルダ自身の名前ではなく子の内容に追従することの before/after）、`--format json`、所要時間、空フォルダ・混在・拒否対象、文書/画像/動画の非退行。
- T17 検収: `cargo test --all-targets --all-features`（通常 + `KASEIFU_FM_FORCE_STUB=no-sdk`）、fmt、clippy、`bash scripts/quality_check.sh`、`git status` / `git diff` 点検（commit しない）。

## 7. 検証手段（実測で示す）

- `cargo test --all-targets --all-features`（729 + 新規の pass/fail 数）。
- `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --all-targets --all-features`（Swift / Foundation Models の無いビルドの縮退）。
- `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`bash scripts/quality_check.sh`（OVERALL と complexity 表）。
- 実機 CLI（隔離 HOME・一時 dir・合成ツリーのみ）: dry-run / `--execute` / `undo` / 二重 undo / `--format json` / 所要時間 / 空フォルダ / 混在 / 拒否対象。配線前バイナリ（`git archive` から `4d94c40` を展開してビルドしたもの）との同一入力比較。
- 親子の履歴（D7）: 子 -> 親 / 親 -> 子の両順序で execute -> undo を実機で一巡し、journal の行を `sqlite3` で直接確認する（行が書き換わっていないこと、`undone_at` が付くこと）。
- `git status --short` と `git diff --stat` の目視（無関係変更・秘密情報なし、`Cargo.toml` / `Cargo.lock` 無変更）。

## 8. 受け入れ基準（原典のうち本ラウンド分）との対応

| 原典 | 本ラウンドでの担保 |
|---|---|
| §5.4 解析対象（子ファイル名・子フォルダ名・種別の分布・代表ファイルの内容・構造・親フォルダ名） | `DirectoryFacts`（files / folders / extensions / counts）+ 子 3 件の `T:` + 既存の `parent_directory`（D1〜D4 / T1・T5・T8）。実機で request を確認（W1 / §9） |
| §5.4 生成例（`新しいフォルダ/` -> `請求・契約関連書類/`） | 同じ子 4 件の合成ツリーで実機検証し、拡張子無しの提案になることを示す（§9） |
| §5.4 要件: 大規模フォルダはサンプリング | 読み 200 件・提示 20+10 件・`truncated` の明示（D2 / F1） |
| §5.4 要件: 無制限の再帰解析は禁止 | 深さ 1 固定（再帰なし）。孫を読まないことをテストで固定（D2 / F3） |
| §5.4 要件: 空フォルダは保留 | モデルを呼ぶ前に `Held` + 理由（D6 / W3 / DIR02） |
| §5.4 要件: 混在は共通テーマを作らない | プロンプトで title 空を許し、空 title は保留（D6 / W4 / DIR03） |
| §5.4 要件: 子要素のパス変更を考慮 | D7（新しい順 undo の不変条件 + 正直な拒否）と DIR06/DIR08 のテスト、`undo` のディレクトリ対応（T12 / W7〜W9） |
| §6 命名規則エンジン（P01-P12・変数・日付形式） | 既存の規則エンジンをそのまま適用（フォルダ専用のテンプレート変数は増やさない）。`{parent}` は親の名前、`{type}` は空（拡張子が無い）、`{device}` は空 = P11 は保留（既存の missing-value 理由） |
| §9.1 必須検証（空・`.`/`..`・区切り・NUL・長さ・同名・symlink・No-op） | `check_directory_name` + 既存の衝突/No-op 判定（D8 / F6 / W6） |
| §9.2 TOCTOU（同一性・移動先の存在・親の状態・権限） | ディレクトリ版の再確認（dev/ino + 種別 + 書込可 + destination 空き + 親）（D8 / W10） |
| §9.3 Journal（Pending -> Rename -> Success、失敗は Failure） | 既存の `move` 行をそのまま使用（mutation の前に記録、結果を後で更新）（T5 / W7） |
| §10 Undo（フォルダを戻せる・変更前の名前を保持・上書きしない・二重 undo 防止・親フォルダ改名後も子の履歴が解決する） | D7 + T12。フォルダの undo、`undone_at`、`Collision`、`NothingToUndo` を実機とテストで確認（W7〜W9 / DIR07 / DIR08） |
| §11.2 Directory Sampling / Lazy Extraction / Early Exit | 読み 200 件の上限（Sampling）、子 3 件だけ内容を読む（Lazy）、空・注入・未知型はモデル前に早期確定（Early Exit）（D2・D3・D6） |
| §11.3 画像最適化との整合 | フォルダ解析では画像を開かない（ImageIO / Vision を呼ばない。子の画像は名前と拡張子のみ）（D3） |
| §12 エラー処理（必須意味情報不足 = 保留、ファイル読み取り失敗 = スキップ、名前衝突 = 実行禁止、権限不足 = 実行禁止、Journal 書き込み失敗 = リネーム禁止、FFI エラーは安全に伝達） | 空/混在/注入は保留、読めない子はスキップ、衝突・権限・journal 失敗は実行禁止、モデル不可は `model unavailable:` で終了（W3〜W6・W10） |
| §13.5 DIR01〜DIR08 | DIR01 通常（F1/F4/W1）、DIR02 空（W3）、DIR03 混在（F2/W4）、DIR04 大規模（F1）、DIR05 ネスト（F3）、DIR06 親子同時（W8）、DIR07 フォルダ Undo（W7）、DIR08 子ファイル履歴（W8） |
| §14 Phase 7（フォルダ解析・フォルダ命名・親子関係の整合性・Undo） | 本ラウンドの全タスク |
| §15 #3（文書・画像・動画・フォルダをリネームできる） | フォルダの dry-run / `--execute` / `undo` を実機で一巡（W11 / §9）。残り 3 種は既存テストで非退行を確認 |
| §15 #14（親フォルダのリネーム後も履歴の整合性を維持する） | DIR06 / DIR08 の両順序テスト + 実機の journal 確認（D7 / W8 / §9） |

## 9. 検収記録（追記式。実装後に実測値を埋める）

### 9.1 ベースラインとテスト（実測）

- ベースライン: `cargo test --all-targets --all-features` = **729 passed / 0 failed / 4 ignored**（HEAD `4d94c40`、本スレッドで再実測）。
- 実装後: **774 passed / 0 failed / 4 ignored**（+45 = v096 統合 25 + 単体 20。内訳: `context::directory` 11、`rename::directory` 2、`rename::validate` 2、`rename::service` 5）。**既存 729 の assert は 1 件も変えていない**（v094 のパス期待値を壊さないよう、canonical 化は journal に記録する `source`/`destination` に限定した — §9.6）。
- `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --all-targets --all-features` = **774 passed / 0 failed / 4 ignored**（Swift / Foundation Models の無いビルドでも同じスイートが通る）。
- `cargo fmt --all -- --check` 違反 0。`cargo clippy --all-targets --all-features -- -D warnings` 警告 0。
- `bash scripts/quality_check.sh` = **OVERALL: PASS**（exit 0）。complexity: 関数 1322、CC<=10 1310（99.1%）、CC 11-15 = 12、**CC>=16 = 0**、最大 CC 15（**ベースライン同**）、baseline-gate PASS、coverage 84.69%（report-only）。新規関数の実測 CC: `refuse_reason` 11 / `plan_directory_rename` 8 / `execute_plan` 9 / `check_directory_stable` 7 / `merge_child_signals` 6 / `finish_directory_plan` 6 / `validate_undo_preconditions` 7 / `assemble_plan` 8 / その他は 1〜5。
- `git status --short`: 変更 17 ファイル + 新規 7（計画書 1 ・ issue 3 ・ `src/context/directory.rs` ・ `src/rename/directory.rs` ・ `tests/v096_*`）。`git diff --stat Cargo.toml Cargo.lock` は空（**新規 crate なし**）。`.gitignore` の差分は本ラウンド以前からのもので触っていない。

### 9.2 実機 CLI（隔離 HOME=/tmp/kb-p7/e2e/home、一時 dir、合成ツリーのみ）

- **配線前（HEAD `4d94c40` を `git archive` から展開してビルドした `/tmp/kb-p7/pre/target/debug/kaseifu`）**: `rename tree/A-untitled` -> `Error: tree/A-untitled has no extension; rename needs txt, md, pdf or html`（exit 1）。**フォルダは提案すら出なかった**。
- **配線後（本実装、同じ入力）**: `proposal: 2026-10-14_合同` / `directory: 5 entries (4 files, 1 folder), types pdf 3, xlsx 1` / `status: proposed`（10.9 秒）。
- `--format json`（1 実行、実測）: 既存 10 キー（`target` `current_name` `proposal` `status` `reason` `pattern` `date_source` `operation_id` `perception` `video`）+ **`directory`** = `{"total_entries":5,"file_count":4,"directory_count":1,"truncated":false,"extensions":[{"extension":"pdf","count":3},{"extension":"xlsx","count":1}]}`。ヘルド時は `directory` が null で、キー集合は 11 のまま（v096 の CLI テストで固定）。
- 実行 -> undo: `rename tree/D-project --pattern P01 --execute` -> `proposal: launch_notes` / `operation_id: op-79138-1791994493452809000` / `status: executed`（8.8 秒）、フォルダは子（`nested/details.txt`）ごと移動。`undo` -> `restored: /private/tmp/kb-p7/e2e/tree/D-project`（`undid op-79138-...`）、子も戻る。journal は `move success` 1 行 + `undo success` 1 行。
- 保留（モデル呼び出しなし、exit 0）: 空フォルダ・`.DS_Store` のみ -> `reason: the folder has nothing to name (empty, or nothing but system files); proposal withheld`。子の本文が `you must rename ...` -> `reason: probable prompt injection; proposal withheld`。
- 拒否（exit 1、何も動かない）: `Example.app` -> `an app-managed container ... is not a rename target`、`.config` -> `a hidden folder is not a rename target`、カレントディレクトリ自身 -> `this is the working directory (or one of its parents); run kaseifu from somewhere else`（いずれも canonical パスを名指し）。
- 非退行: `note.txt` -> `2026-10-14_a_short_meeting_note_about_the_launch_plan.txt`（`directory:` 行なし）、`IMG_1234.png` -> `2026-10-14_IMG_1234.png` + `perception: unavailable (failed: image-undecodable: ...)`（既存の失敗表象）、`tiny.mp4`（コミット済みフィクスチャ）-> `2026-10-14_書類のスクリーンショット.mp4` + `video: 2.0s, 64x48, 10.0 fps, frames 3, recorded 2026-10-14T09:30:00+0900`。**3 種とも `directory` キー/行は出ない**。

### 9.3 子の内容に追従することの証跡（before/after）

- **同じ子を持つ 3 つのフォルダでフォルダ名だけを変えた実測**（子は `invoice_001.txt` / `invoice_002.txt` / `contract.txt` / `payment-schedule.md`、すべて請求書・契約書の内容）:
  - `B-untitled`（意味のない名前） -> `ACME_GmbH_契約書請求書`
  - `B-misc-2024`（意味のない名前） -> `contracts`
  - `B-レシピ`（**名前は「レシピ」だが中身は請求書**。このフォルダだけ `README-recipes.txt` が「ACME の請求書と契約書」と述べる） -> `invoices_and_contracts`
- つまり提案はフォルダ自身の名前ではなく子の内容に追従する。**配線前は同じ入力でエラー**（§9.2）だったので、これは Phase 7 で初めて得られた振る舞いである。
- 子がフォルダだけのフォルダ（0 files / 2 folders、孫は読まない） -> `2024-2025`（子フォルダ名が証拠になる）。
- 原典 §5.4 の例（`invoice_001.pdf` / `invoice_002.pdf` / `contract.pdf` / `payment.xlsx` の合成 PDF + xlsx） -> `2026-10-14_合同` / `2026-10-14_pdfファイル群` など。**限界（実測）**: 本ラウンドの合成 PDF は非圧縮（自前生成）なので、Phase 4 のバイト走査（`strings -n 3` と同じ見え方）が PDF の辞書・xref をそのまま拾い、子の抜粋がコンテナの語彙に寄る（実測: `T:%PDF-1.4 1 0 obj << /Type /Catalog ...`）。実際の PDF は内容ストリームが圧縮されているため抽出できる本文はさらに少なく、名前と種別の分布が主な証拠になる。この品質は原典 §14 の Phase 9（Performance / Quality）の評価対象とする。

### 9.4 親子の履歴（D7 / §15 #14）の実測証跡

隔離 HOME `/tmp/kb-p7/e2e4/home`、ツリー `/tmp/kb-p7/e2e4/tree/E-parent`（`child.txt` / `other.txt`）。ユーザーが打つ形の**相対パス**で実行:

```text
$ kaseifu rename tree/E-parent/child.txt --template "renamed-note" --execute
proposal: renamed-note.txt / status: executed / operation_id: op-99300-1791994838666926000
$ kaseifu rename tree/E-parent --template "renamed-folder" --execute
proposal: renamed-folder / directory: 2 entries (2 files, 0 folders), types txt 2 / status: executed
```

- journal（実測、`sqlite3` 相当の Python で直接参照）: `move success | /private/tmp/.../tree/E-parent/child.txt -> /private/tmp/.../tree/E-parent/renamed-note.txt`、`move success | /private/tmp/.../tree/E-parent -> /private/tmp/.../tree/renamed-folder`。**子の行のパスは絶対（canonical）**で、親行が効いている間は `exists() == False`（= 記録どおりには存在しない）。
- `kaseifu undo` -> `restored: /private/tmp/kb-p7/e2e4/tree/E-parent`（`undid op-99357-...` = **親**の行、新しい順）。直後に**子の記録パスが再び実在（True）**する。
- もう一度 `undo` -> `restored: /private/tmp/kb-p7/e2e4/tree/E-parent/child.txt`（子の行）。`ls tree/E-parent` = `child.txt other.txt`（元の名前と場所に戻った）。
- 3 回目 -> `nothing left to undo`（**二重 undo 防止**）。journal の行は**書き換えられていない**（子の行の `source_path`/`destination_path` は実行時のまま、両行に `undone_at` が入る）。
- 拒否の実測（kaseifu の外で改名した場合）: 記録パスを Finder 相当で手動改名した後 -> `undo failed: the recorded path /... no longer exists (a parent folder may have been renamed outside kaseifu); nothing was changed`。

### 9.5 所要時間とサンプリング（実測）

- フォルダ dry-run の wall（生成含む、本機）: 8.7〜17.0 秒（`AI` の生成が支配的）。実測例: 4 子 10.9 秒、260 子 **5.5 秒**、2 子 8.4 秒。`DEFAULT_FM_TIMEOUT`（20 秒、生成のみ）は超えていない。
- 260 子フォルダの実際の出力: `directory: at least 200 entries (200 files, 0 folders), types txt 200`（上限に当たったことを正直に言う）+ `proposal: Page_Scanning`。
- リーダのコスト: `tests/v096` の `dir04`（260 ファイル作成 + 上限付き読み + fake モデルでの計画）が **0.12 秒**。
- 子の内容抽出の質（実測、プローブ）: テキストの子は**文がそのまま抜粋**になる（`T:請求書 invoice 001 ACME GmbH 2026-10-01 合計 1200 EUR 支払期日 2026-10-31`）。非圧縮 PDF の子は辞書/xref が多くを占めるため、**長い順に上位 4 件だけを採る**規則を入れた（§9.6）。
- 同じ 3 フォルダ（PDF/xlsx のみ）でのタイトルの実測比較: 規則を入れる前は `KfSemanticPayload`（モデル自身の型名）が 4 回中 3 回、入れた後は 0 回（`contracts` / `contracts` / `PDF_Files` / `pdfファイル群` / `合同`）。**シグナルを空にすると逆に悪化する**（3 回中 3 回 `KfSemanticPayload` / `KB_Semantics`）ので、子の抜粋は「何も入れない」より「長いものを少数」が良い。

### 9.6 実装中に確定した追加判断（計画書への追記）

- **A1 の修正範囲を広げた**: 計画では `F:`（ファイル名由来）を落とすとしていたが、実測で **Spotlight の `kMDItemTitle` はフォルダではその名前そのもの**（`mdls` 実測: `kMDItemTitle = "B-untitled"`）であり、`normalize` の `M:`/`K:` シグナル経由でフォルダ名がプロンプトに届いていた（実測: フォルダ名をエコーして `status: noop` になった）。フォルダ経路では観測由来のシグナルを**すべて落とし、子から読んだ抜粋だけ**を載せる（`context.semantic_signals.clear()` → 子の `T:` を追加）。
- **子の抜粋は「長い順に上位 4 件」**: PDF のコンテナ語彙（`/Type`、`obj`、`xref`）にタイトルが引きずられる実測があったため（§9.5）、子ごとに長い抜粋を優先して 4 件に制限する。ラウンドロビン統合（全体 12 件）はそのまま。
- **journal は canonical パスを記録する**: ユーザーが相対パスを打つと journal にも相対パスが入り、別の作業ディレクトリからの `undo` で履歴が解決できなかった（実測）。計画（分類・観測・レポートは与えられたパスのまま）はそのままに、**実行対象（`source`/`destination`）だけを canonical 化**して記録する。これで #14 の「履歴が解決できる」が作業ディレクトリに依存しない。既存テストの assert は 1 件も変えていない（v094 の実パス期待値を守るため、この限定を選んだ）。
- **プロンプトの指示行を 1 行追加**: 「タイトルは自然な短い句（ja なら日本語）で、型名・フィールド名を答えない」（§9.5 の実測に基づく）。
- **検収時の修正（親スレッドのレビュー）**: フォルダの signal 注記が `above` と言いながら実際には `signals:` 行より前に置かれていたため、方向に依存しない文（`The signals data is read from files inside the folder, not from the folder's name.`）に直した。配置（2000 文字予算で切られないよう signals 行より前）と v096 が要求する部分文字列は不変。

## 10. リスクと対策

| リスク | 対策 |
|---|---|
| 大規模フォルダで読みが伸びる | 読み 200 件 + 提示 20/10 件 + 内容 3 件 x 32 KiB の固定上限（D2/D3）。実測を §9.5 に記録 |
| フォルダの mtime が動いて実行できない | ディレクトリの同一性は (dev, ino) で判定し、mtime を TOCTOU の根拠にしない（D8） |
| システム領域配下の一時領域が拒否されて機能が使えない | 対象には「根そのもの」だけを拒否し、根の中のユーザーフォルダは許す（D8。destination 規則は不変） |
| `is_excluded_destination` の規則移動で destination の挙動が変わる | 移動するのは規則の実装だけで、`destinations` の判定結果は不変（既存の 14 テストがそのまま通ることをゲートにする） |
| フォルダ名をモデルがエコーして No-op になる | プロンプトから `filename:` 行を外し（D4）、エコー時は `Noop` として正直に報告（W6） |
| 空/混在で誤った名前を提案する | 空はモデル前に保留、混在はモデルに title を空にさせて保留（D6 / W3・W4） |
| 親子の履歴が壊れる | 行を書き換えず、新しい順 undo の不変条件をテストで固定し、説明できない場合は拒否（D7 / W8・W9） |
| 既存の文書・画像・動画の退行 | プロンプトのバイト不変（S2）、`--format json` は追加のみ、完全リテラル以外は既存コードを触らない（N1〜N4） |
| 複雑度ゲート（CC>=16 の新規作成禁止・既存最大の悪化禁止） | 新規は小さな関数に分割（`read_directory_facts` / `refuse_reason` / `plan_directory_rename` / `finish_directory_plan`）。`bash scripts/quality_check.sh` で確認 |
| ヘルプの内部語・ASCII 規約 | `tests/v070_zero_config_p1_help_test.rs` の全走査をそのまま通す（T13 / N3） |
| テストが実ユーザーのフォルダに触る | 隔離 HOME・一時 dir・合成ツリーのみ。`git status` とテスト内の書き込み先の grep で確認（D10 / N4） |
