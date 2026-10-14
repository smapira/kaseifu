# Apple Foundation Models 統合（AI リネーム基盤）Phase 0-2 実装計画

- 起票: 2026-10-14
- 原典: `documents/plans/user/20261014a.md`（kaseifu - Apple Foundation Models 統合・AI リネーム機能 開発指示書 v2.0、全 1127 行。§3 FFI、§3.3 非同期、§3.4 データ交換、§4.2 利用条件、§5.1 文書解析、§12 エラー処理、§14 開発フェーズ、§15 受け入れ基準 17 項目）
- 前提: HEAD `9be5f71`（v0.8.0、clean）。`documents/plans/user/20261014b.md`（TREC Benchmark）は**別プログラムであり本計画の対象外**（読むだけ・計画しない）
- 今回の増分: **Phase 0（既存実装調査）+ Phase 1（FFI PoC）+ Phase 2（Semantic Provider）のみ**
- 対象外（実装しない）: Phase 3 以降（`rename` サブコマンド、命名テンプレート / プリセット P01-P12、実際のリネーム実行、Journal / Undo 統合、画像 / 動画 / フォルダ / 一括処理、性能評価）。原典 §15 の受け入れ基準のうち 3-7、10-14 は後続ラウンドで満たす
- 境界の理由: FFI と Swift ビルド統合は配布（`cargo install`）と CI への影響が最も大きく、独立した増分としてレビューしたい。命名ルールとリネーム実行はこの土台が固まってから載せる
- 制約: 新規 Rust crate 依存なし（build.rs は `xcrun` / `swiftc` / `ar` を起動するだけ）/ 新しい設定ファイル・別 DB・独自キャッシュ・外部プロセスなし / 既存 organize / watch / classify / inspect / doctor / undo / uninstall の挙動を壊さない（受け入れ 17）/ CLI 面は不変（`src/main.rs` の `enum Commands` に触れない）/ 出力 ASCII のみ / commit・push・tag は親が実施（本スレッドでは行わない）/ `cargo install` は実行しない（`cargo build` と `cargo build --release` まで）
- ベースライン実測（2026-10-14、worktree `kaseifu`、HEAD `9be5f71`、clean、本スレッドで再実行）:
  - `cargo test --all-targets --all-features` = **450 passed / 0 failed**
  - 親スレッド実測: `bash scripts/quality_check.sh` = **OVERALL: PASS**（rustfmt 0 / clippy 0 / CC>=16 = 0 / 最大 CC 15 / coverage 85.26% report-only）
- 確認手段: serena / context7 / codebase-memory-mcp / graphify は本環境で未接続のため、**実コードの直接読解 + `grep`** と、**SDK 一次情報（`MacOSX26.5.sdk` の `FoundationModels.swiftinterface`）の直接読解**、および**実機スパイク（Swift オブジェクト生成 -> アーカイブ -> Rust バイナリへのリンク -> 実行）**で確認した（§3 の「確認手段（実測）」に証跡）。

---

## 1. Phase 0-9 の全体地図と今回の増分

| Phase | 内容 | 今回 | 後続ラウンドで必要になるもの（既存資産 / 新規） |
|---|---|---|---|
| 0 既存実装調査 | Rust 構成 / CLI / SQLite / Journal / Undo / FileContext / Spotlight / xattr / 既存 LLM Provider / ビルド配布 / 対応 macOS | **完了（本計画 §2）** | - |
| 1 FFI PoC | Swift ライブラリのビルド / Rust からの呼び出し / FM 推論 / 結果受け取り / メモリ管理 | **本ラウンド（推論を除く。§2.4 の外部ブロッカー）** | Apple Intelligence 有効化後に `cargo run --example fm_bridge_smoke` 1 コマンドで再確認 |
| 2 Semantic Provider | FileContext -> 入力変換 / Swift 推論 / 構造化結果 / エラー処理 / 利用可能性チェック | **本ラウンド** | - |
| 3 Naming Rule Engine | テンプレート / プリセット / 日付形式 / Prefix / Suffix / 連番 / SQLite 設定管理 | 対象外 | 既存 `storage::history` の DB を再利用（`operations` テーブル + 新規テーブルはマイグレーションで追加）。原典 §6.2 の P01-P12、§6.4 の日付形式 |
| 4 Document Rename | PDF / TXT / Markdown、dry-run / execute / journal / undo | 対象外 | 既存 `operations::{plan,mover,undo}`、`storage::history::record_pending`（変更前に記録）、`context::builder` + `normalize`。文書テキスト抽出は新規（§5.1 のトークン制限つき） |
| 5 Image Rename | 画像入力 / ImageIO / EXIF / FM 画像理解 | 対象外 | 既存 `context::exif` / `context::image_metadata` / `normalize::image` を再利用。FM の画像入力 API は §4.3（Swift 側の拡張） |
| 6 Video Rename | AVFoundation / 代表フレーム 3-5 枚 / フレーム解析 | 対象外 | Swift 側に AVFoundation 追加（§5.3）。Rust 側は同じ Semantic Provider 契約 |
| 7 Directory Rename | フォルダ解析 / サンプリング / 親子整合 | 対象外 | 既存 `platform::macos::filesystem::read_directory`（100 件上限）を再利用 |
| 8 Batch Rename | 再帰 / 一括プレビュー / 衝突検出 / 一括 Undo | 対象外 | 既存 `operations::organize` の計画 -> 確認 -> 実行の型 |
| 9 Performance / Quality | 推論時間 / FFI オーバーヘッド / セッション再利用 / 大規模フォルダ / 長時間動画 / メモリ / 異常終了 | 対象外 | 本ラウンドの設計判断 D3（非同期・キャンセル）と §11.5 のセッション再利用はここで評価する |

今回の増分が後続を安くする理由: (a) C ABI と JSON 契約（`fm-rename-v1`）が確定し、Phase 3-9 は Rust 側の追加だけで済む、(b) Swift 側は「モデル呼び出し」だけを差し替えれば画像 / 動画へ拡張できる構造（`runGeneration` の分岐点が 1 箇所）、(c) 配布（`cargo install`）と CI の縮退経路がテストで固定される。

---

## 2. Phase 0: 既存構造の実測（変更前）

### 2.1 再利用する既存構造（実測表）

| 対象 | 実測（`file:line`） | 今回の扱い |
|---|---|---|
| モジュール一覧 | `src/lib.rs:22-37`（`auth` ... `watch`） | `semantic` を 1 つ追加（既存モジュールは不変） |
| CLI 定義 | `src/main.rs:67-251` `enum Commands`（可視 11 + 隠し 1）、dispatch は `src/main.rs:304-371` の 1 箇所 | **触らない**（rename は Phase 3 以降。CLI 面の変更ゼロ） |
| Provider 契約の前例 | `src/classifier/jev.rs:84-90` `trait JevIntegration: Send + Sync`（`Pin<Box<dyn Future<Output = ...> + Send + '_>>` を返す）、`src/classifier/mock_jev.rs:19-100`（設定値をそのまま返す mock）、`src/classifier/pipeline.rs:36-58`（`&dyn JevIntegration` 注入） | `trait SemanticProvider` を**同型**で作る（本番 `FoundationModelsProvider` + `fake::FakeSemanticProvider`） |
| 観測モデル | `src/models/file_context.rs:5-10` `FileContext`、`:68-81` `NormalizedFileContext`、`:88-138` `ImageMetadata` / `CaptureTime` / `CameraInfo` | `NormalizedFileContext` を入力に使う（`FileContext` は既存 `normalize` を通して正規化済みの形。§5.1 の「FileContext -> ...」は既存 pipeline の Phase 1 -> 2 に相当） |
| Context 構築 | `src/context/builder.rs:10-63`（canonicalize / メタデータ / `read_directory` 100 件 / Spotlight / xattr） | 再利用（変更なし）。Phase 4 で `build` -> `normalize` -> `analyze` の順に繋ぐ |
| 正規化 | `src/normalize/mod.rs:15-28`（`normalize` / `normalize_with_options`）、`src/normalize/metadata.rs`（`M:` などの信号） | 再利用（変更なし） |
| SQLite / Journal | `src/storage/history.rs:16` `JOURNAL_FILE_NAME`、`:26` `LEGACY_DATABASE_PATH`、`:35-37` `journal_path()`、`:86-104` `operations` テーブル（id / timestamp / operation / source_path / destination_path / candidate_id / confidence / margin / result / error / undone_at）、`:110-137` `record_pending`（**変更前に記録**） | 今回は**触らない**（Phase 3 で命名ルール保存に再利用、Phase 4 でリネーム履歴に再利用） |
| Undo | `src/operations/undo.rs:38-46`（Journal のみから逆操作、衝突は拒否） | 今回は触らない（Phase 4） |
| Doctor | `src/doctor.rs:1-23`（読み取り専用の診断、`CheckResult` は next アクション付き） | 今回は触らない。将来の `Foundation Models` 行（利用不可理由の表示）の受け皿 |
| 設定 | `src/config/storage.rs:15-23`（`~/Library/Application Support/kaseifu/config.toml` / `app_dir()`） | 新規設定ファイルを**作らない**（原典 §7.1 / 受け入れ 8） |
| スキーマ | `schemas/{file-context-v1,jev-decision-v1,candidate-destination-v1}.json`（draft 2020-12 / `$id` / `title` / `type` / `required` のみの最小様式。コードからの参照は無く文書として使われる） | 同じ流儀で `schemas/fm-rename-v1.json` を追加 |
| ヘルプ契約 | `tests/v070_zero_config_p1_help_test.rs:143-227`（可視コマンドを**自動列挙**して禁止語・非 ASCII を検査。隠し `discover` も明示的に対象） | 新コマンドを足さないため対象は増えない（テスト変更不要） |
| README 契約 | `tests/v080_zero_config_finish_test.rs:77-140`（Quick Start が冒頭 / 見出し・アンカー解決） | 開発者向けの追記はこの契約を壊さない範囲で行う（§4.6） |
| ビルド / 配布 | `Cargo.toml`（edition 2024、`build.rs` なし、`[[bin]] kaseifu`）、配布は `cargo install --path .` | `build.rs` を新設。**実行はしない**（親がリリース時に確認） |
| CI | `.github/workflows/rust-quality.yml`（`macos-latest` / Rust 1.98.1 / `bash scripts/quality_check.sh`） | ワークフロー変更なし。ランナーの Xcode が 26 未満でもビルドが通る設計（D2） |
| 品質ゲート | `scripts/quality_check.sh`（rustfmt / clippy -D warnings / cargo test / complexity 解析は `-p src` / baseline-gate / report-tests / coverage report-only） | 新規コードも同じゲートを通す（新規 CC>=16 なし、既存最大 CC 15 を悪化させない） |
| 隔離テストの前例 | `tests/v070_zero_config_p1_cli_test.rs`（隔離 HOME + 偽 mdfind + HTTP モック）、`tests/v080_zero_config_uninstall_test.rs`（注入ストア） | 新規テストは**ファイルシステムに触れない**（パスを一切開かない。fake 注入と JSON 契約のみ） |

### 2.2 既存 LLM Provider（Jev）との関係

- 既存は HTTP（`RealJevIntegration`、`src/classifier/jev.rs:129-200`、タイムアウト 15 秒 `:139`）で、外部サービスへの依存。今回の Foundation Models は**同一プロセス内の FFI**（原典 §3.1）であり、外部プロセス / JSON Lines / XPC を使わない。
- Jev は「候補の中から選ぶ」決定（`JevDecision`）を返すのに対し、Foundation Models は「意味情報（title / category / project）」を返す。**混ぜない**: 別 trait（`SemanticProvider`）とし、Jev への暗黙フォールバックは行わない（原典 §4.2 / §12 / 受け入れ 15）。
- 注入方式（trait + mock）は Jev の前例をそのまま踏襲する。

### 2.3 環境実測（本スレッドで再確認）

- macOS 26.4.1（build 25E253）/ arm64。Xcode `/Applications/Xcode.app/Contents/Developer`、SDK `MacOSX26.5.sdk`。Swift 6.3.3（swift-driver 1.148.6）、`swiftc -print-target-info` の既定 target は `arm64-apple-macosx26.0`。
- **Apple Intelligence の状態は作業中に変化した**。セッション開始時点では `SystemLanguageModel.default.availability` = `unavailable(.appleIntelligenceNotEnabled)`（`supportedLanguages` 23）だったが、作業中の有効化により **available** になり、本ブリッジ経由の実推論が成功した（実測は §2.4 と §9）。有効化前・未有効の機体では同じ経路が `unavailable: appleIntelligenceNotEnabled` / `modelNotReady` / `deviceNotEligible` をそのまま返す（`foundation-models-sdk-missing` / `swift-toolchain-missing` / `unsupported-os` も含めてテストで固定）。
- SDK 一次情報（`FoundationModels.swiftinterface`）で確認した API: `SystemLanguageModel.default.availability`（`.available` / `.unavailable(reason)`、reason は `deviceNotEligible` / `appleIntelligenceNotEnabled` / `modelNotReady`）、`LanguageModelSession(model:instructions:)`、`respond(to:generating:)`（`@Generable` + Optional プロパティ可）、`GenerationOptions`、`Instructions`。
- 実機スパイクで確定したリンク事情（詳細は D1）: (a) `swiftc` は `xcrun` 経由で呼ばないと標準ライブラリを見つけられない、(b) `import FoundationModels` は強い autolink を出すので weak link には抑制が必要、(c) Swift の後方互換ライブラリには toolchain の探索パスが必要、(d) deployment target < 12.0 だと Swift concurrency が `@rpath` 参照になり起動時に dyld が abort する。

### 2.4 Phase 1 の完了条件に対する本ラウンドの結果

| 原典 §14 Phase 1 の完了条件 | 本ラウンド |
|---|---|
| Swift ライブラリのビルド成功 | **達成**（`build.rs` が swiftc でコンパイル -> `ar` でアーカイブ -> Rust へ静的リンク） |
| Rust からの呼び出し成功 | **達成**（`kaseifu_fm_available` を実機で呼び、JSON を受け取る） |
| Foundation Models の推論成功 | **達成（実測）**: ブリッジ経由の `respond(to:generating:)` が title/category を返した（初回 11.1 秒 = モデルロード含む。§9 に実出力）。Apple Intelligence 未有効の機体では同じ経路が理由つき unavailable を返す（テストで固定） |
| 結果の受け取り成功 | **達成**（コールバック -> JSON -> Rust パース。成功時は `SemanticInfo`、利用不可時はエラー JSON） |
| メモリ管理の検証 | **達成**（所有権契約 + exactly-once + destroy 後コールバック禁止 + `leaks --atExit` が **0 leaks**） |

### 2.5 再利用しなかったもの（理由）

| 候補 | 判断 | 理由 |
|---|---|---|
| `JevIntegration` の実装流用 | しない | 返すもの（決定 vs 意味情報）と境界（HTTP vs FFI）が別。`JevError` の分類も合わない |
| `OperationHistory` の新規テーブル | 今回はしない | 命名ルール / リネーム履歴は Phase 3-4。DB を先に変えない（原典 Phase 0 の「調査完了前に新しい設定ファイルや DB を作らない」） |
| `doctor` への FM 行追加 | 今回はしない | CLI 面の変更をゼロに保つ。理由つき利用不可は `available()` の JSON とテストで固定する |
| `config/default.toml` への設定追加 | しない | 新規設定ファイル / 新規キーを追加しない（原典 §7.1、受け入れ 8） |
| Swift Package Manager / Xcode プロジェクト | しない | `swiftc` 直呼びで十分（新規依存なし・単一ファイル・C ABI のみ）。SPM はビルドキャッシュと配布の複雑さを増やす |

---

## 3. 設計判断（選択肢と結論）

### D1. Swift 成果物の作り方とリンク方法

- **結論**: `swiftc -parse-as-library -emit-object` -> `ar crs $OUT_DIR/libkaseifu_fm_bridge.a` -> `cargo:rustc-link-search=native=$OUT_DIR` + `cargo:rustc-link-lib=static=kaseifu_fm_bridge`。最小 OS は **12.0**（arm64 / x86_64 とも）。FoundationModels は **weak link**。Swift ランタイムは OS 内（`/usr/lib/swift`）を使い、rpath は不要。
- **weak link の作り方（実測で確定）**: Swift コンパイラは `import FoundationModels` から**強い** autolink（`LC_LINKER_OPTION: -framework FoundationModels`）を出す。これを `-Xfrontend -disable-autolink-framework -Xfrontend FoundationModels` で抑止し、Rust の最終リンクに `-weak_framework FoundationModels` を渡す。実測: 生成バイナリの `otool -L` が `... FoundationModels (... , weak)` を示す（`LC_LOAD_WEAK_DYLIB`。リリースバイナリでも確認済み）。
- **deployment target は 12.0（実測で必須と判明）**: `Task`（非同期ブリッジ）が使う Swift concurrency ランタイムは macOS 12 以降 OS に含まれる。リンクの deployment target が 11.0 のままだとリンカが `@rpath/libswift_Concurrency.dylib` を要求し、**Xcode の無いマシンでは dyld が起動時に abort する**（この機体で再現: `Library not loaded: @rpath/libswift_Concurrency.dylib ... no LC_RPATH's found`）。対策は (a) Swift オブジェクトを `-target *-apple-macosx12.0` でコンパイルし、(b) Rust のリンクに `-mmacosx-version-min=12.0` を渡すこと。これでロードが OS パス（`/usr/lib/swift/libswift_Concurrency.dylib`）になり、macOS 12-15 でも起動できる。
- **影響（明示）**: バイナリの最小 macOS が 11.0 -> **12.0** に上がる。macOS 11（Big Sur、2023 に EOL）では kaseifu が起動しなくなる。代替案（11.0 を保つために `-rpath /usr/lib/swift` を足し、macOS < 12 では `Task` を使わず即時拒否する）は、macOS 11 上で concurrency シンボルに触れた瞬間に落ちるリスクを検証できないため捨てた。**採用した案は 12 以降で確実に動くことを実測で確認できる**。
- **補足（実測で必要と判明）**: 最小 OS を 12.0 にしても Swift の後方互換ライブラリ（`libswiftCompatibility56` / `Packs`）が autolink されるため、ツールチェーンの `usr/lib/swift/macosx` を `-L` に追加する。
- **`cargo install --path .` で成立するか**: 成果物は `OUT_DIR` に置かれ、最終バイナリへ静的に取り込まれる（dylib ではない）ため、バイナリ移動後も動く。`build.rs` は install 時にも実行される。
- **捨てた代替**: dylib リンク（`cargo install` でバイナリだけが移動し `@rpath` が壊れる）/ `swiftc -emit-library -static`（単一 `.o` + `ar` と等価で、余分な抽象）/ 外部プロセス・XPC（原典 §3.1 で禁止）/ `-target` 省略（既定が `macosx26.0` になり macOS 26 未満で起動不能）/ `-static-stdlib`（システムフレームワークと Swift ランタイムが二重化し未定義動作の危険）。

### D2. SDK に FoundationModels が無い環境 / macOS 26 未満での縮退

- **ビルド時**: build.rs が (1) `xcrun --find swiftc`（無ければ `swiftc`）で Swift ツールチェーンを探し、(2) `import FoundationModels` の typecheck プローブで SDK の対応を判定する。(1)(2) のいずれかが失敗したら `cargo:rustc-cfg=kaseifu_fm_stub="no-toolchain"|"no-sdk"` を立て、**Rust 側スタブ**（同じ 6 関数の C ABI）をビルドする。スタブは `available()` が `{"status":"unavailable","reason":"..."}` を返し、`generate` は受理したリクエストに対し「利用不可」のエラー JSON をコールバックで正確に 1 回返す。**ビルドは成功し、既存コマンドはそのまま動く**。
- **呼び出し方法（実測で必要と判明）**: コンパイラは常に `xcrun swiftc` 経由で起動する。`swiftc` バイナリを直接呼ぶと標準ライブラリを見つけられず（`unable to load standard library for target ...`）プローブが失敗し、**全ビルドが黙ってスタブに縮退する**（実測: 最初の実装で発生し、修正）。xcrun が無い場合のみ直接呼び出しにフォールバックし警告する。
- **実行時（macOS 26 未満）**: weak link により起動は成功し、Swift 側が `#available(macOS 26.0, *)` で `unsupported-os` を返す。既存コマンドは Foundation Models を呼ばないため無影響。
- **ローカル検証用の build-time スイッチ**: `KASEIFU_FM_FORCE_STUB=<no-sdk|no-toolchain>`（既定 off。指定時はその理由でスタブをビルド）を用意し、**スタブ経路をこの機体でも同一テストで検証**できるようにする（CI 相当の再現。実測済み）。`cargo:rerun-if-env-changed` を宣言する。
- **捨てた代替**: 別途 Swift のスタブファイルをビルド（検証ロジックが二重化しドリフトする）/ リンク失敗時の自動リトライ（失敗原因が不明瞭）/ macOS 26 未満を切り捨てて minos 26 でビルド（既存コマンドの動作環境を狭めるため不可）。

### D3. 非同期・キャンセル・タイムアウト・所有権（C ABI での表現）

- **呼び出し形**: `generate(context, request_json, callback, user_data) -> int32`。戻り値は**受付判定**のみ: `0` = 受理（コールバックは**正確に 1 回**発生）、非 0 = 即時拒否（コールバックは**発生しない**）。拒否コード: `1` null 引数 / `2` 不正 JSON・必須フィールド欠落 / `3` schema_version 不一致 / `4` 同一コンテキストで実行中 / `5` 破棄中 / `7` 内部エラー。
- **exactly-once**: Swift 側は `NSLock` + `finished` フラグで完了経路を 1 本化し、二重完了を防止する。`cancel` は Task を cancel するだけで、コールバックは完了経路からのみ（キャンセルも含めて 1 回）。
- **destroy 中のコールバック禁止**: `destroy` は `destroyed` を立て、実行中なら完了シグナルを待つ（上限 5 秒。コールバック自体は実行しない）。待機後にコンテキストの参照を解放する。実行中 Task は ARC でコンテキストを強参照しているため、待機がタイムアウトしても use-after-free は起きず、Task 完了時に自然に解放される（リークしない）。
- **Rust 側の所有権（リークしない設計）**: コールバックの `user_data` は**小さな整数キー**で、プロセス全体のレジストリ（`Mutex<HashMap<u64, oneshot::Sender<..>>>`）を引く。タイムアウト・即時拒否時は呼び出し側がエントリを削除し、**コールバックが永遠に来なくても何も漏れない**。trampoline は必ず結果文字列を `kaseifu_fm_free_result` で解放し、エントリが残っていれば送信（受信側が消えていればそのまま破棄）。
  - 初版は `Box<Sender>` をリークしたポインタを渡していたが、`leaks --atExit` が「タイムアウト後にプロセス終了すると Box が漏れる」を検出（112 bytes）したため、レジストリ方式に変更した（修正後は **0 leaks**）。
- **タイムアウト**: `SwiftFmBridge::generate(request, timeout)` が `tokio::time::timeout` で待ち、超過時に `kaseifu_fm_cancel` を呼んで `Err(Timeout)` を返す（Rust の async ランタイムはブロックしない）。
- **二重解放防止**: 結果文字列は Rust ラッパ（`OwnedResult` + `Drop`）だけが 1 回解放する。生ポインタを外へ出さない。
- **panic / 例外の境界越え禁止**: Swift 側は全エラーを捕捉して JSON へ変換（`try` を境界の外へ出さない）。Rust 側の extern 関数と trampoline は panic しないコードだけを書く（`unwrap` / `expect` / 添字 panic を使わない）。
- **捨てた代替**: Swift の async を C ABI へ直接公開（原典 §3.3 で禁止）/ 同期ブロッキング generate（Rust の async ランタイムを塞ぐ）/ コールバックを複数回呼び得る設計 / destroy で即時解放（use-after-free リスク）/ `Box<Sender>` をリークしたポインタ渡し（タイムアウト＋終了で漏れる、と実測で判明）。

### D4. JSON 契約の version と未知フィールド / 不正値

- **version**: 全エンベロープに `schema_version: "fm-rename-v1"` を必須。Swift 側は version 不一致を即時拒否（コード 3）。Rust 側は応答の version 不一致・`request_id` 不一致・未知 `status` を `InvalidResult`（対象スキップ扱い、§12）にする。
- **未知フィールド**: 無視する（Rust は serde の既定、Swift は辞書から既知キーのみ読む）。**不正値**: 期待型でなければ「欠落」と同じ扱い（`title` が数値なら `None`。クラッシュも捏造もしない）。
- **意味情報の不足**: `semantic` の各フィールドは Optional のまま返し、**AI に捏造させない**（§6.5 の「提案を保留」の材料。空文字は `None` に正規化）。
- **スキーマファイル**: 既存 3 本と同じ流儀（draft 2020-12 / `$id` / `title` / `type` / `required`）で `schemas/fm-rename-v1.json` を追加し、request / success response / error response / availability の 4 形を `oneOf` の `required` で示す。JSON Schema バリデータは導入せず（新規依存禁止）、**テストで必須キーと実出力の一致を固定**する。
- **捨てた代替**: version なし（原典 §3.4 が管理を要求）/ `deny_unknown_fields`（前方互換を壊す）/ バリデータ crate の導入（新規依存）。

### D5. モデル経路以外の検証（Apple Intelligence 未有効環境）

- `available()` の実出力を**テストで固定**し、理由がそのまま出ることを示す（未有効機体では `unavailable: appleIntelligenceNotEnabled` / `modelNotReady` / `deviceNotEligible`、SDK なしビルドでは `foundation-models-sdk-missing` / `swift-toolchain-missing`）。
- `generate` 経路は「create -> リクエスト検証 -> Task 起動 -> availability 判定 -> コールバック -> Rust パース」まで実行される。未有効機体ではモデル呼び出しの 1 分岐だけがスキップされ、エラー JSON で明示される。
- **モデル非依存の原則（親からの更新指示を反映）**: 既定のテストスイートは**モデルを必要としない**。
  - 利用可能性チェック、即時拒否、タイムアウト/キャンセル、destroy 規則、リンク証跡はどこでも実行される。
  - 完了まで推論が要るテスト（受理->コールバック、並行呼び出し、応答エンベロープ）は、**モデルが available な機体では実行せず明示的に print して素通り**し、CI（スタブ / 未有効）では完全に実行される。
  - 実推論は **`--ignored` 付きテスト**（`inference_round_trip_reports_semantic_fields_and_latency`、`destroy_during_a_generation_suppresses_the_callback`）と **`examples/fm_bridge_smoke`** に分離し、手動で実行する。
- スタブ経路（CI 相当）は `KASEIFU_FM_FORCE_STUB=no-sdk`（および `no-toolchain`）で**同一テストスイート**を実行して検証する。
- メモリは `leaks --atExit` をブリッジテストとスモークに対して実行して実測する（結果は **0 leaks**）。

### D6. その他の決定

- **セッション再利用はしない**（Phase 2 は 1 リクエスト 1 セッション。§11.5 の「会話履歴が混ざらない」を構造で保証。再利用は Phase 9 で評価）。コンテキストもリクエストごとに create / destroy する（Phase 9 の最適化候補として記録）。
- **プロンプト組み立ては Swift 側**（Swift Minimal: Apple API との橋渡し。Rust はデータだけを渡し、入力境界は Rust 側でも強制する）。ファイル内容は読まない（Phase 4 以降。Phase 2 はメタデータのみ）。
- **README は開発者向けに最小限追記**（Swift ツールチェーンの有無でブリッジ / スタブが切り替わること、スモークコマンド）。CHANGELOG はユーザー可視の変更が無いため**触らない**。`tests/user-test-plan.csv` も CLI 面の変更が無いため**触らない**。
- **隠し CLI / 新サブコマンドは追加しない**（スモークは `examples/` の開発者用バイナリ。`cargo install` では配布されない）。

### D7. 公開 API の骨子（実装者向け・命名規則は既存に合わせる）

```rust
// src/semantic/mod.rs
pub mod bridge;    // trait FmBridge + SwiftFmBridge（本番）
pub mod contract;  // SCHEMA_VERSION / FmRequest / FmResponse / SemanticInfo / Availability / ErrorKind
pub mod fake;      // FakeFmBridge / FakeSemanticProvider（mock_jev.rs と同じ位置づけ）
pub mod ffi;       // extern "C" 宣言（raw） + 安全ラッパ（FmContext / available / trampoline）
pub mod provider;  // trait SemanticProvider + FoundationModelsProvider
#[cfg(kaseifu_fm_stub)]
mod ffi_stub;      // Swift ツールチェーン / SDK が無いビルド用の Rust スタブ
pub const BRIDGE_KIND: &str;  // "swift" | "stub"（テストとスモークの分岐に使う）
```

- C ABI 名は原典 §3.2 の仮称をそのまま確定（製品プレフィクス `kaseifu_fm_` は既存の `kaseifu` / `KASEIFU_*` 命名と整合）: `kaseifu_fm_available` / `kaseifu_fm_create` / `kaseifu_fm_generate` / `kaseifu_fm_cancel` / `kaseifu_fm_free_result` / `kaseifu_fm_destroy`。
- trait の形は `JevIntegration`（`src/classifier/jev.rs:84-90`）と同じ `Pin<Box<dyn Future<Output = ...> + Send + '_>>` 戻し。
- `FmBridge`（FFI 面: JSON 文字列 -> JSON 文字列）と `SemanticProvider`（ドメイン面: `NormalizedFileContext` -> `SemanticInfo`）の 2 層。fake は両層に用意する。

---

## 4. BDD シナリオ（振る舞いと具体例）

### B1. 利用可能性チェック（§4.2 / 受け入れ 15）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| B1-1 | 実機（Apple Intelligence 有効） | `available()` が `{"schema_version":"fm-rename-v1","status":"available"}` を返し、同じ経路が実推論まで到達する（実測） |
| B1-2 | 実機（未有効） | `reason` が `appleIntelligenceNotEnabled`（実測済み。テストの既知理由集合に含まれる） |
| B1-3 | モデル準備中 / 非対応機体 | `modelNotReady` / `deviceNotEligible`（区別して出る。SAFE12 の検証） |
| B1-4 | SDK なし / ツールチェーンなしのビルド | `reason` が `foundation-models-sdk-missing` / `swift-toolchain-missing`。ビルドは成功し、既存コマンドは動く（実測） |
| B1-5 | macOS 26 未満での実行 | weak link で起動し、`reason: "unsupported-os"`（起動時クラッシュなし） |
| B1-6 | 未知の status / reason | Rust 側は `InvalidResult` / `Unknown` として安全に扱う（panic しない） |

### B2. FFI 呼び出しとメモリ（§3.2 / §3.3、原典 §13.1 FFI01-08）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| B2-1 | Rust -> Swift 呼び出し | `available()` が JSON 文字列を返し、Rust が解放する（FFI01） |
| B2-2 | 正常受理 | 妥当な request で `generate` が `0` を返し、コールバックが**正確に 1 回** JSON を渡す（FFI02/03） |
| B2-3 | 即時拒否 | null コンテキスト(1) / 不正 JSON(2) / version 不一致(3) / `request_id` 欠落(2) で拒否され、**コールバックは 0 回**（FFI05） |
| B2-4 | 多重完了防止 | 完了後に `cancel` してもコールバックは増えない（FFI07） |
| B2-5 | 破棄中のコールバック禁止 | `destroy` 後にコールバックが発生せず、クラッシュもしない（FFI04） |
| B2-6 | メモリ解放 | `free_result` は所有権どおり 1 回だけ。タイムアウト後にプロセスが終了しても漏れない（レジストリ方式。`leaks --atExit` で **0 leaks** を実測） |
| B2-7 | タイムアウト | 応答が来ない場合 `Err(Timeout)` + `cancel`。遅延して届いた結果は trampoline が安全に破棄（FFI06） |
| B2-8 | 複数リクエスト | 逐次 2 件が `request_id` ごとに独立して完了する（FFI08）。並行呼び出しは受理か busy 拒否のいずれかで、受理数とコールバック数が一致する |

### B3. 意味情報の抽出（§3.4 / §5.1 / Phase 2）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| B3-1 | 入力変換 | `NormalizedFileContext`（filename / extension / size / dates / parent / signals / image メタデータ）から `FmRequest` を組み立てる。`target_type` は画像拡張子なら `image`、他は `document` |
| B3-2 | トークン制限 | 信号は 20 件・各 120 文字、filename 512 文字、path 1024 文字で**文字境界を壊さず**切り詰める |
| B3-3 | 成功応答 | `status: "success"` + `semantic{title,category,project}` -> `SemanticInfo`（空文字は `None`） |
| B3-4 | 必須情報不足 | `title` が無い応答 -> `SemanticInfo { title: None, ... }`（**捏造しない**。提案保留の材料） |
| B3-5 | 前方互換 | 未知フィールドを含む応答を無視して読める |
| B3-6 | 不一致応答 | `request_id` 不一致 / 未知 `status` / JSON 破損 -> `InvalidResult`（対象スキップ） |

### B4. エラー処理（§12）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| B4-1 | 利用不可 | `SemanticError::Unavailable { reason }`（明示的エラー。**別 LLM への暗黙フォールバックなし**） |
| B4-2 | タイムアウト | `Timeout`（`is_skippable() == true` = 対象スキップ） |
| B4-3 | 結果不正 | `InvalidResult`（`is_skippable() == true`） |
| B4-4 | モデル初期化失敗 | `ModelInitFailed`（中断扱い。`is_skippable() == false`） |
| B4-5 | キャンセル | `Cancelled` |
| B4-6 | FFI エラー | panic せず `Err` として Rust に伝わる（境界越えの例外 / panic なし） |

### B5. 既存機能の非破壊（受け入れ 8 / 9 / 17）

| # | シナリオ | 期待する振る舞い |
|---|---|---|
| B5-1 | 既存テスト | 450 passed が全緑のまま（**テストを減らさない**） |
| B5-2 | CLI 面 | `enum Commands` 不変。ヘルプ走査・補完整合・README 契約のテストが無変更で緑 |
| B5-3 | 依存 / 設定 / キャッシュ | 新規 crate 依存 0、新規設定ファイル 0、新規 DB / キャッシュ 0、外部プロセス 0 |
| B5-4 | 品質ゲート | rustfmt 0 / clippy 0 / 新規 CC>=16 なし / 既存最大 CC 15 不悪化 / OVERALL PASS |
| B5-5 | リンク証跡 | ブリッジ有効ビルドで `otool -L` が FoundationModels を **weak** と示す（テストで固定。スタブビルドでは該当なし） |

---

## 5. タスク一覧（実装順・直列厳守）

| # | タスク | 主なファイル | 期待する振る舞い | テスト |
|---|---|---|---|---|
| T1 | build.rs + Swift ブリッジ骨格（`available` のみ）+ リンク PoC（Phase 1 の核） | `build.rs`（新規）, `swift/kaseifu_fm_bridge.swift`（新規） | B1-1, B2-1, B5-5 | 実機スパイク + `otool` 目視 |
| T2 | C ABI 本体（create / generate / cancel / free_result / destroy、非同期・exactly-once・destroy 待機）+ Rust スタブ + raw 宣言と安全ラッパ | `swift/kaseifu_fm_bridge.swift`, `src/semantic/ffi.rs`（新規）, `src/semantic/ffi_stub.rs`（新規） | B2-2〜B2-8 | `tests/v090_fm_bridge_test.rs`（新規） |
| T3 | JSON 契約（serde 型・境界・エラー種別）+ スキーマファイル | `src/semantic/contract.rs`（新規）, `schemas/fm-rename-v1.json`（新規） | B3-2〜B3-6, D4 | ユニットテスト + 契約テスト |
| T4 | ブリッジ trait + 本番実装 + fake | `src/semantic/bridge.rs`（新規）, `src/semantic/fake.rs`（新規） | B2-7, B4-6 | ユニットテスト |
| T5 | Semantic Provider（trait + 本番 + 入力変換 + 利用可能性） | `src/semantic/provider.rs`（新規）, `src/semantic/mod.rs`（新規）, `src/lib.rs` | B3-1〜B3-6, B4-1〜B4-6 | `tests/v090_semantic_provider_test.rs`（新規） |
| T6 | 開発者用スモーク（1 コマンド） | `examples/fm_bridge_smoke.rs`（新規） | D5（有効化後の確認手段 / leaks 対象） | 実機実行 + `leaks --atExit` |
| T7 | README 最小追記 + 計画書 / issues + 品質ゲート | `README.md`, `documents/plans/dev/**` | B5-1〜B5-4 | `bash scripts/quality_check.sh` |

各タスク完了ごとに `cargo fmt` / `cargo clippy -D warnings` / 対象テストを実行してから次へ進む。

**両ビルド経路の警告 0（Plan Architect F2/F3）**: `cargo clippy --all-targets --all-features` は「ブリッジ有効ビルド」と「スタブビルド（`KASEIFU_FM_FORCE_STUB=no-sdk`）」の**両方**で警告 0 であること（CI はスタブ、ローカルはブリッジ。どちらかで警告が出るとゲートが落ちる）。`examples/` は `cargo test --all-targets` でビルド対象になるため、こちらも警告 0 を完了条件に含める。

**cfg の宣言（Plan Architect F4）**: build.rs は `cargo:rustc-check-cfg=cfg(kaseifu_fm_stub, values("no-toolchain", "no-sdk"))` を宣言する（`unexpected_cfgs` 警告の回避）。

---

## 6. 検証手段（実測で示す）

| # | コマンド | 期待 |
|---|---|---|
| V1 | `cargo test --all-targets --all-features` | 450 + 新規が全緑、0 failed |
| V2 | `cargo fmt --all -- --check` / `cargo clippy --all-targets --all-features -- -D warnings` | 違反 0 / 警告 0 |
| V3 | `bash scripts/quality_check.sh` | OVERALL PASS、新規 CC>=16 なし、baseline-gate PASS |
| V4 | `cargo build` / `cargo build --release` | 成功（`cargo install` は実行しない） |
| V5 | `cargo run --example fm_bridge_smoke` | 実機の利用可能性と、有効なら実推論の semantic フィールド + 所要時間が出る |
| V6 | `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --test v090_fm_bridge_test` | スタブ経路でも同一テストが緑（CI 相当の再現） |
| V7 | `leaks --atExit -- <bridge test binary>` / `... -- ./target/debug/examples/fm_bridge_smoke` | **0 leaks**（実測値を報告） |
| V8 | `otool -L <test binary>`（B5-5 テスト内で実行） | FoundationModels が `weak` |
| V9 | `git status` / `git diff` の最終確認 | 意図したファイルのみ（`.env` 非表示、秘密なし） |
| V10 | `KASEIFU_FM_FORCE_STUB=no-sdk cargo clippy --all-targets --all-features -- -D warnings` | スタブ経路でも警告 0（F2） |
| V11 | `cargo test --test v090_fm_bridge_test -- --ignored --nocapture` | 実推論の往復（title/category と所要時間）と destroy 抑制の実測が出る |

---

## 7. 受け入れ基準（原典 §15）との対応

| # | 基準 | 本ラウンド |
|---|---|---|
| 1 | Rust から Swift を FFI 経由で呼び出せる | **達成**（B2-1, V5） |
| 2 | FM による意味情報抽出が動作する | **外部ブロッカー**（§2.3。有効化後に V5 で確認） |
| 3-7 | リネーム / 命名ルール / 保存 | 対象外（Phase 3-8） |
| 8 | 新しい設定ファイルを追加しない | **達成**（新規 TOML / YAML / JSON 設定なし。`schemas/` は既存と同じ文書スキーマ） |
| 9 | 独自キャッシュを追加しない | **達成** |
| 10-14 | dry-run / 上書き禁止 / Journal / Undo / 親フォルダ整合 | 対象外（Phase 3-8。既存 Journal / Undo の設計は §2.1 で確認済み） |
| 15 | FM 利用不可時に安全に終了 | **達成**（B1, B4-1。理由つき明示、フォールバックなし） |
| 16 | FFI 境界でメモリリーク / 未定義動作なし | **達成**（B2-2〜B2-8, V7。所有権契約 + exactly-once + destroy 待機） |
| 17 | 既存 organize / watch を壊さない | **達成**（B5-1〜B5-4。CLI 面の変更ゼロ） |

---

## 8. リスクと対策

| リスク | 対策 |
|---|---|
| CI ランナー（Xcode < 26）でブリッジがコンパイルできない | D2 のスタブに自動縮退しビルドは成功。テストは既知の reason 集合を受理し、スタブでも緑 |
| Swift ツールチェーンの場所・互換ライブラリの探索パスが環境依存 | `xcrun --find swiftc` の位置から導出。見つからなければスタブ（ビルドは失敗させない） |
| `-Xfrontend -disable-autolink-framework` は内部フラグで将来変わり得る | リンク証跡テスト（B5-5）で weak link を固定。壊れた場合はリンク時に気付ける（強リンクだと weak 判定が落ちる） |
| モデル推論が未有効で Phase 1 の一部が未検証 | 外部ブロッカーとして報告。構造は同一経路で、有効化後 1 コマンド（V5）で確認。暗黙フォールバックは実装しない |
| 非同期の寿命管理に潜在バグ（二重完了 / 破棄後コールバック / UAF） | exactly-once と destroy 待機をコードで構造化し、テスト（B2-4, B2-5）+ `leaks`（V7）で固定。`unsafe` は ffi.rs に隔離 |
| 新規コードの CC>=16 | 変換 / パース / 待機を小さな関数に分割。`quality_check.sh` の baseline-gate で実測 |
| ビルド時間の増加（Swift コンパイル + 全テストバイナリへのリンク） | 単一ファイル・`-O` は release のみ。`rerun-if-changed` を Swift ソースに限定 |
| スコープ逸脱（rename / 命名ルールを先に作ってしまう） | CLI 面は不変。`enum Commands` に触れないことを計画の完了条件に固定 |
| 生成物の混入（Swift 中間ファイル等） | コンパイルは常に `$OUT_DIR`（`/target` 配下、`.gitignore` 済み）で行い、リポジトリ直下に何も残さない（V9 で確認） |

---

## 9. 検収記録

| 工程 | 成果物 | 判定 |
|---|---|---|
| Plan Architect | `issues/20261014-plan-architect-ai-rename-p0.md` | 承認（F1〜F8 を計画に反映済み） |
| Reviewer | `issues/20261014-reviewer-ai-rename-p0.md` | **APPROVE**（blocking 0。R1-R3 は実装中に発見・修正済み、R5 はユーザー了解事項） |
| QA | `issues/20261014-qa-ai-rename-p0.md` | **PASS**（quality_check.sh OVERALL PASS / 506 passed + ignored 3 / CC>=16 0 / 新規依存 0 / 隔離監査 OK） |

### 完了時の実測サマリ（2026-10-14、本 worktree）

- テスト: 既存 450 -> **506 passed / 0 failed（+ ignored 3）**。新規 59: lib ユニット 25 / ブリッジ統合 17（うち ignored 3）/ provider 17。
- `bash scripts/quality_check.sh`: **OVERALL: PASS**（rustfmt 0 / clippy 0 / cargo-test 506 / complexity 829 関数・CC>=16 = 0・最大 CC 15 / baseline-gate PASS / coverage 85.61% report-only）。
- `cargo build` / `cargo build --release`: 成功。`otool -L` が FoundationModels を **weak**、concurrency を OS パスで示し、minos は 12.0。
- 実推論（本ブリッジ経由）: title/category を返した（初回 10.7-11.1 秒 = モデルロード含む / ウォーム 1.6-4.7 秒）。
- メモリ: `leaks --atExit` = **0 leaks for 0 total leaked bytes**（ブリッジテスト 14 件 + 実推論スモーク）。
- スタブ経路: `KASEIFU_FM_FORCE_STUB=no-sdk` / `no-toolchain` で同一テストが全緑（506）、スモークは `unavailable: foundation-models-sdk-missing` / `swift-toolchain-missing` を明示。

### 実測エビデンス（追記式）

#### 実推論（Phase 1 の完了条件）

- `cargo test --test v090_fm_bridge_test -- --ignored --nocapture`（モデルロードを含む初回実行）:
  ```text
  semantic: title=Some("2026-Q4-budget-draft") category=Some("budget") project=None (11137 ms)
  destroy returned after 56 ms
  cancel returned after 0 ms
  test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out
  ```
  再実行（モデルがウォームな状態）:
  ```text
  semantic: title=Some("2026-Q4-budget-draft.pdf") category=Some("budget") project=None (2424 ms)
  destroy returned after 102 ms
  ```
- `cargo run --example fm_bridge_smoke`:
  ```text
  bridge: swift
  availability: available
  semantic: title=Some("2026-Q4-budget-draft") category=Some("budget") project=None (8455 ms)
  ```
  （再実行時は 1.6 秒。同じ入力でも title/category の粒度は変わり得る = モデルは提案を返すだけであり、命名規則の適用は Phase 3 以降の Rust 側）
- スタブビルドのスモーク（CI 相当）:
  ```text
  bridge: stub
  stub reason: foundation-models-sdk-missing (no Swift bridge was built)
  availability: unavailable: foundation-models-sdk-missing
  semantic error: Unavailable(foundation-models-sdk-missing) (0 ms; skippable: false)
  ```
  （`no-toolchain` では `swift-toolchain-missing`）

#### リンク証跡（リリースバイナリ）

```text
$ otool -L target/release/kaseifu | grep -iE "FoundationModels|swift_Concurrency"
  /System/Library/Frameworks/FoundationModels.framework/Versions/A/FoundationModels (... , weak)
  /usr/lib/swift/libswift_Concurrency.dylib (...)
$ otool -l target/release/kaseifu | sed -n '/LC_BUILD_VERSION/,+4p'
  platform 1
    minos 12.0
      sdk 26.5
```

#### 既存コマンドの非破壊

- `./target/release/kaseifu --help` / `doctor`（隔離 HOME + `KASEIFU_KEY_SOURCE=env`）が従来どおり動作。
- CLI 面の変更ゼロ（`src/main.rs` の `enum Commands` 不変。ヘルプ走査・補完・README 契約の既存テストが無変更で緑）。
- 実補完ディレクトリ（`/opt/homebrew/share/zsh/site-functions/_kaseifu`）の mtime は実行前のまま（副作用なし）。
