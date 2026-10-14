# AIリネーム Phase 5（Image Rename）実装計画

## 0. 位置づけと進め方

- 原典（読み取り専用・編集禁止）: `documents/plans/user/20261014a.md`（AIリネーム開発指示書 v2.0）§4.3・§5.2・§11.3・§12。`20261014b.md`（TREC）は別プログラムで対象外。
- 前提: Phase 0-2 は実装済み（全体地図は `documents/plans/dev/20261014f-ai-rename-p0.md`）、Phase 3+4 は実装・検証済み（記録は `documents/plans/dev/20261014i-ai-rename-p34.md`、HEAD `c1a8d99`）。Phase 0 の再調査はしない。触る既存コードのみ実コードで再確認した。
- 進め方: BDD タスク分解 → plan-architect レビュー（issue 記録）→ implementer → reviewer → QA → 検収。commit / push / tag はしない（親スレッドが実施）。
- スキル運用の注記: 本環境のスキルカタログに `/researcher` `/plan-architect` `/implementer` `/reviewer` `/qa` が無いため（Phase 3+4 と同一条件）、各工程の手順を直接なぞって実施し、根拠を本計画書と issues に残す。
- ツール利用実績: `read_file` で `swift/kaseifu_fm_bridge.swift` / `src/semantic/contract.rs` / `provider.rs` / `bridge.rs` / `ffi.rs` / `src/rename/service.rs` / `document.rs` / `rules.rs` / `mod.rs` / `src/main.rs` / `src/context/image_metadata.rs` / `exif.rs` / `src/normalize/image.rs` / `src/models/file_context.rs` / `schemas/fm-rename-v1.json` / `tests/v092*` / `tests/v32*` を確認、`terminal` で sw_vers・SDK・Swift・cargo test・Swift プローブ・公式ドキュメント照合、`grep` で rename 配線・SDK シンボルを実測。LSP / serena / context7 等は本環境に無いため read_file + grep + terminal + fetch で代替した。

## 1. ベースライン（実測）

- HEAD: `c1a8d99`（`origin/main` と同期、clean。開始前に `git status --short` で確認する）。
- `cargo test --all-targets --all-features` = **639 passed / 0 failed**（本スレッドで再実測）。
- `bash scripts/quality_check.sh` = OVERALL: PASS が期待値（Phase 3+4 で PASS。本実装後に再実測する）。
- テスト版数接頭辞: `tests/` に `v090`・`v091`・`v092` が使用済み。新規結合テストは `v093_*` とする。
- 環境実測: macOS 26.4.1、Xcode 26.6、SDK MacOSX26.5（MacOSX.sdk と同一）、Swift 6.3.3。Apple Intelligence は有効で実機検証可能。テストはモデル非依存が既定。
- 最大の実測事実: 導入済み SDK の FoundationModels swiftinterface に画像入力 API（`Attachment`）が存在しない。最小 Swift プローブで確定した（D1）。

## 2. 利用する土台の実測再確認（file:line）

| 土台 | 実測 | Phase 5 での使い方 |
|---|---|---|
| `swift/kaseifu_fm_bridge.swift:182-225` | `kfBuildPrompt` は text のみ。`input.image` は capture_time・camera の2行をテキストとして載せる | 画像専用の人物特定禁止・断定禁止の2行を target_type==image 時のみ追加する。Attachment 配線はしない（D1） |
| `src/semantic/contract.rs:41-49,82-111` | `TargetKind::Image` あり。`FmInput.image: Option<FmImageFacts>`。未知フィールド無視・型違いは欠損扱い | 契約・スキーマ無変更。fm-rename-v1 のまま画像を流す（D1） |
| `src/semantic/provider.rs:190-231,235-267` | `build_request` が bound 済みで組立。`target_kind` は画像拡張子を Image 判定。`image_facts` は空メタデータを None にする | そのまま再利用。新規 FFI・プロバイダ変更なし |
| `src/context/image_metadata.rs:49-98` | `is_image_file`（jpg/jpeg/heic/heif/hif/tif/tiff/png/webp）、`collect`（Spotlight 優先・EXIF 穴埋め・PNG は IHDR 寸法） | そのまま再利用。Rust 側の画像デコード新規実装なし |
| `src/context/exif.rs:44-83` | `parse_file`（JPEG/TIFF/HEIC ベストエフォート、PNG/WebP は Unsupported、破損は Corrupt）。新規 crate なし | そのまま再利用。欠損・破損時の扱いは D3 に従う |
| `src/normalize/image.rs:12-18` | `validate`（不正値は落とす・直さない、全欠損は None） | そのまま再利用 |
| `src/rename/service.rs:149-182,283-363` | `plan_rename`（observe→文書信号→rule選択→injection検査→analyze→finish_plan）。`finish_plan` は EXIF 撮影日時を第3優先で解決し device を camera から解決する。実行は journal move 行＋TOCTOU 再確認 | 文書信号の取得部分だけ画像用に分岐させる。finish_plan・execute_plan・journal・undo は無変更で乗る |
| `src/rename/document.rs:54-80` | `classify_document` は画像拡張子を Phase 5 守備範囲エラーにする | 新 `src/rename/image.rs` の `classify_image` を追加する |
| `src/main.rs:1174-1184` | `rename_file` が `classify_document` で先行ゲートする | 画像も通すゲートに変える（映像・不明は従来どおり Phase 番号付きエラー） |
| `schemas/fm-rename-v1.json` | envelope の oneOf のみ。image の詳細は Rust の型と Swift の実装に委ねる | 変更しない（D1）。テストで未知フィールド無視を固定する |

## 3. スコープ

### 3.1 対象（今回実装する）

- Phase 5 のテキスト同等 path: 画像拡張子（jpg/jpeg/png/heic/heif/hif/tif/tiff/webp、大文字小文字不問）の `kaseifu rename IMG_XXXX.HEIC`（または png/jpg）の dry-run（既定）/ `--execute` / Journal（move 行再利用）/ `undo`。既存 Phase 4 経路に乗せる。
- EXIF end-to-end: 撮影日時・カメラ・解像度が `{date}`・`{device}` に使われることの確認（既存メタデータ経路の再利用、二重実装なし）。
- Swift プロンプトの画像 hardening（2行）: 人物の特定禁止・断定的記述の禁止を target_type==image 時に明記し、実機で確認する。
- FFI 契約は fm-rename-v1 のまま（互換拡張で済ませる。schemas/ とテストで固定する）。
- 結合テスト `tests/v093_ai_rename_p5_test.rs`（Fake 注入、一時 dir のみ、HOME 隔離）。

### 3.2 対象外（着手しない）

- Phase 6（動画・AVFoundation）・Phase 7（フォルダ）・Phase 8（一括）。`--recursive` は生やさない。
- Foundation Models マルチモーダル配線（Attachment による画像内容理解）。SDK に公開 API が無いため本ラウンドでは配線しない。将来の接続点だけ残す（D1）。推測の代替実装（Vision 分類を FM 内容理解の代用にする等）は作らない。
- Vision OCR（任意）。送る。理由と次回条件を D5 に書く。
- README 等の利用者文書の整備（リリース時にまとめて行う）。
- 新規 Rust crate、新規設定ファイル・別 DB・独自キャッシュ・外部プロセス。Rust 側の画像デコード新規実装・新規画像デコード crate（禁止事項の遵守）。
- `organize` からの Rename Service 呼び出し、`discover` の公開、既存 JSON キーの削除・変更。

## 4. 設計判断（選択肢と結論。黙って決めない6点）

### D1. 画像入力の API 形と FFI 契約（fm-rename-v1 互換拡張で済むか）

- 実測（プローブ）:
  - probe2.swift（Attachment(image)）→ `error: cannot find 'Attachment' in scope`。probe3.swift（Attachment(imageURL:)）→ 同様に失敗。
  - 対照: probe_io.swift（ImageIO の read-only open＋thumbnail 縮小）→ typecheck 成功。Attachment を使わない FM 呼び出し → 成功。
  - SDK 実測: FoundationModels.swiftmodule/arm64e-apple-macos.swiftinterface（MacOSX.sdk / MacOSX26.sdk / MacOSX26.5.sdk の3者同一）に Attachment の公開定義なし（grep は logFeedbackAttachment のみ）。tbd に ImageSegment はあるが公開 Swift API ではない。
  - 公式一次情報（fetch）: Attachment は PromptRepresentable に適合し `session.respond { \"...\"; Attachment(image) }` で使う。`init(_: CGImage, orientation:)` と `init(imageURL:orientation:)` がある。framework が scaling・color conversion を行うため呼び出し側の scaling は不要。Vision OCR/barcode の system tool 連携もある。WWDC26 の新機能として記述。
  - 結論: 導入済み SDK（Xcode 26.6 / SDK 26.5）にマルチモーダル公開 API は無い。Phase 1 の PoC と同じ進め方（最小プローブで API 形を確定）の結果、本ラウンドではマルチモーダル配線を行わない。
- 選択肢A（採用）: fm-rename-v1 のまま。path フィールドが既にファイルパスを運んでおり（原典 §3.4 の Request 例どおり）、Swift は将来そのパスを ImageIO で読み込む。input.image の EXIF テキストは既に流れている。未知フィールド無視の前方互換により将来の任意ヒントも互換拡張で足せる。新スキーマ版は不要。schemas/ 無変更＋テストで固定する。
- 選択肢B（不採用）: 新スキーマ版（例 fm-rename-v2）。送る物が変わらないのに版を上げると Swift・Rust・テストの三点を同時に変える必要が生じ、今回の差分に見合わない。将来 Attachment 配線時に再検討する。
- 選択肢C（不採用）: FFI 越境で画像バイトを渡す。スコープで明示的に禁止（Rust はファイルパスのみ渡す）。採用しない。
- 将来の接続点: Swift 側は target_type==image かつ path がある要求に対し、ImageIO で read-only open→thumbnail（D2 のレシピ）→ Attachment(cgImage) を PromptBuilder に載せて session.respond(generating: KfSemanticPayload.self) する。Rust 側の変更は不要（path は既送）。SDK 更新後に最小プローブ→配線の順で着手する。

### D2. 入力画像の上限（辺・バイト数）と縮小方式、アスペクト比保持

- 選択肢A（採用・将来の配線用にレシピ確定、今回は配線しない）: 長辺上限 2048 px。ImageIO の thumbnail 生成（kCGImageSourceCreateThumbnailFromImageAlways=true、kCGImageSourceShouldCache=false、kCGImageSourceCreateThumbnailWithTransform=true、kCGImageSourceThumbnailMaxPixelSize=2048）でアスペクト比を保持する。元画像には一切書き込まない（read-only open のみ）。上限の根拠: 2048^2x4 byte ≒ 16 MiB（メモリ bound）、公式が scaling を肩代わりするため呼び出し側の縮小は最適化に留まること、OCR の細部と推論時間の均衡（1024 では小文字が潰れやすく、4096 ではメモリ・時間だけ増える）。probe_io.swift が typecheck 成功済み。
- 選択肢B（不採用）: 上限なし。100MP 級 TIFF でメモリ・推論時間が跳ねる。採用しない。
- 選択肢C（不採用）: 長辺 1024。OCR 精度・小物の判読が落ちる。採用しない。
- 今回の実装: FFI 越境で画像バイトを送らない（テキスト同等 path のみ）のため縮小の実行コードは入れない。縮小が必要になるのは将来の Swift 配線時であり、その時点で本レシピを kfRunGeneration の image 分岐に入れる。

### D3. EXIF 欠損時・デコード失敗時の扱い（原典 §12 の分類に従う）

- 前提の実測（plan-architect レビューで確定）:
  - (a) `resolve_date` は RFC 3339 のみ受理する。EXIF 正規化形（`YYYY-MM-DDTHH:MM:SS`、zone なし）は読み飛ばされる。現状のままでは EXIF 撮影日時が `{date}` に使われることは決してない。要件 #3 のため受理形式の拡張が必須である。
  - (b) 日付優先順位は作成→更新→EXIF→文書内→現在（Phase 3+4 の D5）。実ファイルは mtime を必ず持つため画像でも EXIF が選ばれることは事実上無い。写真の FS 日時はコピー・転送時刻であり撮影日時ではないため、画像では EXIF を先頭にする。
- 原典 §12: 画像デコード失敗→スキップ、必須意味情報不足→提案保留、推論タイムアウト・結果不正→スキップ、ファイル読み取り失敗→スキップ。
- 選択肢A（採用）:
  - ファイルが読めない（open 失敗・権限・消失）→ Unreadable エラーで終了（提案を作らない）。
  - 日付受理の拡張（`rules.rs` のみ）: RFC 3339 に加え zone 無し wall-clock 形（`%Y-%m-%dT%H:%M:%S` と `%Y-%m-%d %H:%M:%S`）を受理する。zone の推測はしない（wall-clock の年月日時分秒だけ使い offset を捏造しない）。影響範囲は画像の EXIF 値に限られる（文書の created/modified は offset 付きで従来どおり、文書内日付は別経路）。
  - 画像の日付優先順位: EXIF→作成→更新→現在（文書内日付は画像では使わない。画像に本文スキャンはしないため `document_ymd` は常に None）。文書の順位は変えない。根拠: 原典 §5.2 が撮影日時を画像の日付の主情報源とし、生成例が撮影日を示すこと。
  - EXIF 欠損（撮影日時・カメラなし）→ 上記順位でフォールバックする。device を含む P11 は missing: device で保留（捏造しない）。解像度欠損は続行する。
  - バイト列が壊れている→ Rust 側はデコードしないためクラッシュしない。メタデータは None として扱い、日付フォールバックで続行する。真のデコード失敗スキップは将来の Swift マルチモーダル分岐で inference-failed→スキップとして実装する。
  - 必須意味情報（title 等）不足→提案保留（既存 Held＋理由表示）。AI に埋めさせない。
- 選択肢B（不採用）: EXIF 欠損で即スキップ。日付フォールバック（原典 §6.4）があるのに捨てるのは過剰。採用しない。
- 選択肢C（不採用）: 欠損値を AI に補完させる。原典 §6.5（捏造禁止）に違反する。採用しない。

### D4. 画像推論の所要時間と DEFAULT_FM_TIMEOUT（20 秒）の adequacy

- 選択肢A（採用）: 20 秒を維持する。根拠: Phase 3+4 の実測（初回約 11 秒・ウォーム時 2〜4 秒、文書 dry-run 2.5〜8.2 秒）はテキスト同等 path の実力であり、画像のテキスト同等 path（送るのはパス文字列＋EXIF 数十文字。画像バイトを送らない）は文書と同程度のトークン量に収まる。本ラウンドの実機検証（§9）で画像 2〜3 件の所要時間を実測し、20 秒以内であることを確認する。マルチモーダル配線時（将来）に再実測し、超過の evidence が出た場合のみ根拠つきで変更する。
- 選択肢B（不採用）: 予告的に延長（例 60 秒）。evidence が無い変更はしない。採用しない。
- 選択肢C（不採用）: 短縮。初回 11 秒の実測があるため危険。採用しない。

### D5. Vision OCR を入れるか送るか

- 選択肢A（採用）: 送る。理由: (1) 原典は「必要時」と条件付きであり必須ではない（§5.2 の表は OCR を Vision とするが §11.3 は「不要なOCRを実行しない」）。(2) OCR を入れるには Swift 側に system tool の組み込み＋要求/応答の契約拡張が必要で、D1 のとおりマルチモーダル基盤自体が SDK に無い現状では土台の無い拡張になる。(3) テキスト同等 path の命名（日付＋タイトル）に OCR は不要であり、入れると Rust・Swift・テストの三点が膨らむ。次回条件: SDK に Attachment が来て画像内容理解が通った後、看板・文書写真等の OCR 需要を実測し、Swift tool 追加＋契約互換拡張の計画で着手する。
- 選択肢B（不採用）: 今回入れる。D1 のブロッカーを抱えたまま Swift 側を広げるのは順序が逆であり、スコープ（膨らむなら次回送り）の指示にも反する。採用しない。

### D6. 実機検証用の画像素材の用意方法（実ユーザーの写真に触れないこと）

- 選択肢A（採用）: 合成画像のみ。一時 dir 内に (a) 最小 PNG（IHDR 手書き。寸法のみ。EXIF なし。Fake テストと実機の共通素材）、(b) EXIF 付き TIFF（v32 の fixture_tiff パターン再利用。Make/Model/撮影日時/GPS を内包。Spotlight が拾わない一時 dir でも EXIF フォールバックで capture_time・camera が取れる）。HEIC 実ファイルは生成しない（生成手段が OS 依存で再現性が低い）。実ユーザーの写真・~/Pictures/・~/Desktop/・~/Library/Application Support/kaseifu/・実 Keychain・実補完 dir には一切触れない。rename --execute の実機確認は一時 dir 内の合成ファイルに限る。
- 選択肢B（不採用）: 実写真を使う。禁止事項に違反する。採用しない。
- 選択肢C（不採用）: ネットワーク画像を持ってくる。秘密情報・ライセンス・再現性の問題がある。採用しない。
- 限界の明記: 合成画像に写真的内容が無いため、実機の title はファイル名・メタデータ由来になり、真の視覚理解の検証にはならない。視覚理解の実証は SDK 更新後の次回に持ち越す。本ラウンドの実機検証は「画像で dry-run/execute/undo が通ること」「EXIF が {date}・{device} に反映されること」「人物名を捏造しないこと」の3点に絞る。

## 5. BDD シナリオ（振る舞いと具体例）

### 画像分類（src/rename/image.rs）

- I0 拡張子判定: IMG_1234.HEIC / photo.JPG / a.png / a.tif / a.webp → Image。movie.mp4 → Video エラー（Phase 6 の旨）。note.txt → Document エラー相当ではなく画像分類では NotImage として文書側へ委譲。拡張子なし → エラー（推測しない）。大文字小文字不問。
- I1 プロンプト注入の扱い: 画像に本文は無いため、ファイル名だけ検査する（you must rename を含む画像ファイル名 → 提案保留）。本文スキャンはしない。

### 画像プラン（plan_rename の画像分岐。Fake 注入）

- I2 dry-run 既定: IMG_1234.HEIC（Fake title 海辺の夕日、EXIF なし、mtime 2026-10-14）＋ P02 → 2026-10-14_海辺の夕日.HEIC を提案し、移動せず journal 不変。
- I3 execute＋undo: I2 の計画を execute → move 行が 1 行増える（success）。undo --last 1 で旧名に復帰し、二重 undo は停止する。
- I4 EXIF 日時（原典 IMG04）: EXIF 撮影日時 `2026-10-01T10:00:00` を持つ画像＋ P02 → 画像優先順位（D3）により日付部が `2026-10-01` になり `date_source` が `exif` になる。
- I5 機器（P11 meta）: camera Apple iPhone 17 を持つ画像＋ P11 → {date}_{device}_{title} が解決できる。camera なし＋ P11 → missing: device で保留（捏造しない）。
- I6 必須値不足: title 空 → 全パターン保留。未知変数 {unknown} → エラー（空置換しない）。
- I7 検証: 衝突先存在→上書き禁止、拡張子変更拒否、symlink 拒否、同名 No-op、AI 由来の危険文字（/ 等）は保留（黙って置換しない）。文書 D6 と同一基準。
- I8 読み取り失敗: 存在しないパス・削除済み → Unreadable エラー（提案を作らない）。壊れたバイト列→クラッシュせずフォールバック継続（D3）。将来の Swift デコード失敗は inference-failed→スキップに接続する。
- I9 モデル利用不可: 利用不可注入時は明示エラーで終了（フォールバックなし）。

### Swift プロンプト hardening

- I10 画像分岐の文言: target_type==image の要求に対し、プロンプトが人物の特定禁止・不明確な内容の断定禁止を含む。文書のプロンプトは変わらない（差分で保証する）。
- I11 実機: 合成画像（人物なし）の実推論で人物名が出ないこと。合成素材の限界上、強い拒否の実証にはならない旨を記録する。

### 非破壊（全ラウンド制約）

- N1: 既存 639 テストが全て通る。既存コマンドの挙動不変。
- N2: cargo fmt --check 違反 0、clippy 警告 0、新規 CC>=16 関数なし、complexity ゲート PASS。
- N3: 出力 ASCII のみ（生成ファイル名の非 ASCII を除く）・em dash なし・TUI なし・NO_COLOR/非 TTY 準拠。--format json は既存キー削除なし。discover 隠蔽維持。
- N4: テストは隔離 HOME・一時 dir・注入のみ。実ユーザーパス・実 Keychain・実補完 dir を読み書きしない。実ファイル移動なし。秘密情報を含めない。.env を表示しない。

## 6. タスク一覧（実装順・直列）

- T1 `src/rename/image.rs`（新規）: `ImageKind`・`classify_image`。拡張子判定は `context::image_metadata::is_image_file` を再利用し、二重の拡張子リストを持たない。I0/I1（単体テスト付き）。Rust の画像デコードは書かない。
- T2 `src/rename/service.rs`: 画像分岐（画像拡張子なら文書信号読みを飛ばし、ファイル名 injection 検査のみ。画像の日付は EXIF 優先（D3）で解決する）。`observe`・`finish_plan`・`execute_plan` は無変更で再利用。I2〜I9。CC 上昇を避けるため分岐関数は分割する。
- T2b `src/rename/rules.rs`: zone 無し wall-clock 日時の受理（D3 の (a)）。`parse_rfc3339` の拡張のみ。文書の既存テストは壊さない。
- T3 `src/rename/mod.rs`＋`src/main.rs`: `pub mod image` 公開と `rename_file` ゲートの画像対応（画像拡張子を先に判定し、文書判定の既存メッセージは変えない）。I2/I3 の CLI レベル。`document.rs` の画像向けエラーメッセージ文面は変えない（到達不能になるが API として残す）。
- T4 swift/kaseifu_fm_bridge.swift: 画像 hardening 2行（人物特定禁止・断定禁止。target_type==image 時のみ）。I10。Attachment 配線はしない。
- T5 契約固定のテスト（tests/v093 内または単体）: fm-rename-v1 の画像要求が serialize/parse できること、未知フィールドを無視すること。D1 の固定。
- T6 結合テスト tests/v093_ai_rename_p5_test.rs: I0〜I9 の CLI レベル＋Fake 注入テスト（一時 dir のみ、HOME 隔離）。
- T7 実モデル検証（分離。テストには含めない。§9 に追記する）: 一時 dir の合成 PNG/TIFF で dry-run/execute の実出力・所要時間・Journal/Undo 実測を記録する。HEIC は扱わない（D6）。
- T8 検収: cargo test --all-targets --all-features、cargo fmt --check、clippy、bash scripts/quality_check.sh、git diff 点検（commit しない）。

## 7. 検証手段（実測で示す）

- cargo test --all-targets --all-features（639＋新規の pass/fail 数）。
- cargo fmt --all -- --check、cargo clippy --all-targets --all-features -- -D warnings。
- bash scripts/quality_check.sh（OVERALL と complexity 表）。
- 実モデル: 一時 dir で dry-run/execute の実出力（提案名・所要時間）、journal 行、undo 復帰の実測。画像サイズ（byte・寸法）も記録する。
- git diff --stat と差分の目視（無関係変更・秘密情報の混入なし）。

## 8. 受け入れ基準（原典 §15 のうち本ラウンド分）との対応

- 2（意味情報抽出）: Phase 0-2 済み。本ラウンドは画像メタデータを image facts として追加投入する（テキスト同等）。視覚理解は SDK 待ちとして明示する。
- 3（一部）: 画像のリネームができる（動画・フォルダは対象外として明示エラー）。
- 4/5/6: 命名ルール選択・日付/Prefix/Suffix/カテゴリ/連番・カスタムテンプレートができる（画像でも同一エンジン）。
- 7/8/9: SQLite 保存・新設定ファイルなし・独自キャッシュなし。
- 10/11/12/13: dry-run 既定・上書き禁止・Journal 記録・Undo 動作。
- 17: 既存機能を壊さない（N1/N2）。
- 1/14/15/16: FFI・親子整合（Phase 7 へ）・利用不可時安全終了・FFI 安全性は Phase 0-2 の実績＋I9 で維持する。

## 9. 検収記録（追記式。実装後に実測値を埋める）

- cargo test --all-targets --all-features: **663 passed / 0 failed**（2026-10-14 実測。内訳: ベースライン 639＋回収分 21＋人物方針 3）。
- cargo fmt --check: 違反 0。clippy (`-D warnings`): 警告 0。complexity: CC>=16 が 0、最大 CC 15（更新前と同じ）、baseline-gate PASS。`bash scripts/quality_check.sh` = **OVERALL: PASS**（2026-10-14 実測）。
- 実モデル（隔離 HOME・一時 dir。詳細は §11.4）: 合成 PNG（33 byte、64x48）P02 dry-run 15.3 秒→`2026-10-14_shot.png`。合成 EXIF JPEG（268 byte）P02 dry-run 10.9 秒→`2026-10-14_IMG_1234.jpg`、P11 dry-run 11.4 秒→`2026-10-14_TestMake_TestModel_IMG_1234.jpg`（`{device}` が EXIF バイト由来であることの live 証拠）。PNG execute→journal `move` 行（success、`op-50822-...`）→`undo --last 1` で復帰、二重 undo は `nothing left to undo` で停止。人物示唆ファイル名（`young_woman_portrait.png`）5.3 秒→ファイル名エコーのみで氏名・外見属性の捏造なし。全件 20 秒タイムアウト内（D4 の adequacy を実測確認）。
- plan-architect 指摘: issues 記録の A1〜A7 を反映済み（初版の2件の見落としを修正）。人物記述方針は A1〜A7 と矛盾しない追加であり差し戻し不要（reviewer 記録に整合確認を残す）。
- reviewer / QA 指摘: `documents/plans/dev/issues/20261014-reviewer-ai-rename-p5.md`、`20261014-qa-ai-rename-p5.md` に記録。対応後に本節へ要旨を追記する（§11.6 参照）。
- commit / push / tag はしない（親スレッドが実施）。

## 10. リスクと対策

- マルチモーダル API が SDK に無い（D1 の実測）: 推測配線を作らずテキスト同等 path に絞る。視覚理解の実証は SDK 更新後の次回に持ち越す。到達点を本計画書と報告に明記する。
- 合成画像に写真的内容が無い（D6）: 実機 title はファイル名・メタデータ由来になる。視覚理解の検証ができない旨を記録し、人物画像等の実写真で代替しない。
- HEIC 実ファイルの扱い: 生成・検証ともに今回扱わない。HEIC は拡張子判定と EXIF ベストエフォートの範囲で受け付けるが、実機検証は PNG/TIFF で行う。
- 実モデル所要時間: 初回約 11 秒の実績があるため直列実行し 20 秒以内を確認する。超過時は Fake での検証到達点を報告する。
- Unicode NFC の完全比較ができない（Phase 3+4 の残リスクを継承）: バイト一致 No-op＋exists 衝突検査で実装し、残リスクとして報告する。

## 11. 引き継ぎ更新履歴（2026-10-14。Phase 5 完成。中断からの回収）

本節は追記のみであり、§1〜§10 の原文は残す。ファイル名は変えない。

### 11.1 選別結果（使った部分／作り直した部分と理由）

- 使った部分（そのまま継承）: `src/rename/image.rs`（`is_image_file` 再利用の分類。drift なし）
  ／`src/rename/rules.rs` の `parse_datetime`＋`resolve_image_date`（A1・A2 の反映）
  ／`src/rename/service.rs` の画像／文書分岐（小関数分割で CC 上昇なし）
  ／`src/main.rs` の `reject_foreign_rename_target`＋`src/rename/mod.rs`＋`tests/v092` の更新
  ／`tests/v093_ai_rename_p5_test.rs` の全 18 テスト（隔離・決定的。A6 の相関方式を含む）。
  理由: `cargo test` 660 passed / fmt・clippy clean・quality OVERALL PASS を回収状態で実測し、
  設計も A1〜A7 と整合していたため。壊れた中途半端な部分は無かった。
- 作り直した部分: Swift 画像プロンプトの文言（旧2行の一般的な人物特定禁止を、承認済み人物記述方針の
  具体的な禁止属性＋非人物要素の命名＋人物のみ時の保留に置き換え。理由: 前任は本方針を知らず、旧文言では
  性別・年齢等の外見属性が対象外だった）／`tests/v093` へ方針テスト 3 件を追加／本 §11 と §9 の実測追記
  ／reviewer・QA 記録の新規作成。Rust 側の決定論的ガードは方針 5 に従い作らない（作らない理由も記録する）。

### 11.2 D1・D2 の再検証（回収分を鵜呑みにせず再実測。結論は同じ）

- D1（FFI 契約）: 当日 SDK（Xcode 26.6 同梱 MacOSX.sdk、Swift 6.3.3、`sw_vers` 26.4.1）で `Attachment` の公開定義なしを再確認。
  swiftinterface 内の `Attachment` 5 件は全て `logFeedbackAttachment`（feedback 用）で画像入力 API ではない。
  最小プローブ `Attachment(imageURL:)` は `cannot find 'Attachment' in scope` で失敗。
  よって fm-rename-v1 互換拡張のまま（schemas 無変更）。将来の接続点（path を ImageIO で読む）は D1 の記載どおり。
- D2（上限・縮小）: ImageIO の thumbnail レシピ（`kCGImageSourceThumbnailMaxPixelSize=2048`、
  アスペクト比保持、read-only open）が typecheck 成功することを再確認。今回は FFI 越境で画像バイトを送らないため
  実行コードは入れない（前任計画どおり）。上限 2048 の根拠（16 MiB bound・OCR 精度と時間の均衡）は維持する。
- D3（EXIF 欠損時分類）・D5（Vision 送り）・D6（合成素材のみ）: 前任計画どおり。変更なし。
- D4（20 秒 adequacy）: §11.4 のとおり実画像 4 件（5.3〜15.3 秒）で確認し、20 秒を維持する。

### 11.3 人物記述方針の反映（ユーザー承認済み5項目の実装対応）

1. 氏名・外見属性（性別・年齢・人種・身体・服装・髪型・続柄・役割・人数）を表示・検索に使わない:
   Swift 画像プロンプト 1 行目
   （`Never output a person's name, and never describe people by gender, age, ethnicity, body, clothing,
   hairstyle, relationship, role, or headcount.`）と Instructions の画像分岐に明記。文書側は byte-identical。
2. 氏名の三類型: (a) 私人氏名の捏造は既存の `never invent facts` で禁止（継続）。
   (b) 有名人認識は上記 1 行目で禁止（継続・明確化）。(c) OCR 人名の転記禁止は Vision 採用時に再定義する
   （今回は OCR 対象外のため、D5 の次回条件に含める）。
3. Rust 側のデータに人物属性を持ち込まない: `FmImageFacts` は撮影日時・カメラ・解像度のみ
   （テスト `v093_image_facts_carry_no_person_fields` でキー固定）。画像プランは本文スキャンをしない
   （テスト `v093_image_requests_carry_no_body_signals` で `T:` 信号ゼロを固定）。journal は move 行
   （パス＋rule label）のみで人物スロットを持たない。表示・ログも提案名以外に人物由来の値を出さない。
4. 人物以外の要素で命名し、人物以外に言いようがなければ保留: プロンプト 2 行目
   （`Name the file from non-person elements such as place, object, event, time of day, or animal;
   when only people could be described, leave the fields empty.`）。空 title→`missing: title` 保留の既存経路に接続
   （テスト `v093_plan_holds_when_the_title_is_missing` が機構を固定）。自撮りに例外なし（文言に例外規定なし）。
5. 決定論的ガードは採用しない: 日本語の誤検出を避けるためプロンプト指示を主とし、実機の目視確認で担保する
   （§11.4 の目視結果）。Rust 側の出自フィルタは作らないと明示する。

旧 T4（hardening 2行）は上記の 3 行＋Instructions 拡張に置き換わった。
旧 I10（人物特定禁止・断定禁止の文言）は上記 1・4 と断定禁止（`leave a field empty when unsure`）に拡張された。
旧 I11（実機で人物名が出ないこと）は §11.4 の目視確認に置き換わった（合成素材の限界の注記は維持する）。

### 11.4 実モデル検証の記録（T7。隔離 HOME・一時 dir・合成ファイルのみ）

- 素材: (a) 最小 PNG 33 byte（64x48、EXIF なし）。(b) EXIF 付き JPEG 268 byte
  （DateTimeOriginal `2025:08:10 15:30:00`、Make `TestMake`、Model `TestModel`、SOF 64x48。
  `tests/v093` の Rust fixture と同一レイアウトであることをバイト比較で確認）。HEIC 実ファイルは扱わない（D6）。
- dry-run: PNG P02 が 15.3 秒で `2026-10-14_shot.png`（`date_source: exif`。Spotlight capture 由来。
  合成 PNG に写真的内容が無いため title はファイル名由来）。JPEG P02 が 10.9 秒で `2026-10-14_IMG_1234.jpg`。
  JPEG P11（meta）が 11.4 秒で `2026-10-14_TestMake_TestModel_IMG_1234.jpg`
  （`{device}` が EXIF バイト由来であることの live 証拠。Spotlight は当該ファイルに acquisition 情報を索引せず）。
- 日付の注記: live の `{date}` は Spotlight capture（当日）が EXIF-fallback より優先された。
  EXIF 日時→`{date}` の順序自体は `resolve_image_date` の単体テストと `v093` の相関テストで決定的に固定する
  （plan-architect A6 の設計どおり。Spotlight 状態に依存する日付断言は live ではしない）。
- execute／undo: PNG の `--execute` で `status: executed`（`operation_id: op-50822-...`、journal `move` 行 success）。
  `undo --last 1` で旧名に復帰し、二重 undo は `nothing left to undo` で停止する。
- 人物の目視確認: 全ての live 提案に氏名・外見属性は含まれない（`shot`、`IMG_1234`、機器名のみ）。
  人物示唆ファイル名（`young_woman_portrait.png`）の probe（5.3 秒）はファイル名エコー
  （`2026-10-14_young_woman_portrait.png`）に留まり、氏名の捏造は無い。ただし本ラウンドは画像バイトを
  送らないため、人物写真の視覚理解に対する方針の効き目は未検証であり、Attachment 配線後の次回に持ち越す
  （真の目視確認の条件: 画像入力が通った後、実ユーザーの写真を使わず許諾済み・合成の人物画像で確認する）。
- 非破壊の確認: 動画は `.mp4 is a video; video rename arrives with Phase 6` で失敗継続。
  文書（txt）は従来どおり提案する。`--format json` のキーは追加なし・削除なし。
- 所要時間の総括: 4 件とも 20 秒以内（5.3〜15.3 秒。初回が最も遅く、ウォーム後は 5〜11 秒）。D4 の維持根拠とする。

### 11.5 plan-architect 記録との整合（差し戻し不要の根拠）

既存記録（`20261014-plan-architect-ai-rename-p5.md`）の A1〜A7 は全て実装に反映済みであり、
人物記述方針は A5（画像分岐内に限り文書差分ゼロ）の内側の文言拡張として収まる。契約・順序・隔離・CC・OCR送りの
判断を変更しないため、plan-architect への差し戻しは不要と判断した（reviewer 記録に確認内容を残す）。

### 11.6 reviewer／QA（フルサイクルの close）

- reviewer 記録: `documents/plans/dev/issues/20261014-reviewer-ai-rename-p5.md`（承認）。
- QA 記録: `documents/plans/dev/issues/20261014-qa-ai-rename-p5.md`（承認。663 passed / OVERALL PASS 実測）。
- §9 の reviewer／QA 行の要旨追記は本 §11.6 をもって行う。
- Vision OCR は次回送り（D5）。

### 11.7 人物記述方針の解除（ユーザー決定。§11.3 を上書き）

§11.3 で実装した人物記述の制限（氏名・外見属性の禁止、非人物要素優先、人物のみなら保留）は、
**ユーザー決定により解除した**（同日）。理由は次のとおり（決定の要旨）:

- 本ツールは Apple のオンデバイスモデルの上に載るものであり、写真の意味づけと属性の選定は
  モデル（および Jev）の責務である。ツールはその出力を受けて並べ替え・リネームするだけなので、
  出力内容の制限は行わない。
- 特定につながる属性の判別はモデル側の問題であり、本ツールはモデル出力がプライバシーに
  つながるか否かを関知しない。属性は本ツールにとってアトリビュートの 1 つにすぎない。
- 帰属は README の Safety 節に明記する（利用者への説明）。

現在の画像プロンプトに残る唯一の規則は「不確実な内容を断定しない（空欄にする）」であり、
これは正確性のための規則であって人物記述の制限ではない。prompt-injection の保留
（文書・ファイル名）と dry-run 既定は別件の安全策として維持する。

本節の解除により、§11.3 および reviewer 記録の人物方針該当箇所は**履歴**として残る
（コード上の制限は解除済み）。

### 11.8 画像日付の修正（EXIF 優先の実装。別コミット）

§11.4 の実機検証で `date_source: exif` と表示されながら日付が
ファイル作成日（当日）になる事象を親が検出し、原因を特定して修正した:

- 原因 1: `apply_spotlight_capture` が `kMDItemContentCreationDate`（ファイル作成日）で
  capture 枠を先に埋めるため、ファイル内 EXIF の DateTimeOriginal が読まれなかった。
- 原因 2: `resolve_image_date` は capture 値の出所にかかわらず source を `exif` と
  表示していたため、作成日が「EXIF 由来」に見えていた。
- 修正: capture は Spotlight の EXIF 属性 → ファイル内 EXIF のみとし、作成日は
  capture としては使わない（`creation_date` として別途保持）。EXIF が無い画像は
  created → modified → now に落ち、表示も `created`/`modified` になる。
- 実測: 標準配置（Exif IFD）の EXIF JPEG は `2025-08-15_standard.jpg`
  （`date_source: exif`）、EXIF なし PNG は当日（`date_source: modified`）。
- 既知の限界: EXIF パーサは 0x9003 を Exif IFD（0x8769 経由）からのみ読む。
  PIL が書く「IFD0 直下の 0x9003」は非標準配置として読まない（実測）。
- 受け入れ: `src/context/image_metadata.rs` と `tests/v32_image_metadata_test.rs`
  の修正として別コミットで着地（`6d668b4`）。単独で 640 passed / 0 failed を実測。

### 11.9 次ラウンド（Vision ハイブリッド）

ユーザー判断により、知覚層の不足（SDK に画像入力 API が無いため画像バイトがモデルに届かない）
は Vision の `VNClassifyImageRequest` で補い、Foundation Models は日本語タイトル化に使う
ハイブリッドを次ラウンドで追補する。§11.4 の「真の視覚理解への方針の効き目は未検証」は
そのラウンドで再評価する。
