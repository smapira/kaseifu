# AIリネーム Phase 6（Video Rename）実装計画

## 0. 位置づけと進め方

- 原典（読み取り専用・編集禁止）: `documents/plans/user/20261014a.md`（AIリネーム開発指示書 v2.0）§4.4・§5.3・§11.4・§12・§13.4。`20261014b.md`（TREC）は別プログラムで対象外。
- 前提: Phase 5b は着地済み（HEAD `440719c`、タグ `v0.9.0`）。計画は `documents/plans/dev/20261014j-ai-rename-p5.md` / `20261014k-ai-rename-p5b-vision.md`。知覚は「Vision で見る -> Foundation Models で日本語化」のハイブリッドを確立済みで、**本ラウンドはその設計をそのまま流用する**（新しい知覚経路を二重に作らない）。
- 進め方: BDD タスク分解 -> plan-architect レビュー（issue 記録）-> implementer -> reviewer -> QA -> 検収。commit / push / tag はしない（親スレッドが実施）。
- スキル運用の注記: 本環境のスキルカタログに `/researcher` `/plan-architect` `/implementer` `/reviewer` `/qa` が無いため（Phase 3+4・5・5b と同一条件）、各工程の手順を直接なぞって実施し、根拠を本計画書と issues に残す。
- ツール利用実績: `read_file` で `src/semantic/{mod,contract,provider,bridge,ffi,ffi_stub,fake,perception}.rs` / `src/rename/{service,document,image,rules,validate}.rs` / `src/models/file_context.rs` / `src/context/image_metadata.rs` / `src/normalize/mod.rs` / `src/main.rs` / `swift/kaseifu_fm_bridge.swift` / `build.rs` / `schemas/fm-rename-v1.json` / `tests/v09{2,3,4}*` / `scripts/quality_check.sh` / `README.md` を確認、`terminal` でベースライン再実測（686 passed）、AVFoundation の実機プローブ（メタデータ・代表フレーム・Vision・音声のみ・破損ファイル・長時間動画・所要時間・chrono の `%z` 挙動）、`grep` で完全リテラル・呼び出し箇所・文言を実測。LSP / serena / context7 / codebase-memory-mcp / graphify は本環境に無いため read_file + grep + terminal で代替した。

## 1. ベースライン（実測・2026-10-14）

- HEAD: `440719c`（タグ `v0.9.0`、`origin/main` と同期、`git status --short` 空 = clean）。
- `cargo test --all-targets --all-features` = **686 passed / 0 failed / 4 ignored**（本スレッドで再実測。ログ `/tmp/kb-test-baseline.log`）。
- 環境実測: macOS 26.4.1、Swift 6.3.3（`swift-driver 1.148.6`）。Apple Intelligence 有効。
- **ffmpeg / ffprobe は本機で動作しない**（`dyld: Library not loaded: /opt/homebrew/opt/x265/lib/libx265.216.dylib`。実測）。ffmpeg はテスト・製品の依存にしない方針なので実害はないが、**フィクスチャ生成も ffmpeg 以外の手段（AVAssetWriter / afconvert / 手書き WAV）で行う**（§4-D7 の実測参照）。
- テスト版数接頭辞: `tests/` は `v090`〜`v094` が使用済み。新規結合テストは `v095_*` とする。

### 1.1 AVFoundation の実機プローブ（`/tmp/kb-probe`、合成素材のみ）

| 検証 | 実測結果 |
|---|---|
| メタデータ（同期 0.006〜0.009 秒） | `duration 2.0s` / `64x48` / `frame_rate 10.0` / `creationDate`（QuickTime `com.apple.quicktime.creationdate`）→ **元の文字列 `2026-10-14T09:30:00+0900` を保持**（`commonMetadata` の `stringValue`）。`asset.creationDate` の `dateValue` は UTC に変換される（`2026-10-14T00:30:00Z`）ため、**日付が繰り上がる危険があり単独では使わない** |
| 代表フレーム（2 秒 10fps の合成 MP4） | 3 枚（<15 秒規則）を 0.2 / 1.0 / 1.8 秒で取得、計 2.023 秒（コールド、Vision 初回込み）→ ウォーム 0.169 秒 |
| 代表フレーム（130 秒 8fps の合成 MP4） | **5 枚を 13 / 39 / 65 / 91 / 117 秒**（= 10%..90% の等間隔、原典 §5.3 の 120 秒例と同じ規則）で取得。**抽出 + 5 枚 x Vision で 0.196〜0.236 秒**（長時間動画でも処理量は伸びない） |
| 音声のみ（AAC のみの MP4） | `loadTracks(.video)` が空 → **0.027 秒で `video-no-video-track`**（明示理由） |
| 破損ファイル（テキストを `.mp4` に改名） | **0.008 秒で `video-unreadable`**（AVFoundationErrorDomain -11829）。詳細文字列は改行入りで長いため Swift 側で domain + code に丸める（§5-V3） |
| 実フレームの Vision | 合成 64x48 でも 12 件のラベル（`document 0.049, screenshot 0.048, ...`）が返り、降順・上限・床の既存レシピで bounded |
| autolink | `import AVFoundation` の object に `-framework AVFoundation` が入る（既存 build.rs のリンク手順のまま。Vision/ImageIO と同じ） |
| chrono | `DateTime::parse_from_rfc3339` は `+0900` を拒否、`%Y-%m-%dT%H:%M:%S%z` は `+0900` / `+09:00` の両方を受ける（offset の壁時計を保持）。`Z` は RFC3339 側で受ける（0.4.45 で実測） |

## 2. 利用する土台の実測再確認（file:line）

| 土台 | 実測 | Phase 6 での使い方 |
|---|---|---|
| `src/semantic/perception.rs:23-220` | 画像知覚の envelope・版検査・床/件数/文字数の bound・重複除去・降順 | **そのまま再利用**する（ラベル bound は `bound_labels` を動画の統合にも通す）。統合（フレーム間）の新規ロジックは別ファイルに 1 つだけ足す |
| `src/semantic/ffi.rs:66-93,145-152,260-280` | `raw` C ABI（同期・文字列返却）と `take_string` / `OwnedResult`（`kaseifu_fm_free_result` で解放） | 動画も同じ形で `kaseifu_video_perceive(path) -> JSON` を足す（新規 unsafe はこの module に閉じる） |
| `src/semantic/bridge.rs:55-118` | `FmBridge` trait（available / perceive / generate）。`SwiftFmBridge` が本番 | `video_perceive` を同じ trait に足す（**テストの注入口は 1 つのまま**） |
| `src/semantic/provider.rs:104-120,209-286` | `SemanticProvider` trait、`build_request`、`image_facts` / `perception_facts` の写像 | `video_facts` を同じ流儀で追加。`SemanticProvider::perceive_video` を追加 |
| `src/semantic/fake.rs:37-200` | `FakeFmBridge`（応答・perceive 記録）、`FakeSemanticProvider` | 動画の Fake 設定（成功・失敗・呼び出し記録）を追加 |
| `src/semantic/provider.rs:245-251` | `target_kind` は画像か文書しか返さない（動画は未配線） | `TargetKind::Video` を返す分岐を追加（`contract.rs:44-60` の variant は既にある） |
| `src/context/mod.rs` / `src/context/image_metadata.rs:59-67` | 拡張子 predicate は context 層が持ち、`rename::image` と `semantic::provider` の両方が使う | 動画 predicate を `src/context/video.rs` に置く（`src/semantic/` は `crate::rename` を **0 箇所** しか参照していない実測。層を逆転させない） |
| `src/rename/document.rs:64-79` | 動画拡張子は「video rename is not available yet」で拒否（P6 で虚偽になる） | メッセージを「is a video, not a document (txt, md, pdf, html)」に更新。未知拡張子のメッセージは変更しない（最小差分） |
| `src/semantic/ffi_stub.rs:29-70` | Swift 不在ビルドの同一 C ABI（常に unavailable + REASON） | `kaseifu_video_perceive` を足し同じ REASON で縮退 |
| `src/semantic/contract.rs:83-100,151-160` | `FmInput`（`image: Option<FmImageFacts>` / `perception: Option<FmPerception>` を skip 直列化） | `video: Option<FmVideoFacts>` を**追加のみ**で足す（既存キー削除・型変更なし） |
| `src/models/file_context.rs:67-153` | `NormalizedFileContext`（`image_metadata` / `perception` を skip 直列化） | `video: Option<VideoInfo>` を同じ流儀で追加（完全リテラルは 3 箇所のみ: `src/normalize/mod.rs:117`、`src/discovery/search_terms.rs:117,145`） |
| `src/rename/image.rs:37-57` | 拡張子ベースの `is_image` / `classify_image`（`context::image_metadata` の predicate を再利用） | 動画版 `src/rename/video.rs` を同じ形で追加（判定は rename 層に 1 つだけ） |
| `src/rename/service.rs:200-321,450-479` | `plan_rename` の画像分岐、`perceive`、`label_echo`、`resolve_plan_date`、`finish_plan` | 動画分岐を追加（injection 検査 -> 動画知覚 -> 統合 -> context -> analyze）。日付は `recorded` 優先を 1 分岐で足す |
| `src/rename/rules.rs:242-307` | `parse_datetime`（RFC3339 -> zone 無し）と `resolve_image_date`（実信号 -> created -> modified -> now） | `resolve_video_date` を同じ規則で追加し、`parse_datetime` に `%z` を 1 段だけ足す（`+0900` を受ける） |
| `src/main.rs:1200-1213,934-946,978-991,1124-1198` | `reject_foreign_rename_target`（video を拒否）、guidance、`RenameReport` + `emit_rename_report` | video を受理に変更、guidance / help / report を更新（JSON は追加のみ） |
| `swift/kaseifu_fm_bridge.swift:166-238,274-339` | `kfPerceive`（ImageIO -> Vision -> envelope）、`kfClassifyImageRequest` のラベル整形、`kfBuildPrompt` | ラベル整形を `kfClassify` に切り出して**画像と動画で共用**。`kaseifu_video_perceive` と動画のプロンプト行を追加 |
| `schemas/fm-rename-v1.json:21-56` | request の required キー列挙 + 任意の `image` / `perception` | `video` を任意プロパティとして明文化（required に入れない） |
| `scripts/quality_check.sh:57-120` | fmt / clippy / test / complexity / baseline-gate / self-test / coverage | 変更しない（ゲートとして使う） |

## 3. スコープ

### 3.1 対象（今回実装する）

- **AVFoundation メタデータ**: 再生時間・解像度・フレームレート・作成日時を Swift で取得し、Rust の `VideoInfo` に載せて `input.video` でモデルへ、`--format json` と text 出力にも出す。
- **代表フレーム抽出**: 3〜5 枚（長さに応じて枚数と位置を調整）、動画全体はデコードしない（§4-D2 の decode bound）。
- **フレーム -> 知覚**: 抽出フレームを既存の Vision レシピ（2048 thumbnail・上限 12・床 0.01）で分類し、**Rust 側で統合**（重複除去・最大確信度・上限 12）。
- **言語化**: 統合ラベルを既存の `input.perception` -> `visual_labels` 経路で Foundation Models に渡し、日本語タイトルを得る。
- **命名への配線**: `kaseifu rename clip.mp4`（拡張子不変、stem 80 字・単語境界・アンダースコア、dry-run 既定、`--execute` で journal -> rename -> undo）を動画で一巡させる。
- **失敗時の分類（原典 §12）**: フレーム抽出失敗・デコード不能・音声のみは**その対象をスキップ（提案を保留）**し理由を表示。他対象の処理は止めない。部分的なフレーム失敗は失敗フレームだけスキップ。
- **表示の更新**: 拒否メッセージ・help・guidance・README・`tests/v092_ai_rename_p34_test.rs` の該当テストを同一ラウンドで更新。
- テスト: `tests/v095_ai_rename_p6_video_test.rs`（Fake 注入 + 合成フィクスチャ 2 点、いずれも CI で stub/Swift 両対応）。
- 実機検証: 合成動画で dry-run / `--execute` / `undo` を実行し、**配線前との提案名比較**・所要時間・実ラベルを記録する。

### 3.2 対象外（着手しない）

- Phase 7（フォルダ）・Phase 8（一括）・Phase 9。
- 動画バイトの FFI 越境、フレーム画像のファイル書き出し（**読み取り専用**原則）、Rust 側の動画デコード、新規 Rust crate、新規設定ファイル・別 DB・独自キャッシュ・外部プロセス。
- 音声の内容理解（原典 §5.3 の制限事項。フレーム解析だけでは音声内容を理解できない）。動画の動作（モーション）解析。
- OCR（画像ラウンドの D5 で次回送りのまま）。
- 既存コマンド（organize / watch / classify / inspect / doctor / undo / uninstall / bench、rename 文書系・画像系）の挙動変更。`--format json` の既存キー削除。`discover` の公開。内部 Phase 語の help 露出。

## 4. 設計判断（選択肢と結論。黙って決めない 7 点）

### D1. 知覚層の呼び出し形（既存 ABI の再利用 vs 動画用 ABI の追加）

- 選択肢A（不採用）: 抽出フレームを一時ファイルに書き出して既存 `kaseifu_vision_perceive` を各フレームに呼ぶ。**フレームの書き出しは禁止**（§3.2 / 原典 §11.3 の「元画像を変更しない」精神）であり、さらに Rust から動画メタデータが見えなくなる。
- 選択肢B（不採用）: `kaseifu_fm_generate` の内部で Swift が AVFoundation と Vision を回し、**統合済みラベルだけ**を応答に載せる。FFI 追加ゼロだが、(a) Rust から各フレームとメタデータが見えず「統合」を Rust のテストで固定できない（ソース grep に堕ちる）、(b) 原典 §5.3 が要求する再生時間・解像度・フレームレート・作成日時を Rust が命名・表示・契約に使えない、(c) 生成のタイムアウト・キャンセル契約に観測処理が混ざる。
- 選択肢C（採用）: **新 C ABI `kaseifu_video_perceive(video_path) -> JSON`（同期・文字列返却・既存 `kaseifu_fm_free_result` で解放）**。1 回の呼び出しで「メタデータ + 代表フレーム列（各フレームの時刻とラベル）」を返す。Rust が（1）メタデータを観測モデルへ、（2）フレーム間の統合と bound、（3）`input.video` / `input.perception` への写像と命名規則の適用、を受け持つ。根拠: 既存の層構成（contract / ffi / bridge / provider / perception）にそのまま乗り、**Rust 側のテストで統合・bound・契約を決定的に固定できる**。知覚の入り口は `FmBridge` のまま 1 つなので Fake 注入だけで OS に触れない。
- 帰結: Swift は「書かない」原則のまま、**Rust が渡した動画 1 本を read-only で開き、代表フレームだけをメモリ上で分類**する（フレームもヘッダも書き出さない）。
- predicate の置き場（層の逆転防止）: 拡張子 predicate は `src/context/video.rs` に置き、`rename::video` と `semantic::provider` の両方がそれを使う（`image_metadata::is_image_file` と同じ構図）。`src/semantic/` から `crate::rename` への参照は現在 0 件で、この依存を新設しない。

### D2. 代表フレームの枚数と位置、長時間動画の上限

- 枚数（原典 §5.3「3〜5 フレーム」の範囲内で長さに応じて調整）: **< 15 秒 -> 3 枚 / 15〜60 秒 -> 4 枚 / >= 60 秒 -> 5 枚**。原典の 120 秒例（5 枚）と整合し、短いクリップに 5 枚を割かない。上限は常に **5 枚**。
- 位置: `duration * (0.1 + 0.8 * i / (n - 1))`（i = 0..n-1）。120 秒 / 5 枚なら 12 / 36 / 60 / 84 / 108 秒で**原典 §5.3 の例と一致**。130 秒の実測でも 13 / 39 / 65 / 91 / 117 秒。
- `duration` が非有限・0 以下（壊れたコンテナ）: 時刻 [0, 0.5, 1.0] の 3 回だけ試し、1 枚も取れなければ明示エラー（`video-frames-unavailable`）。
- **動画全体をデコードしない**ための bound（実測付き）:
  1. `AVAssetImageGenerator` を使い `AVAssetReader` で全走査しない（要求時刻の近傍だけをデコード）。
  2. `requestedTimeToleranceBefore/After = 1 秒` — 1 回の抽出でデコードされる範囲を要求位置の ±1 秒に閉じる（動画長に関わらず最大 5 回 x 1 秒分）。
  3. `maximumSize = 2048 x 2048` — デコード後のフレームを長辺 2048 に縮小（画像経路と同じ 16 MiB 級 bound）。
  4. 枚数上限 5。
- 実測: 130 秒動画で 5 枚抽出 + 5 x Vision = 0.196〜0.236 秒（本機）。**動画長に比例するコストが観測されない**ことを §9 に記録する。
- 選択肢（不採用）: 全編均等の 5 枚固定 / キーフレーム位置の走査（`AVAssetReader` が必要になる）/ シーン検出（重い）。

### D3. フレーム間の統合方式

- 選択肢A（採用）: **ラベル文字列ごとに最大確信度を残す**（同一ラベルの重複は 1 件）。その後、既存の `perception::bound_labels` をそのまま通す（降順・床 0.01・大文字小文字を無視した重複除去・最大 12 件・1 件 64 文字）。
  - 合算（sum）は**フレーム数に依存**して値が伸び、3 枚構成と 5 枚構成でスケールが変わるため不採用。
  - 平均は「1 フレームだけ高確信で出たラベル」を不当に落とすため不採用。
  - 「出現フレーム数による加点・順位」は測定なしで意味を作り込むことになるため今回は不採用（採用するなら実測でタイトル品質の差を示す）。
- 上限: **統合後 12 件**（既存件数と同じ）。5 フレーム x 12 件 = 60 件が来ても、プロンプト・レポート・JSON は 12 件を超えない（bound は境界で 1 回だけ適用）。
- 防御: ブリッジが 5 枚を超えるフレームを返しても Rust が先頭 5 件に切る（`MAX_VIDEO_FRAMES`）。

### D4. 契約の拡張（`fm-rename-v1` の add-only）

- 選択肢A（採用）: `input.video` を**任意**で追加（`duration_seconds` / `width` / `height` / `frame_rate` / `creation_date`）。**統合ラベルは既存の `input.perception.labels` に載せる**（プロンプトの `visual_labels` 経路が画像と動画で 1 本になり、`kfBuildPrompt` の重複実装を作らない）。`schema_version` は `fm-rename-v1` のまま（形状の追加のみ。原典 §3.4 の「未知フィールドを安全に処理」規約は両側に既にある）。
- 選択肢B（不採用）: `input.video.perception` を新設して統合ラベルを動画側に持たせる。プロンプト分岐が二重になり、画像で固定した `visual_labels` のテストが動画に効かなくなる。
- 選択肢C（不採用）: `fm-rename-v2`。破壊的変更が無いので差分に見合わない（温存）。
- `schemas/fm-rename-v1.json`: `input.video` を任意プロパティとして追記し、`perception` の説明を「image and video」に更新（キー・必須集合は変えない）。

### D5. 動画の日付（AVFoundation の作成日時をどう位置づけるか）

- 結論: **AVFoundation の作成日時は Phase 5 でいう「実信号」の位置に置く**。iPhone / カメラが書く QuickTime `com.apple.quicktime.creationdate` は撮影時刻そのもの（時刻オフセット付き）で、ファイルシステムの日付は転送・コピー時刻になり得る。
- 優先順: **recorded（AVFoundation）-> created（ファイル作成）-> modified（更新）-> now**。`resolve_image_date` と同じ構造で `resolve_video_date` を追加し、`date_source` は `recorded` と表示する（新しい `DatePreference::Recorded`）。
- オフセットの扱い: 実測どおり QuickTime の値は `2026-10-14T09:30:00+0900` の形で来る。`asset.creationDate.dateValue` は UTC に正規化されるため**採用しない**（日付が 1 日ずれ得る）。`stringValue` をそのまま持ち、`parse_datetime` に `%Y-%m-%dT%H:%M:%S%z` を 1 段だけ足して受ける（オフセットの壁時計 = 撮影地の時刻をそのまま使う。ゾーンを推測しない既存規則と一致）。`+0900` は現行 RFC3339 では**捨てられる側だった**ので、この追加は受け入れ範囲を広げるだけで既存の解釈を変えない（chrono 0.4.45 で実測）。
- 制限（§9 に記録）: コンテナに文字列が無く `dateValue` しかない場合は UTC 文字列になる（日付が 1 日ずれる可能性）。その場合でも「created/modified よりは撮影に近い」ため recorded 扱いを維持する。

### D6. タイムアウト（20 秒の adequacy）

- 構成: 動画経路の wall = 動画知覚（同期・timeout 対象外）+ FM 生成（`DEFAULT_FM_TIMEOUT = 20 秒`）。
- 実測: 動画知覚は 0.027 秒（音声のみ）/ 0.169 秒（2 秒動画・3 枚・ウォーム）/ 0.196〜0.236 秒（130 秒動画・5 枚）。コールドでも 2.0 秒（Vision 初回ロード込み）。
- 結論: **`DEFAULT_FM_TIMEOUT` は変更しない**（生成のみに掛かる既存契約を維持）。画像ラウンドの実測（生成 8〜17 秒）と本ラウンドの知覚コストを足しても 20 秒以内に収まる見込み。実機で `skipped: Timeout` を観測した場合のみ、根拠付きで見直す（P5b からの条件付き繰り越しを継続）。§9 に実測 wall を記録する。

### D7. テスト用の動画素材

- 選択肢A（採用）: **合成フィクスチャ 2 点をコミット**し、AVFoundation の実経路を CI でも通す（CI は macOS。Swift ツールチェーンが無いビルドでは stub が `unavailable` を返すので、テストは v094 と同じ「実経路 or 明示 unavailable」の二択を許容する形にする）。
  - `tests/fixtures/video/tiny.mp4` — **2,457 bytes**、64x48 / 2.0 秒 / 10 fps / H.264、QuickTime creationdate `2026-10-14T09:30:00+0900`。出自: 本機の AVAssetWriter で生成した合成パターン（動く矩形 + グラデーション。テストパターンのみで実ユーザーの動画は使わない）。生成ツールは開発時の手作業（§9 にコマンドを記録）。
  - `tests/fixtures/video/audio-only.mp4` — **8,580 bytes**、2.0 秒 AAC のみ（映像トラックなし）。出自: 手書き WAV（Python で生成した 440 Hz 正弦波）を macOS の `afconvert` で AAC-in-MP4 にした合成音。
  - サイズと出自は `tests/fixtures/video/README.md` にも明記する。
- 選択肢B（不採用）: フィクスチャ無し・境界スタブのみ。実経路の退行（AVFoundation 呼び出しの破壊）を CI が検出できなくなる。
- 選択肢C（不採用）: 実ユーザーの動画を使う。禁止事項。
- 選択肢D（不採用）: テスト実行時にフィクスチャを生成する。CI に Swift も afconvert も無い前提を崩し、テストが重くなる。
- 注記: ffmpeg は本機で壊れている（§1）ため生成に使わない。フィクスチャはコミット済みのバイナリで、テストは読み取りのみ。

## 5. BDD シナリオ（振る舞いと具体例）

### 動画境界（`src/semantic/video.rs` / ffi / bridge）

- V1 正常: `status:success` + `video{duration_seconds:120.0,width:1920,height:1080,frame_rate:29.97,creation_date:"2026-10-14T09:30:00+0900"}` + `perception.frames[5]` -> `VideoPerception{info, frames}`（フレーム数・時刻・ラベルが読める）。
- V2 統合: frames が `[frame0: sky 0.30, moon 0.10] [frame1: sky 0.42, sunset_sunrise 0.20, moon 0.05] [frame2: tree 0.9]` -> 統合 = `tree(0.9) sky(0.42) sunset_sunrise(0.20) moon(0.10)`（最大値・降順・重複 1 件）。13 件 x 5 フレームの 60 件 -> 12 件に bound、床 0.01 未満は落ちる。
- V3 失敗の分類: `kind:failed reason:video-no-video-track` -> `Failed{detail}` / `kind:unavailable` -> `Unavailable{reason}` / 壊れた JSON・版違い・未知 status -> `Malformed`。**エラー詳細は domain + code に丸め、改行を含めない**（レポート 1 行を壊さない）。
- V4 実機（bridge + コミット済みフィクスチャ、stub では明示 unavailable を許容）: `tiny.mp4` -> `duration 2.0 秒 +/- 0.2` / `64x48` / `frame_rate 10 +/- 0.5` / `creation_date` が `2026-10-14T09:30:00+0900` / フレーム 3 枚（ラベルは降順・12 件以内・64 文字以内）。`audio-only.mp4` -> `video-no-video-track`。テキストを `.mp4` に改名 -> `video-unreadable`。

### 契約（`src/semantic/contract.rs` / schema）

- C1 動画要求: `FmRequest{target_type:video}` を serialize すると `input.video.*` と `input.perception.labels` が読める。`input.image` は動画では出ない。`input` の既存キー集合は不変。
- C2 後方互換: `video` の無い要求は従来どおり parse でき、`video` の型が違う場合は欠損扱い（推測しない）。
- C3 文書・画像要求は `video` を運ばない（キー自体が出ない）。
- C4 schema: `video` は任意（required に入らない）。`$id` は `fm-rename-v1` のまま。

### 配線（`src/rename/service.rs` / `main.rs`）

- W1 動画プラン: Fake 動画知覚（3 枚: `[ocean 0.5] [ocean 0.7, beach 0.4] [beach 0.6]`）＋ Fake モデル（title `海辺の風景`）＋ P02 -> 提案 `2026-10-14_海辺の風景.mp4`、request の `input.perception.labels` = `beach(0.6) ocean(0.7)` の降順 = `ocean(0.7) beach(0.6)`、`input.video.duration_seconds` がある。
- W2 文書・画像プラン: 動画知覚は 0 回（`video_perceived_paths` が空）、request に `video` キーなし。
- W3 注入ファイル名: 動画知覚も推論も 0 回（Held）。
- W4 音声のみ / 抽出失敗: モデルは呼ばれず `Held` + 理由（`no video track (audio only)` / `no representative frame could be extracted` / `could not be read`）。ファイルは動かない。
- W5 統合ラベルが空: `Held` + 理由（`no visual content could be perceived`）。
- W6 ラベル丸写し: title が `ocean`（統合ラベルと一致）-> `Held` + 理由。`海辺の風景` は通る。
- W7 日付: 動画 creation date がある -> `date_source: recorded`、無ければ created -> modified -> now。`parse_datetime` が `+0900` を壁時計のまま受ける。
- W8 非破壊: 拡張子不変（`.mp4` のまま）・stem 80 字・単語境界・アンダースコア・上書き禁止・No-op・symlink 拒否・Journal 先行・undo（Phase 5 の規則を動画で再確認）。
- W9 CLI: `rename clip.mp4`（中身はテキスト）-> 終了コード 0・`status: held`・理由に動画の失敗分類・**内部 Phase 語なし**・モデル呼び出しなし。`--format json` は既存キーを保ったまま `video` を追加。未知の拡張子は従来どおり fail-fast。
- W10 provider 防御: `perceive_video` に動画でないパス（`note.txt` / `clip.png`）を渡すと `Failed("not a video")`（AVFoundation に渡さない）。

### Swift 側

- S1 プロンプト: `input.perception.labels` があるときだけ `visual_labels:` 行（`<data>` 内）。動画では追加で「代表フレームのサンプルである（全編・音声ではない）」行と「動作・音を断定しない」行が入る。**画像のプロンプトはバイト不変**（画像 gate 数 2 を維持）。`input.video` がある場合の `video:` 行（長さ・解像度・fps）は `signals:` 行より前に置く。
- S2 ABI: 存在しないパス・ディレクトリ -> `status:error` の JSON（プロセスは落ちない）。null パス -> 明示エラー。
- S3 所有権: 動画知覚の戻り値も `kaseifu_fm_free_result` で解放（既存規約の再利用。`@_cdecl("kaseifu_fm_free_result")` は 1 つのまま）。
- S4 読み取り専用: `AVAssetWriter` / `CGImageDestination` / `FileManager` を使わない（grep で固定）。ラベル整形は `kfClassify` を画像・動画で共用する。

### 非破壊（全ラウンド共通）

- N1: 既存 686 テストが全て通る（変更は `FmInput` / `NormalizedFileContext` の完全リテラル 5 箇所のみ、既存テストの assert は不変）。既存コマンドの挙動不変。
- N2: `cargo fmt --check` 違反 0、clippy 警告 0、**新規 CC>=16 なし**、既存最大 CC を悪化させない、complexity ゲート PASS。
- N3: 出力 ASCII のみ（提案名の非 ASCII を除く）・em dash なし・TUI なし・NO_COLOR / 非 TTY 準拠。JSON は追加のみ。help に内部 Phase 語を出さない。
- N4: テストは隔離 HOME・一時 dir・注入のみ。実ユーザーの動画・`~/Movies/`・`~/Desktop/`・実 Keychain・実補完 dir を読み書きしない。実ファイル移動は一時 dir 内の合成ファイルのみ。`--execute` の実機確認も一時 dir の合成素材のみ。秘密情報を混ぜない。

## 6. タスク一覧（実装順・直列）

- T1 `src/models/file_context.rs`: `VideoInfo`（duration/width/height/frame_rate/creation_date、全て Option）+ `NormalizedFileContext.video`（`skip_serializing_if` / `default`）。`src/normalize/mod.rs:117` と `src/discovery/search_terms.rs:117,145` に `None`。
- T2 `src/semantic/video.rs`（新規）: envelope の parse（`VideoPerception` / `FrameLabels` / `VideoInfo`）、フレーム上限 `MAX_VIDEO_FRAMES = 5`、各フレームの bound、`merge_frames`（最大確信度 -> `bound_labels`）、失敗分類（`perception` の共通マッパを再利用）。V1〜V3 の単体テスト。`src/semantic/mod.rs` に `pub mod video;` を追加。
- T3 `src/semantic/perception.rs`: 失敗マッピングを `pub(crate)` に切り出し（重複実装なし）。既存テスト不変。
- T4 `src/semantic/contract.rs`: `FmVideoFacts` + `FmInput.video`（任意・skip）+ テスト。C1〜C3。
- T5 `src/semantic/ffi.rs`: `raw::kaseifu_video_perceive` + 安全ラッパ（NUL・非 UTF-8 を明示エラー）。
- T6 `src/semantic/ffi_stub.rs`: 同一 ABI を unavailable + REASON で実装。
- T7 `src/semantic/bridge.rs`: `FmBridge::video_perceive` と `SwiftFmBridge` 実装。層 doc 更新。
- T8 `src/semantic/fake.rs`: `FakeFmBridge` の動画設定 + 呼び出し記録、`FakeSemanticProvider` の動画設定（既定 unavailable）。
- T9 `src/semantic/provider.rs`: `SemanticProvider::perceive_video`（非動画は `Failed("not a video")`、predicate は `context::video`）、`video_facts` 写像、`target_kind` の `TargetKind::Video` 分岐、`build_request` の配線。C1/C3/W10。
- T10 `src/context/video.rs`（新規）: `is_video_file`（拡張子ベース、mp4/mov/m4v/3gp/3g2、大文字小文字を無視）+ 単体テスト。`src/context/mod.rs` に `pub mod video;`。
- T11 `src/rename/video.rs`（新規）: `is_video`（context 層 predicate を再利用）/ `classify_video`（非動画は理由付きで拒否）/ `VideoError`。`src/rename/mod.rs` に `pub mod video;`。
- T12 `src/rename/rules.rs`: `resolve_video_date` + `DatePreference::Recorded` + `parse_datetime` の `%z` 段。W7 の単体テスト。
- T13 `src/rename/service.rs`: `plan_video_rename`（classify -> observe -> injection 検査 -> 動画知覚 -> 統合 -> context -> analyze -> finish）、`VideoReport`、`RenamePlan.video`、`PlanFacts.video`、保留理由の生成、日付モード（document / image / video）。W1〜W8。
- T14 `src/main.rs`: `reject_foreign_rename_target` が動画を受理、help / guidance の文言更新、`RenameReport.video` + `video_json` + text 行。W9。
- T15 `src/rename/document.rs`: 動画拡張子のメッセージから「not available yet」を外す（P6 で虚偽になるため）。
- T16 `swift/kaseifu_fm_bridge.swift`: ラベル整形を `kfClassify` に切り出して共用、`kaseifu_video_perceive`（メタデータ -> フレーム数・位置 -> 抽出 -> 分類 -> envelope）、動画プロンプト行、ヘッダ更新。S1〜S4。
- T17 `schemas/fm-rename-v1.json`: `input.video` の明文化と `perception` 説明の更新。C4。
- T18 テスト: `tests/fixtures/video/{tiny.mp4,audio-only.mp4,README.md}` と `tests/v095_ai_rename_p6_video_test.rs`（V4・C1〜C4・W1〜W10・S1・S4）。`tests/v092_ai_rename_p34_test.rs` の動画テストを新挙動へ更新。
- T19 README 更新: rename 節（動画対応・代表フレーム・読み取り専用）、`classify` 節の動画記述との矛盾がないことの確認。
- T20 実機検証（テストに含めない。§9 に記録）: 動画 dry-run / `--execute` / `undo`、配線前後の比較、`--format json`、所要時間、実ラベル、音声のみ、長時間、文書・画像の非退行。
- T21 検収: `cargo test --all-targets --all-features`（通常 + stub）、fmt、clippy、`bash scripts/quality_check.sh`、`git diff` 点検（commit しない）。

## 7. 検証手段（実測で示す）

- `cargo test --all-targets --all-features`（686 + 新規の pass/fail 数）。
- `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --all-targets --all-features`（Swift / AVFoundation / Vision の無いビルドの縮退）。
- `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`bash scripts/quality_check.sh`（OVERALL と complexity 表）。
- 実機 CLI（隔離 HOME・一時 dir・合成動画のみ）: dry-run / `--execute` / `undo --last 1` / 二重 undo / `--format json` / 所要時間 / 実ラベル。配線前バイナリ（HEAD `440719c` を `/tmp/kb-probe/kaseifu-pre` へ退避）との同一入力比較。
- `git status --short` と `git diff --stat` の目視（無関係変更・秘密情報なし、`Cargo.toml` / `Cargo.lock` 無変更）。

## 8. 受け入れ基準（原典のうち本ラウンド分）との対応

| 原典 | 本ラウンドでの担保 |
|---|---|
| §4.4 動画対応（AVFoundation で代表フレーム -> 解析、動画を直接入力しない） | AVFoundation でメタデータ + 代表フレームを取得し、Vision ラベル -> FM 日本語タイトルの既存経路に載せる。動画バイトはモデルにも FFI にも渡さない（T14 / V4 / §9） |
| §5.3 解析情報（再生時間・解像度・フレームレート・作成日時・代表フレーム） | `VideoInfo` -> `input.video` + レポート表示。実機で実測（V4 / W1 / §9） |
| §5.3 代表フレーム 3〜5 枚 / 長さで調整 | D2 の枚数・位置規則。120 秒例と一致することを単体テストで固定（V2 / T14） |
| §5.3 制限事項（音声不問・動作不確実・複数画像が使えない環境では個別フレームを解析して統合） | 複数画像入力は使わず、**フレームごとに分類して Rust で統合**（D3）。プロンプトは「代表フレームのサンプルであり音声・動作は見ていない」と明示（S1 / §9） |
| §11.4 最適化（全編をデコードしない・代表のみ・枚数上限・長時間で制限・抽出失敗はスキップ） | D2 の 4 つの bound（tolerance / maximumSize / 枚数 / generator）+ 失敗フレームのスキップと全体失敗の保留（W4） |
| §12 エラー処理（フレーム抽出失敗 = スキップ、読取失敗 = スキップ、FFI エラーは安全に伝達、panic / exception を境界に漏らさない） | Swift は `try?` / `catch` で全て JSON エラー化、Rust は `PerceptionError` に分類し Held + 理由。プロセスは落ちない（V3 / W4 / W9） |
| §13.4 VID01〜VID09 | VID01/02 は MP4/MOV の実経路（T16 / §9）、VID03 再生時間（V4）、VID04 代表フレーム（V4）、VID05 複数フレーム解析（V2/W1）、VID06 長時間（D2 実測）、VID07 抽出失敗（W4）、VID08 音声のみ（V4/W4）、VID09 内容が変化する動画（130 秒合成の 5 枚でフレーム差を実測） |

## 9. 検収記録（追記式。実装後に実測値を埋める）

### 9.1 ベースラインとテスト（実測）

- ベースライン: 686 passed / 0 failed / 4 ignored（HEAD `440719c`、本スレッドで再実測）。
- 実装後 `cargo test --all-targets --all-features` = **729 passed / 0 failed / 4 ignored**（新規 43 件: v095 統合 24 + `semantic::video` 単体 10 + `rename::video` 3 + `context::video` 2 + `rules` 3 + `contract` 1。既存 686 は維持、`v092` / `v093` の完全リテラルと動画テストのみ更新）。
- `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --all-targets --all-features` = **729 passed / 0 failed / 4 ignored**（Swift / Vision 不在ビルドの縮退でも同じスイートが通る。実 AV 経路の 3 テストは `unavailable` + 理由を受容）。
- `cargo fmt --all -- --check` 違反 0。`cargo clippy --all-targets --all-features -- -D warnings` 警告 0。
- `bash scripts/quality_check.sh` = **OVERALL: PASS**。complexity: Functions 1260、CC<=10 1249（99.1%）、CC 11-15 = 11、**CC>=16 = 0**、最大 CC 15（ベースラインと同じ）、baseline-gate PASS（no new/worsened CC>=16）、coverage 83.76%（report-only）。新規関数の実測 CC: `plan_video_rename` 9 / `video_hold_reason` 10 / `merge_frames` 3 / `video_json` 6 / `keep_strongest` 5 / `perceive_video` 1-3 / `video_facts` 3 / `is_video_file` 3。
- `git status --short` は変更 21 ファイル + 新規 7（計画・issue 2・`src/{context,rename,semantic}/video.rs`・`tests/v095_*`・フィクスチャ 3）。`Cargo.toml` / `Cargo.lock` 無変更（新規 crate なし）。

### 9.2 実機 AVFoundation（合成素材のみ。`/tmp/kb-verify/media`、プローブは `/tmp/kb-probe/probe_video`）

- メタデータ（同期 0.006〜0.009 秒）: `VID_0012.mov` -> `duration 30.0s / 640x360 / 6.0 fps / creation_date "2026-10-14T17:42:11+0900"`（元のオフセットを保持）。`holiday-camera-roll.mov` -> `130.0s / 480x270 / 4.0 fps / "2025-08-10T15:30:00+0900"`。
- 代表フレーム: 30 秒動画 -> **4 枚**（3 / 11 / 19 / 27 秒 = 10/30/50/70%）。130 秒動画 -> **5 枚**（13 / 39 / 65 / 91 / 117 秒 = 10/30/50/70/90%、原典 §5.3 の 120 秒例と同じ規則）。
- 所要時間（抽出 + フレームごとの Vision、ウォーム）: 30 秒動画 **0.228 秒**、130 秒動画 **0.209 秒**。**動画長に比例するコストは観測されない**。
- 失敗の分類: AAC のみ `silent-slides.mp4` -> **0.027 秒で `video-no-video-track`**。テキストを `.mp4` に改名 -> **0.008 秒で `video-unreadable`**（detail は `AVFoundationErrorDomain -11829` の短い 1 行）。
- 実ラベル（統合後、`VID_0012.mov`・JSON 12 件）: `recreation 0.055, sport 0.055, sports_equipment 0.051, ball 0.051, ping_pong 0.039, outdoor 0.036, celestial_body 0.036, moon 0.036, sky 0.029, blue_sky 0.029, document 0.018, screenshot 0.018`（降順・重複なし・64 文字以内）。
- コミット済みフィクスチャ（`tests/fixtures/video/`）も同じテストスイートで実経路を通る（v095 の 3 テスト）。

### 9.3 CLI 実機（隔離 HOME=/tmp/kb-verify/home、一時 dir、合成動画のみ）

- **配線前（HEAD `440719c` を `git archive` からビルドした `/tmp/kb-pre/target/debug/kaseifu`）**: `rename VID_0012.mov --pattern P02` -> `Error: .mov is a video; video rename is not available yet`（exit 1、0.8 秒）。**動画は提案すら出なかった**。
- **配線後（本実装）**: 同じ入力・同じオプションで提案が出る。例: `2026-10-14_ポーカー.mov` / `2026-10-14_空の天.mov` / `2026-10-14_ボールの打ち上げ.mov`（同一入力で 8 回中 7 回 propose、1 回は「title repeats a perception label (ping pong)」の保留 = ラベル丸写しガードが実機で発火）。
- 実行例（text、`VID_0012.mov`、P02、17.9 秒）:
  ```text
  proposal: 2026-10-14_ボールの打ち上げ.mov
  date_source: recorded
  video: 30.0s, 640x360, 6.0 fps, frames 4, recorded 2026-10-14T17:42:11+0900
  perception: recreation(0.055) sport(0.055) sports_equipment(0.051) ball(0.051) ping_pong(0.039) outdoor(0.036) (+6 more)
  status: proposed
  ```
- 長時間（130 秒、P07、14.1 秒）: `proposal: 2025-08-10_recreation_キャンプ.mov` / `date_source: recorded` / `video: 130.0s, 480x270, 4.0 fps, frames 5, recorded 2025-08-10T15:30:00+0900`。
- `--format json`（VID_0012.mov、12.7 秒）: 既存キー 9 個（`target, current_name, pattern, proposal, date_source, status, reason, operation_id, perception`）+ **`video` を追加**。`perception.status="labels"` 12 件、`video = {"creation_date":"2026-10-14T17:42:11+0900","duration_seconds":30.0,"frame_rate":6.0,"frames":4,"height":360,"width":640}`。
- execute / undo: `EXEC_0002.mov --pattern P01 --execute` -> `proposal: pong.mov` / `operation_id: op-41226-1791988794912803000` / `status: executed`（17.5 秒）。journal は `move ... success candidate_id=P01` -> `undo ... success` の 2 行、`undo --last 1` で `EXEC_0002.mov` に復帰し、二重 undo は `nothing left to undo`。
- 失敗経路: `silent-slides.mp4` -> `reason: the video has no video track (audio only); proposal withheld`（0.11 秒、モデル呼び出しなし）。`broken.mp4` -> `reason: AVFoundation could not read the video; proposal withheld`（0.08 秒）。いずれもファイルは動かない。
- 非退行: `note.txt` -> `proposal: 2026-10-14_a_short_meeting_note_about_the_launch_plan.txt`（`video:` / `perception:` 行なし、8.5 秒）。`IMG_1234.png` -> `proposal: 2026-10-14_月.png` + `perception:` 行（`video:` 行なし、14.1 秒）= 画像経路は不変。
- 出力は非 TTY / JSON モードで `label: value` 行、TTY では既存の ASCII パネル。em dash なし（v092/v095 のテストと目視）。
- 所要時間の総括: 動画 dry-run は **8.1〜17.9 秒**（知覚 0.2〜2.0 秒 + 生成）。`DEFAULT_FM_TIMEOUT`（20 秒、生成のみ）は超えていない。timeout 超過を観測した場合のみ根拠付きで見直す（D6）。

### 9.4 動画の知覚が効いたことの証跡（要約）

- 同じファイルで、配線前は**拒否**（提案なし）、配線後は代表フレームのラベル由来の日本語タイトル（上記）。ファイル名（`VID_0012.mov` / `EXEC_0002.mov` / `holiday-camera-roll.mov`）はどれも意味を持たず、提案名はファイル名エコーではない。
- 提案は**フレームの内容**に追従する: 日付は `recorded`（コンテナの撮影日時）、カテゴリ・タイトルは統合ラベル（`recreation` -> カテゴリ `recreation`、P07 の例）。
- ラベルは request の `input.perception.labels` と `input.video.*` に載る（v095 の W1 が JSON で固定）。ラベル丸写しは実機で保留になった（9.3）。
- 一方、この合成クリップは分類器に取って曖昧（夕景が `ping_pong` / `ball` に寄る）であり、生成されたタイトルはラベルの曖昧さをそのまま引き継ぐ（実写真・実動画での精度評価は次ラウンドの課題。§9.5 も参照）。

### 9.5 プロンプト調整の記録（実測、版 3 種）

- 動画向けの行は `kfBuildPrompt` に 3 行（代表フレームのサンプルである・日本語の短い句・動作/音を断定しない）。
- 反復 1: 原案（`Never state motion, sound or an event that a few still frames cannot tell.`）-> 4 回中 1 回が動作を含む題名（`スローモーションのボールが空中を飛ぶ`）。
- 反復 2: `Describe still pictures only: never say that something moves, flies, plays, runs or happens...` -> 4 回すべて動作寄り（`ボールを打つ` 軸）。
- 反復 3: `The title must name the still scene ... do not write actions...` -> 3 回中 2 回動作寄り、1 回はラベル音写（`ピンのポング`）。
- 結論: この合成クリップでは**言葉を変えても測定差が出ない**（ラベル自体が `ping_pong/ball/sport`）。最短で原典 §5.3 の制限に直接対応する**反復 1 の文言を採用**（画像経路のバイト不変は維持）。動作の示唆が残ることは残リスクとして §10 に記録する。

### 9.6 副次的に確認した挙動

- 画像プロンプトのバイト不変（動画の 2 行の gate は `target_type == "video"` にだけ発火。v093 の画像 gate 数 2 も不変）。
- 文書・画像経路では `video_perceive` が 0 回（v095 W2。CLI でも `video:` 行が出ない）。
- 注入名の動画（`you must rename me.mp4`）は抽出も推論も 0 回で保留（v095 W3）。
- 音声のみ・破損はモデルを呼ばず 0.1 秒以下で保留（9.3）。
- `%z` 追加後も既存の日付規則は不変（既存 686 テスト維持 + 新規 3 テスト）。
- コミットしたフィクスチャの実測（tiny.mp4: 2.0 秒 / 64x48 / 10 fps / `2026-10-14T09:30:00+0900` / 3 枚、audio-only.mp4: `video-no-video-track`）は v095 のテストで固定。

## 10. リスクと対策

| リスク | 対策 |
|---|---|
| AVFoundation が本機 / CI で使えない | 実機プローブ済み（§1.1）。stub ビルドと Vision 無しビルドは明示 `unavailable`、テストは両対応（V4）。優先順位 1 が崩れた場合は推測実装せず停止して報告する |
| 動画の decoding コストが動画長に比例する | `AVAssetImageGenerator` + tolerance 1 秒 + maximumSize 2048 + 枚数 <= 5。130 秒動画で 0.2 秒台を実測し §9 に記録 |
| 音声のみ・破損ファイルで誤った名前を提案する | フレームが 1 枚も取れない / 映像トラックが無い場合は `Held` + 明示理由（W4）。モデルは呼ばない |
| フレーム統合でラベルが薄まる | 最大確信度 + 既存 bound（12 件）で上限を固定。順位・件数を単体テストで固定（V2） |
| フレーム抽出・Vision がモデル利用不可でも走る | 知覚は request を組む前に必要（ラベルを載せる）。実測コストは 0.03〜0.24 秒で bounded。画像ラウンドと同じ順序の限界を §9 に記録 |
| 作成日時の UTC 正規化で日付が 1 日ずれる | 文字列（オフセット付き）を優先し `dateValue` は文字列が無い場合のみ。その限界を D5 と §9 に明記 |
| `+0900` 形式の受け入れで既存の日付解釈が変わる | `%z` 段は「現行 RFC3339 で捨てられていた形式」だけを受ける（拒否 -> 受理の一方向）。既存テストの assert は不変 |
| 既存テストの型変更 | `FmInput` / `NormalizedFileContext` の完全リテラル 5 箇所に `None` / `video: None` を足すだけ。assert は変更しない |
| predicate の層逆転 | `src/context/video.rs` に置き、`src/semantic/` から `crate::rename` への新規依存を作らない（実測: 現在 0 件） |
| 既存 665 テストの退行 | 文書・画像のプロンプト分岐と経路は不変。`git diff` とテストで固定（N1 / S1） |
| 出力の 1 行性が壊れる（AV エラー詳細の改行） | Swift 側で domain + code に丸める（V3）。Rust の bound とあわせて 1 行を保つ |
| 合成クリップのラベルが曖昧で、タイトルにその曖昧さが乗る | 実測と限界を §9.4・§9.5 に明記。実写真・実動画での品質評価は次ラウンドの課題（本ラウンドの証跡は「エコーからの脱却と配線」に限る） |
| モデルがフレーム間の変化から動作を示唆する題名を出す | プロンプトとセッション指示の両方で動作・音を禁止し（3 版を実測、§9.5）、残ることを残リスクとして記録（フレームだけでは動作を正確に理解できないという原典 §5.3 の制限そのもの） |
| 制限事項の表示漏れ（音声を理解していないこと） | 動画のプロンプトは「全編でも音声でもない代表フレームのサンプル」と明示（S1）。README の rename 節も同じ表現で書く（T19） |
| 題名がラベルをそのまま写す退化 | 実機で 8 回中 1 回発火し保留（§9.3）。ガードは既存 `label_echo` の再利用で、部分一致・別言語の描写は通す |
