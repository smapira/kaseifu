# AIリネーム Phase 5b（Vision 知覚層の追加）実装計画

## 0. 位置づけと進め方

- 原典（読み取り専用・編集禁止）: `documents/plans/user/20261014a.md`（AIリネーム開発指示書 v2.0）§4.3・§5.2・§11.3・§12。`20261014b.md`（TREC）は別プログラムで対象外。
- 前提: Phase 5 は着地済み（HEAD `b9b7e68`、計画は `documents/plans/dev/20261014j-ai-rename-p5.md`、§11.9 が本ラウンドの予告）。ただし **画像の内容がモデルに届いていない**（SDK に画像入力 API が無い）ため、`IMG_1234.jpg` のようなファイル名エコー提案になる。本ラウンドは知覚層を Apple Vision で補い、Foundation Models は日本語タイトル化に使うハイブリッドを追加する。
- 進め方: BDD タスク分解 → plan-architect レビュー（issue 記録）→ implementer → reviewer → QA → 検収。commit / push / tag はしない（親スレッドが実施）。
- スキル運用の注記: 本環境のスキルカタログに `/researcher` `/plan-architect` `/implementer` `/reviewer` `/qa` が無いため（Phase 3+4・5 と同一条件）、各工程の手順を直接なぞって実施し、根拠を本計画書と issues に残す。
- ツール利用実績: `read_file` で `src/semantic/{mod,contract,provider,bridge,ffi,fake,ffi_stub}.rs` / `src/rename/{image,rules,service,validate}.rs` / `src/models/file_context.rs` / `src/normalize/mod.rs` / `src/main.rs` / `swift/kaseifu_fm_bridge.swift` / `build.rs` / `schemas/fm-rename-v1.json` / `tests/v09{0,2,3}*` / `tests/phase4_context_test.rs` / `src/discovery/search_terms.rs` / `scripts/quality_check.sh` を確認、`terminal` でベースライン再実測・Swift/ImageIO/Vision のプローブ（typecheck・実ラベル・所要時間・autolink）・`git` 状態・旧バイナリの退避、`grep` で rename 配線・シンボル・構造体リテラルを実測。LSP / serena / context7 / codebase-memory-mcp / graphify は本環境に無いため read_file + grep + terminal + fetch で代替した。

## 1. ベースライン（実測・2026-10-14）

- HEAD: `b9b7e68`（`origin/main` と同期、`git status --short` 空 = clean）。
- `cargo test --all-targets --all-features` = **664 passed / 0 failed**（本スレッドで再実測。ログ `/tmp/kb-test.log`）。
- `cargo build` 成功。**配線前の比較用バイナリを `/tmp/kb-probe/kaseifu-pre` へ退避した**（HEAD の実装のまま。D6 の前後比較に使う。repo 内には置かない）。
- 環境実測: macOS 26.4.1、Swift 6.3.3、SDK 26.5。Apple Intelligence 有効（実推論可）。
- テスト版数接頭辞: `tests/` は `v090`〜`v093` が使用済み。新規結合テストは `v094_*` とする。
- 知覚層の実測プローブ（`/tmp/kb-probe`、合成画像のみ・実ユーザー写真は使わない）:
  - `import Vision` + `VNClassifyImageRequest` + ImageIO thumbnail の typecheck 成功。SDK に `Vision.framework`（`Vision.tbd`）あり。
  - 実行時の実ラベル: 1024x768 の合成夕景（SVG レンダ）→ `outdoor 0.106, celestial_body 0.105, moon 0.105, daytime 0.042, sun 0.042, sky 0.039, sunset_sunrise 0.038, ...`。注目すべきは **確信度が絶対値として低い**（最大 0.106）こと。分類は上位ほど意味がある。
  - 所要時間: 長辺 2048 の thumbnail 経由で **0.078〜0.385 秒**（ウォーム 0.078〜0.139 秒）。
  - autolink 実測: `import Vision` / `import ImageIO` した object の `LC_LINKER_OPTION` に `-framework Vision` / `-framework ImageIO` が入る（build.rs の既存リンク手順でそのまま通る）。
  - OCR 実測（判断材料）: 看板風の合成画像で `OPEN 1.0` / `9:00 - 18:00 1.0` を 0.397 秒で取得。動くが今回は送る（D5）。

## 2. 利用する土台の実測再確認（file:line）

| 土台 | 実測 | Phase 5b での使い方 |
|---|---|---|
| `src/semantic/mod.rs:19-31` | contract → ffi → bridge → provider の層構成。`ffi_stub` は cfg で切替 | 知覚もこの層に足す（新モジュール `perception.rs` ＋ ffi 追加） |
| `src/semantic/ffi.rs:65-89,131-133,241-261` | raw C ABI（callback 型 generate と、値を返す `kaseifu_fm_available`）。`take_string`＋`OwnedResult` が所有権を処理 | 知覚は短時間・同期なので `kaseifu_fm_available` と同じ「文字列を返す」形の ABI にする（新規 unsafe はこの module 内に閉じる） |
| `src/semantic/contract.rs:20-35,83-111,337-361` | 境界の bound 定数、`FmInput`/`FmImageFacts`、`clean_optional`/`bound_chars`。未知フィールド無視 | `FmInput.perception`（任意）を互換拡張で足す。既存キーは不変（D2） |
| `src/semantic/provider.rs:190-231,235-267` | `build_request` が bound 済みで組立。`image_facts` が観測（context）→契約（FmImageFacts）を写す | 同じ形で `context.perception` → `input.perception` を写す（二重実装なし） |
| `src/semantic/bridge.rs:55-98` | `FmBridge` trait（available / generate）。`SwiftFmBridge` が本番 | `perceive` を同じ trait に足す（テストの注入口が 1 つのまま。D1） |
| `src/semantic/fake.rs:37-144,191-232` | `FakeFmBridge`（応答・request 記録）、`FakeSemanticProvider` | 知覚の Fake（ラベル・失敗・呼び出し記録）を追加（D4） |
| `src/semantic/ffi_stub.rs:29-58,144-153` | Swift 不在ビルドの同一 C ABI。常に unavailable＋REASON | `kaseifu_vision_perceive` を足し、同じ REASON で縮退（知覚なし＝Phase 5 と同じ提案） |
| `src/models/file_context.rs:67-81,128-159` | `NormalizedFileContext`（`image_metadata: Option<ImageMetadata>` を skip 直列化） | `perception: Option<PerceptionInfo>` を同じ流儀で追加（観測モデルに知覚を載せる） |
| `src/normalize/mod.rs:110-130` | 本番の `NormalizedFileContext` 完全リテラル | 新フィールドの `None` を追加（1 箇所） |
| `src/rename/service.rs:172-194,361-427,429-458` | `plan_image_rename`（本文読みなし・ファイル名 injection 検査のみ）→ `analyze` → `finish_plan`／`rule_inputs` | 画像分岐に「知覚 → context へ格納 → analyze」を挿入。文書分岐は無変更。`finish_plan`/`execute_plan`/journal/undo は共用のまま |
| `src/main.rs:1122-1171,1263-1300` | `RenameReport`＋`emit_rename_report`（text 行と JSON キー） | 知覚ラベル行を追加（JSON は追加のみ） |
| `swift/kaseifu_fm_bridge.swift:182-231,234-265,370-374` | `kfBuildPrompt`（target_type==image の 2 行 gate）、`kfRunGeneration`、`free_result` | 知覚 ABI を追加し、`input.perception` があるときだけプロンプトに 1 行足す（文書プロンプトはバイト不変） |
| `build.rs:31-64,152-165,168-232` | FM SDK プローブで Swift/stub を切替、`-disable-autolink-framework FoundationModels`＋弱リンク | 変更しない（Vision は object の autolink で入る。実測済み） |
| `schemas/fm-rename-v1.json` | envelope の oneOf のみ（request は required キー列挙） | `perception` を任意プロパティとして明文化（必須化しない。D2） |

## 3. スコープ

### 3.1 対象（今回実装する）

- 知覚層: 新 C ABI `kaseifu_vision_perceive(path)`（Swift が ImageIO で読み、`VNClassifyImageRequest` で分類）。画像バイトは FFI を越えない（Rust はパスだけ渡す）。
- 契約: `fm-rename-v1` の互換拡張として `input.perception.labels[]` を追加（任意・既存キー削除なし）。知覚応答の envelope も同じ `schema_version` を使う。
- 配線: 画像プランで「知覚 → `input.perception` → Foundation Models（日本語タイトル）→ Rust の命名規則」を成立させる。知覚結果は dry-run 出力と `--format json` にも出し、効き目を実測できるようにする。
- 縮退: 知覚が使えない/失敗する環境では Phase 5 と同じ提案に戻り、理由を明示する（黙って落とさない）。
- テスト: `tests/v094_ai_rename_p5b_vision_test.rs`（Fake 注入・一時 dir・HOME 隔離）。実ラベル確認は `#[ignore]` の実機テストと CLI 手動に分離。
- 実機検証: 合成画像で dry-run / `--execute` / `undo` を実行し、**配線前バイナリ（`b9b7e68`）との提案名比較**を記録する。

### 3.2 対象外（着手しない）

- Phase 6（動画・AVFoundation）・Phase 7（フォルダ）・Phase 8（一括）。
- 画像バイトの FFI 越境、Rust 側の画像デコード実装、新規 Rust crate、新規設定ファイル・別 DB・独自キャッシュ・外部プロセス。
- `VNRecognizeTextRequest`（OCR）: 今回は送る（D5 に実測と次回条件）。
- 人物記述の制限（ユーザー決定で解除済み。§5.3 の 1 行と「不確実な断定禁止」は維持）。
- 既存コマンド（organize / watch / classify / inspect / doctor / undo / uninstall / bench、rename 文書系）の挙動変更。`--format json` の既存キー削除。`discover` の公開。
- README 等の利用者文書の整備（リリース時にまとめて）。

## 4. 設計判断（選択肢と結論。黙って決めない 6 点）

### D1. 知覚層の API 形（Swift 内で同居 vs 別 C ABI）

- 選択肢A（不採用）: `kaseifu_fm_generate` の内部で Swift が Vision も実行し、応答にラベルを載せる。FFI 追加ゼロで魅力的だが、(a) Rust からラベルが見えず、**「ラベルが契約に載ってモデルへ渡った」ことを Rust のテストで固定できない**（ソース文字列の grep に堕ちる）、(b) Rust 側の bound（件数・文字数）が効かない、(c) 観測が応答側に混ざり、契約の向き（観測を送る → 意味を受け取る）が濁る。
- 選択肢B（採用）: 新 C ABI `kaseifu_vision_perceive(image_path) -> JSON`（同期・文字列返却）。Rust が知覚結果を受け取り、bound してから生成を依頼する。理由: 既存の層構成（contract=境界の型と bound / ffi=唯一の unsafe / bridge=JSON in-JSON out の注入口 / provider=観測→契約）にそのまま乗る。既存の `kaseifu_fm_available`（値を返す同期 ABI）と同じ所有権・解放規約を再利用でき、新規 unsafe は `ffi.rs` に閉じる。`FmBridge` の注入口が 1 つのままなので、テストは Fake で完全に決定的になる。
- 選択肢C（不採用）: FFI で画像バイトを渡す。スコープで明示的に禁止。
- 帰結: Swift 側は「ファイルシステムを書かない」原則のまま、**Rust が渡した画像 1 枚を read-only で読む**（ヘッダのコメントを「書かない／渡された画像を読むだけ」に更新する）。

### D2. 契約のバージョン（v1 互換拡張 vs v2）と知覚ラベルの上限・確信度の扱い

- 選択肢A（採用）: **`fm-rename-v1` の互換拡張**で足りる。根拠（実測）: (a) 追加は任意フィールド `input.perception` のみで既存キーの削除・型変更なし、(b) 契約は両方向とも未知フィールドを無視する（`contract.rs:9-12` の規約、`tests/v093` の未知フィールド無視テスト）、(c) 知覚応答も同じ `schema_version` で扱える（availability と同じ envelope 形）。v2 は「送る物が変わるのに 3 点（Rust・Swift・schema）を同時に上げる」だけで、差分に見合わない。
- 選択肢B（不採用）: `fm-rename-v2`。将来削除・改名を伴うときのための温存とする。
- ラベル上限: **最大 12 件**（`MAX_PERCEPTION_LABELS`）、**1 ラベル 64 文字**（`MAX_PERCEPTION_LABEL_CHARS`）、確信度降順、**床 0.01**（`MIN_PERCEPTION_CONFIDENCE`）。根拠: Vision の分類は絶対確信度が低く（実測で最大 0.106）、件数上位がそのまま意味上位になる。床 0.01 は実測で長い裾（0.002 以下）を落とすためだけの下限で、0.9 級の `document/map` も 0.03 級の `chart/diagram` も残る。12 件は「同点 0.909 が 2 件、0.197 が 2 件」のような重複帯を飲み込める件数であり、プロンプトの 1 行（2000 文字 budget 内）に収まる。
- **確信度はプロンプトに載せない**（ワイヤには載せる）: 実測値 `sky 0.039` をそのまま渡すと、既存の「不確実な内容を断定しない（空欄にする）」規則と衝突してタイトルが空になる危険がある。順位付きのラベル列だけを渡し、確信度は表示・テスト・監査のために Rust に保持する。
- 未知の `status`/`kind` は明示エラー（無言の成功にしない）。パース規則は availability と同じ流儀。

### D3. 縮小の上限と Vision 入力の適正

- 選択肢A（採用）: **長辺 2048 px**、アスペクト比保持（Phase 5 の D2 で確定済みの ImageIO レシピを実装へ昇格）。`kCGImageSourceShouldCache=false` の read-only open、`kCGImageSourceCreateThumbnailFromImageAlways=true`、`kCGImageSourceCreateThumbnailWithTransform=true`、`kCGImageSourceThumbnailMaxPixelSize=2048`。根拠: 2048^2x4 byte ≒ 16 MiB のメモリ bound、実測で 0.078〜0.385 秒、元画像を変更しない。
- 選択肢B（不採用）: 縮小なし。100MP 級でメモリと時間が跳ねる。
- 選択肢C（不採用）: 長辺 1024。小物・細部のラベルが落ちる可能性がある。
- 選択肢D（不採用）: バイト数上限（例 32 MiB）も併用。ImageIO の thumbnail はデコード後の縮小なのでバイト上限は効果が薄く、入力の拒否条件を増やすだけ。代わりに「開けない／サムネイルが作れない」を明示エラーにする。

### D4. テスト戦略（非決定的な Vision 出力をどう扱うか）

- 選択肢A（採用）:
  - 知覚境界は `FmBridge::perceive` の **Fake で固定**する（ラベル列・失敗理由・呼び出しパスを記録）。契約（envelope・bound・床・整列）は文字列 JSON の単体テストで固定する。
  - **実ラベルは `#[ignore]` の実機テスト**に分離する（`cargo test -- --ignored --nocapture`）。合成画像はテスト内の最小 PNG ライタで決定的に生成し、`assert` は「3 件以上・空でない・重複なし・確信度降順・64 文字以内」に留める（ラベル名そのものは OS の Vision リビジョンで揺れるため固定しない）。生成したファイルは一時 dir に置き、パスを `println!` するので CLI 手動検証にもそのまま使える。
  - 実モデルの end-to-end（ラベル → 日本語タイトル）は CLI 手動（§9）で記録する。
- 選択肢B（不採用）: 実ラベル名を assertion に固定する。OS 更新で壊れるテストは資産にならない。
- 選択肢C（不採用）: Vision を呼ぶテストを通常のスイートに入れる。CI（モデル・Vision 無し環境）で落ちる。

### D5. OCR（`VNRecognizeTextRequest`）を今回入れるか

- 選択肢A（採用）: **次回送り**。根拠: (1) 原典は OCR を「必要時」（§5.2 の表、§11.3 の「不要な OCR を実行しない」）と条件付きにしている。(2) 動くことは実測済み（0.397 秒で `OPEN` / `9:00 - 18:00` を取得）だが、画像内容理解と命名改善という本ラウンドの目的は分類ラベルだけで達成できる。(3) OCR は「画像内の文字」を新たにモデルへ渡すことになり、ファイル名と同種の **注入面**（画像に書かれた指示文）が 1 つ増える。本ラウンドで分類ラベルの配線・縮退・bound を固定してから、需要と注入対策をセットで設計するのが順序として正しい。
- 選択肢B（不採用）: 今回入れる。ABI・契約・プロンプト・テストが同時に膨らみ、優先順位（1→4）の最後に置かれている作業で本ラウンドの検証時間を削る。
- 次回条件: 分類ラベルの配線が着地した後、看板・書類写真・スクリーンショット等の需要を実測し、`kaseifu_vision_perceive` の応答に `text` 配列を足す互換拡張（schema_version は据え置き）として着手する。

### D6. 提案名が知覚情報で改善したことをどう示すか

- 選択肢A（採用・前後比較）: **同一の合成画像・同一オプション**で、配線前バイナリ（HEAD `b9b7e68` を `/tmp/kb-probe/kaseifu-pre` に退避済み）と配線後バイナリの提案名を並べる。ファイル名は `IMG_1234.png`（意味を持たない）とし、配線前はファイル名エコー、配線後はラベル由来の日本語タイトルになることを示す。加えてテストで「ラベルが `input.perception` に載る」「文書プランでは知覚を呼ばない」「知覚失敗でも提案が成立する」を固定する。
- 選択肢B（不採用）: 定性のみ（「良くなった気がする」）。前後比較が最も強い証跡。
- 選択肢C（不採用）: 実ユーザーの写真を使う。禁止事項。
- 追加の安全網（**ラベルそのままのファイル名にしない**）: モデルの `title` が知覚ラベルと（大小・区切りを正規化して）一致した場合は、ラベルの丸写しとみなして提案を保留する（理由を表示）。原典の「命名規則の適用は Rust 側／AI は意味情報を補完」を満たしつつ、退化ケースを決定的に検出できる。実機で誤検出が出た場合はその実測を記録して判断する。

## 5. BDD シナリオ（振る舞いと具体例）

### 知覚境界（`src/semantic/perception.rs` / ffi）

- P1 正常: `{"schema_version":"fm-rename-v1","status":"success","perception":{"labels":[{"label":"sky","confidence":0.039},{"label":"sunset_sunrise","confidence":0.038}]}}` → `PerceptionInfo{labels:[sky, sunset_sunrise]}`。
- P2 整列と bound: 15 件（0.001〜0.9）を渡す → 確信度降順・床 0.01 以上・最大 12 件・各 64 文字以内・空文字と重複は落ちる。
- P3 失敗の分類: `status:error, kind:unavailable, reason:vision-framework-missing` → `Unavailable{reason}`。`kind:image-unreadable` → `Failed{detail}`。壊れた JSON・版違い・未知 status → `Malformed`。
- P4 実機（`#[ignore]`）: 合成 PNG（夕景・1024x768）→ ラベルが 3 件以上、空でない、降順。
- P5 縮退（stub ビルド）: `KASEIFU_FM_FORCE_STUB=no-sdk` のスイートで知覚は `Unavailable{reason:foundation-models-sdk-missing}` になり、プランは Phase 5 と同じ提案を出す。

### 契約（`src/semantic/contract.rs`）

- C1 画像要求が `input.perception` を運ぶ: `FmRequest` を serialize して `input.perception.labels[0].label` が読める。`input.image` のキー集合は不変（`camera/capture_time/height/width`）。
- C2 後方互換: `perception` の無い要求・未知フィールド付き要求が parse できる。`perception` の型が違う（数値・文字列）場合は欠損扱いで、推測しない。
- C3 文書要求は `perception` を運ばない（キー自体が出ない）。

### 配線（`src/rename/service.rs`）

- W1 画像プラン: Fake 知覚（`sun/sky/sunset_sunrise`）＋ Fake モデル（title `夕焼けの空`）＋ P01 → 提案 `夕焼けの空.png`、request の `input.perception.labels` に 3 件、plan の知覚ラベルも 3 件、`date_source` は従来規則（EXIF→created→modified→now）。
- W2 文書プラン: Fake 知覚の呼び出し回数 0、request に `perception` キーなし。
- W3 注入ファイル名: 知覚は呼ばれない（推論を浪費しない）。
- W4 知覚失敗: `Unavailable(reason)` でも提案が成立し、`perception: unavailable(<reason>)` が出力に出る（Held にしない）。
- W5 ラベル丸写し: title が `sunset_sunrise`（ラベルと一致）→ `Held`＋理由「perception label」。比較は trim・小文字化・`_`/空白連続を 1 空白に正規化した等値のみ（部分一致は見ない。plan-architect A4）。title が `夕焼けの空` なら通る。
- W6 非破壊: 拡張子不変・stem 80 字・単語境界・アンダースコア・上書き禁止・No-op・symlink 拒否・Journal 先行・undo（Phase 5 の I2〜I9 を画像で再確認）。

### Swift 側

- S1 プロンプト: `input.perception.labels` があるときだけ `visual_labels:` 行（`<data>` 内）が入る。位置は `signals:` 行より前（`kfMaxPromptChars=2000` の切詰めで知覚が消えないため。plan-architect A1）。文書プロンプトはバイト不変（既存の画像 gate 数 2 を維持）。
- S2 ABI: 存在しないパス → `status:error` の JSON（プロセスは落ちない）。ディレクトリ・非画像 → 同じ。
- S3 所有権: 知覚の戻り値も `kaseifu_fm_free_result` で解放（既存規約の再利用）。

### 非破壊（全ラウンド共通）

- N1: 既存 664 テストが全て通る。既存コマンドの挙動不変（文書プロンプトのバイト一致を含む）。
- N2: `cargo fmt --check` 違反 0、clippy 警告 0、新規 CC>=16 なし、既存最大 CC を悪化させない、complexity ゲート PASS。
- N3: 出力 ASCII のみ（生成ファイル名の非 ASCII を除く）・em dash なし・TUI なし・NO_COLOR/非 TTY 準拠。JSON は追加のみ。
- N4: テストは隔離 HOME・一時 dir・注入のみ。実ユーザーの写真・`~/Pictures/`・`~/Desktop/`・実 Keychain・実補完 dir を読み書きしない。実ファイル移動は一時 dir 内の合成ファイルのみ。秘密情報を混ぜない。

## 6. タスク一覧（実装順・直列）

- T1 `src/models/file_context.rs`: `PerceptionInfo{labels}`・`PerceptionLabel{label,confidence}` と `NormalizedFileContext.perception`（`skip_serializing_if`/`default`）。`src/normalize/mod.rs:117` と `src/discovery/search_terms.rs` の完全リテラル 2 箇所に `None` を足す。
- T2 `src/semantic/perception.rs`（新規）: bound 定数 3 つ、`PerceptionError`（Unavailable/Failed/Malformed）＋Display、`parse`（envelope・版・status・kind）、bound 適用（降順・床・件数・文字数・空と重複の除去）。P1〜P3 の単体テスト。
- T3 `src/semantic/contract.rs`: ワイヤ型 `FmPerception`/`FmPerceptionLabel` と `FmInput.perception`（任意・skip）。`PerceptionInfo` からの変換。C1〜C3。
- T4 `src/semantic/ffi.rs`: `raw::kaseifu_vision_perceive`＋安全ラッパ `vision_perceive(path)`（NUL 混入・null・非 UTF-8 を明示エラーに）。S3。
- T5 `src/semantic/ffi_stub.rs`: 同じ ABI を `unavailable`＋REASON で実装（P5）。
- T6 `src/semantic/bridge.rs`: `FmBridge::perceive` を追加し、`SwiftFmBridge` が ffi＋perception::parse で実装。モジュール doc の層図を更新（知覚を含む）。
- T7 `src/semantic/fake.rs`: `FakeFmBridge` に知覚設定（ラベル／失敗／未実装）と呼び出しパス記録を追加。`FakeSemanticProvider` に知覚設定を追加（既定は unavailable）。
- T8 `src/semantic/provider.rs`: `SemanticProvider::perceive` を追加、`FoundationModelsProvider` が bridge へ委譲。非画像パスは `is_image` で弾いて `Failed("not an image")` を返す（plan-architect A2）。`build_request` が `context.perception` を bound して `input.perception` へ写す。C1/C3。
- T9 `swift/kaseifu_fm_bridge.swift`: `import ImageIO`（＋`canImport(Vision)` gate）、`kaseifu_vision_perceive` 実装（2048 thumbnail → `VNClassifyImageRequest` → 降順・上限 12・床 0.01 → 同一 envelope）、`kfBuildPrompt` に `visual_labels` 行（`input.perception` があるときのみ・**`signals:` より前**。A1）。ヘッダの「ファイルシステムに触れない」記述を「書かない／渡された画像を read-only で読む」に更新。S1/S2。
- T10 `src/rename/service.rs`: 画像分岐に知覚（injection 検査の後・analyze の前）を挿入し `context.perception` へ格納。`PerceptionReport`（Labels/Unavailable）を `RenamePlan` に追加。ラベル丸写しの保留（D6）。W1〜W6。
- T11 `src/main.rs`: `RenameReport` に知覚行（`perception: sky(0.039) sunset_sunrise(0.038) ...`・最大 6 件＋`(+N more)`）と JSON キー（`status`/`labels`/`reason`）を追加。文書・Held/Noop では出さない。
- T12 `schemas/fm-rename-v1.json`: `perception` を任意プロパティとして明文化（required に入れない）。既存の schema テストは不変で通る。
- T13 `tests/v094_ai_rename_p5b_vision_test.rs`（新規）: P1〜P3・C1〜C3・W1〜W6・S1（Swift ソースの gate 検査）・最小 PNG ライタ・`#[ignore]` 実機テスト（P4）。
- T14 実機検証（テストに含めない。§9 に記録）: 旧バイナリとの前後比較、dry-run / `--execute` / `undo`、所要時間、Vision ラベル、`--format json`、文書と動画の非退行。
- T15 検収: `cargo test --all-targets --all-features`（通常＋stub ビルド）、`cargo fmt --check`、clippy、`bash scripts/quality_check.sh`、`git diff` 点検（commit しない）。

## 7. 検証手段（実測で示す）

- `cargo test --all-targets --all-features`（664＋新規の pass/fail 数）。
- `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --all-targets --all-features`（SDK に Foundation Models / Vision が無い環境の縮退を含む）。
- `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`bash scripts/quality_check.sh`（OVERALL と complexity 表）。
- `cargo test --test v094_ai_rename_p5b_vision_test -- --ignored --nocapture`（実機 Vision の実ラベルと所要時間）。
- 実機 CLI（隔離 HOME・一時 dir・合成画像のみ）: 旧バイナリと新バイナリの同一入力比較、`--format json`、`--execute`＋journal、`undo --last 1`＋二重 undo、所要時間。
- `git status --short` と `git diff --stat` の目視（無関係変更・秘密情報なし）。

## 8. 受け入れ基準（原典 §15 のうち本ラウンド分）との対応

| 原典 | 本ラウンドでの担保 |
|---|---|
| §4.3 画像対応（画像の内容理解とタイトル生成） | 知覚ラベル（Vision）→ `input.perception` → Foundation Models が日本語タイトル、という経路を実機で示す（W1・§9） |
| §5.2 解析情報（画像内容=FM、撮影日時・機種=ImageIO/EXIF、解像度=ImageIO、OCR=Vision 必要時） | 内容理解は Vision 知覚＋FM タイトル化に置換（SDK 制約の実測に基づく）。日時・機種・解像度は既存経路のまま。OCR は D5 の理由で次回 |
| §5.2 処理（ImageIO→EXIF、FM→理解、アスペクト比保持・縮小・元画像不変） | ImageIO の thumbnail レシピ（2048・read-only）を実装。元画像は書き換えない（テストと実機で確認） |
| §12（不明確な内容を断定しない、デコード失敗=スキップ、読み取り失敗=スキップ） | Swift の断定禁止規則を維持。知覚失敗はプラン全体を落とさず「知覚なし」で継続し理由を表示 |
| §15-3/4/5/6/10/11/12/13/17（リネーム・命名規則・日付・テンプレート・dry-run 既定・上書き禁止・Journal・Undo・既存非破壊） | Phase 5 の検証を画像で再確認し、既存 664 テストを維持 |

## 9. 検収記録（追記式。実装後に実測値を埋める）

### 9.1 ベースラインとテスト（実測）

- ベースライン: 664 passed / 0 failed（HEAD `b9b7e68`）。
- 実装後 `cargo test --all-targets --all-features` = **686 passed / 0 failed**（新規 22 件: v094 統合 17 件（うち `#[ignore]` 1）+ perception 単体 4 + contract 2、既存 664 は維持）。
- `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --all-targets --all-features` = **686 passed / 0 failed**（Swift 不在ビルドの縮退でも同じスイートが通る）。
- `cargo fmt --all -- --check` 違反 0。`cargo clippy --all-targets --all-features -- -D warnings` 警告 0。
- `bash scripts/quality_check.sh` = **OVERALL: PASS**。complexity: Functions 1201、CC<=10 1190（99.1%）、CC 11-15 = 11、**CC>=16 = 0**、最大 CC 15（ベースラインと同じ）、baseline-gate PASS、coverage 82.80%（report-only）。
- `git status --short` は変更 15 ファイル＋新規 4 ファイル（計画・plan-architect issue・`src/semantic/perception.rs`・`tests/v094_*`）。`Cargo.toml` / `Cargo.lock` 無変更（新規 crate なし）。

### 9.2 実機 Vision（`#[ignore]` テスト、合成画像のみ）

- 実行: `TMPDIR=/tmp/kb-verify/tmp cargo test --test v094_ai_rename_p5b_vision_test -- --ignored --nocapture`
- 素材: 手書き PNG ライタで生成した合成夕景 **1024x768 / 2,360,312 byte**（zlib stored・CRC32・adler32。外部ツール・ネットワーク・新規 crate なし）。
- 結果: ラベル **12 件**（上限どおり）。`outdoor 0.129, celestial_body 0.129, moon 0.129, recreation 0.039, sport 0.039, ping_pong 0.037, sky 0.028, sports_equipment 0.026, ball 0.026, sunset_sunrise 0.026, daytime 0.021, sun 0.021`。降順・重複なし・64 文字以内。
- 所要時間: **0.155〜0.625 秒**（プロセス初回 0.625、ウォーム 0.155〜0.178）。
- 縮退の実機確認: 画像でないバイト列を `BROKEN_0001.png` として渡すと `image-undecodable`（ImageIO がサムネイルを作れない）。プランは継続し `perception: unavailable (failed: image-undecodable: ...)` を表示する（§9.4）。

### 9.3 CLI 実機（隔離 HOME=/tmp/kb-verify/home、一時 dir、合成ファイルのみ）

- **配線前後の比較（同一ファイル `IMG_1234.png`・同一オプション `--pattern P02`）**:
  - 配線前（HEAD `b9b7e68` のバイナリ `/tmp/kb-probe/kaseifu-pre`）: `proposal: 2026-10-14_IMG_1234.png`（**ファイル名エコー**、1.19 秒）。`--pattern P01` では title がファイル名そのもので `status: noop`。
  - 配線後（本実装）: `2026-10-14_月.png`（14.91 秒 / 17.22 秒）、`2026-10-14_月のアウトドア風景.png`（8.01 秒）。**ラベル（moon / celestial_body / outdoor / recreation）由来の日本語タイトル**で、ファイル名エコーではない。
  - dry-run の text 出力には知覚行が出る: `perception: outdoor(0.129) celestial_body(0.129) moon(0.129) recreation(0.039) sport(0.039) ping_pong(0.037) (+6 more)`（表示は 6 件＋残数。JSON は 12 件全部）。
- `--format json`: `rename` オブジェクトのキーは `current_name, date_source, operation_id, pattern, perception, proposal, reason, status, target`（**既存キーは全て保持、追加のみ**）。`perception.status = "labels"`、12 件。文書では `perception: null`。
- execute / undo: `EXEC_0001.png` を `--execute` → `proposal: 月夜のボール投げ.png`、`operation_id: op-92374-1971978514555710000`、`status: executed`（journal `move` 行 success、4.61 秒）。`undo --last 1` で `EXEC_0001.png` に復帰し、二重 undo は `nothing left to undo` で停止。
- 非退行: 文書（`note.txt`）は知覚行なしで従来どおり提案。動画（`clip.mp4`）は `Error: .mp4 is a video; video rename arrives with Phase 6`。EXIF カメラの無い JPEG の P11 は `reason: missing: device` で保留（捏造しない）。
- 所要時間の総括: 画像 dry-run は 8.0〜17.2 秒（知覚 0.2〜0.6 秒＋生成）。20 秒の生成 timeout は超過していないが、プロンプト調整中の 1 回だけ wall 20.2 秒を観測した（生成自体は timeout 未満で完走）。**マージンは薄い**ため、次回 timeout 超過を観測したら根拠つきで `DEFAULT_FM_TIMEOUT` を見直す（§10）。

### 9.4 知覚層が効いたことの証跡（要約）

- 同じ合成画像で、配線前は `2026-10-14_IMG_1234.png`（エコー）、配線後は `2026-10-14_月.png` / `2026-10-14_月のアウトドア風景.png`（ラベル由来の日本語）。
- 知覚を外した場合（`BROKEN_0001.png`）は `perception: unavailable (...)` ＋ `2026-10-14_BROKEN_0001.png`（Phase 5 と同じエコー）に戻る。つまり差は知覚層が生んでいる。
- ラベルは request の `input.perception.labels` に載る（Fake テストで固定）が、**ファイル名には直接使わない**: 命名規則は Rust（P01/P02 等）で、名前はモデルの title から作る。

### 9.5 プロンプト調整の記録（実測に基づく 1 回の反復）

- 初版（`visual_labels` を ` | ` 結合＋「そのまま出すな」）: title が `天体_|_月_|_スポーツ_equipment_|_スポーツ_|_球_|_日光_|_太陽`（区切りごとラベルを羅列）。
- 第2版（`,` 結合＋「短い自然な句」）: `outdoor_recreation`（英語識別子の連接）。
- 第3版（日本語語彙で書く・識別子をそのまま出さない・ファイル名を繰り返さない）: `月` / `月明かり` / `月のアウトドア風景` と安定。最終版を採用（差分は Swift のプロンプト 3 行のみ）。
- 注記: この合成画像は分類器にとっても曖昧（moon/ping_pong/ball が同居）であり、実写真でのラベルはより具体的になる。本ラウンドの検証は「エコーからの脱却」と「配線」の実証に限る。

### 9.6 副次的に確認した挙動

- 文書プロンプトはバイト不変（Swift の差分は `input.perception` がある場合のみ発火するブロックと perception ABI／ヘッダコメントに限る。既存テストの画像 gate 数 2 も不変）。
- 知覚は画像分岐の注入検査の後にだけ呼ばれる（poisoned 名では知覚も推論も 0 回）。
- 知覚はモデル利用不可でも先に走る（順序上不可避）。実測コストは 0.2〜0.6 秒で bounded。

## 10. リスクと対策

| リスク | 対策 |
|---|---|
| Vision のラベルが画像依存で非決定的 | テストは Fake で固定し、実ラベルは `#[ignore]`＋実測記録のみ。assert は件数・順序・長さに限定 |
| 知覚の失敗がリネームを止める | 知覚は「あれば使う」層として設計し、失敗は `Unavailable(reason)` の縮退にして提案を継続（W4） |
| ラベルをそのまま名前にする退化 | プロンプトで「翻訳・解釈し、ラベルをそのまま出さない」と指示し、Rust 側で丸写しを保留（D6） |
| プロンプト注入（画像内・ファイル名） | ファイル名は既存の保留規則を維持。ラベルは分類識別子で `<data>` 内の値として渡す。OCR は注入面が増えるため次回（D5） |
| Vision のメモリ・時間 | 長辺 2048 の thumbnail（16 MiB bound）。実測 0.078〜0.385 秒。FM の timeout（20 秒）は生成のみに適用 |
| 文書経路の退行 | 知覚は画像分岐のみ。文書プロンプトのバイト一致と `perception` キー不在をテストで固定（C3/W2/S1） |
| 既存テストの型変更 | `NormalizedFileContext` の完全リテラル 3 箇所のみ `None` を追加。`SemanticInfo` は変更しない（既存テストのリテラルを壊さない） |
| モデル利用不可でも知覚 0.1〜0.4 秒を使う | 知覚は request を組む前に必要なため前段に置く（`availability()` の事前確認はしない。失敗の観測は従来どおり analyze の `ModelUnavailable` で同じ）。コストは bounded であり実測値を §9 に残す（plan-architect A3） |
| 知覚がプロンプト切詰めで消える | `visual_labels` 行を `signals:` 行より前に置き、2000 文字 budget の末尾切詰めの影響を受けないようにする（A1。実装時に一度取り違え、v094 のソース検査で検出して修正した） |
| 生成時間が 20 秒 timeout に近い | 実測: 画像 dry-run の wall は 8.0〜17.2 秒（知覚 0.2〜0.6 秒を含む。プロンプト調整中に 1 回 wall 20.2 秒を観測、生成自体は完走）。timeout は変更しないが、超過（`skipped: Timeout`）を観測した時点で根拠つきに 30 秒などへ見直す条件付き繰り越し |
| ラベル名が OS の Vision リビジョンで揺れる | テストは件数・順序・長さのみを固定し、ラベル名は断言しない（実ラベルは §9.2 に記録） |
