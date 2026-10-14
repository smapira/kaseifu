# Reviewer 検収: Apple Foundation Models 統合（AI リネーム基盤）Phase 0-2

- 日付: 2026-10-14
- 対象: `documents/plans/dev/20261014f-ai-rename-p0.md` の実装（未コミット差分 + 新規ソース / Swift / テスト / スキーマ / ドキュメント）
- 判定: **APPROVE**（blocking 0。実装中に発見・修正した 3 件の重大問題は修正済みで、証跡付きで記録する）
- レビュー方法: 実コードの読解（`build.rs` / `swift/kaseifu_fm_bridge.swift` / `src/semantic/**` / 新規テスト）と、実バイナリ・実機での再現実験（`cargo test` 両ビルド / `otool` / `leaks` / `--ignored` 実推論 / スタブスモーク）

## 1. 受け入れ条件とのトレーサビリティ

| 原典の要求 | 実装箇所 | 検証 |
|---|---|---|
| §3.2 の 6 関数 | `swift/kaseifu_fm_bridge.swift`（`@_cdecl` 6 つ）、宣言は `src/semantic/ffi.rs` の `raw` | `tests/v090_fm_bridge_test.rs` の raw ABI テスト |
| §3.3 非同期・キャンセル・タイムアウト・所有権 | Swift: `KaseifuFmContext`（lock + `finish` 1 本化 + `destroy` 待機）。Rust: `FmContext::generate` + `await_with_timeout` + delivery レジストリ | exactly-once / busy / destroy 後コールバックなし / タイムアウトで cancel / 遅延結果破棄（テスト + `leaks` 0） |
| §3.4 JSON 契約 | `src/semantic/contract.rs`、`schemas/fm-rename-v1.json` | 契約テスト（未知フィールド・不正型・未知 status・version 不一致） |
| §4.2 利用条件 | `kfUnavailableReason` + `parse_availability` | 理由を区別して出し、テストの既知理由集合で固定 |
| §4.2 / §12 / 受け入れ 15: 暗黙フォールバック禁止 | どこにも代替モデル経路がない（`grep` で Jev への参照なし） | `unavailable` は `SemanticError::Unavailable` のみ |
| §5.1 / Phase 2: FileContext -> 入力変換 | `provider::build_request`（`NormalizedFileContext` から） | 境界値テスト（signals 20 件・filename 512 文字・文字境界安全） |
| §12 エラー分類 | `SemanticError` + `is_skippable` | 全 kind のマッピングをテスト |
| 受け入れ 8 / 9（新規設定・キャッシュなし） | 新規 TOML/YAML/DB/キャッシュなし。`schemas/` は既存と同じ文書スキーマ | `git status` / `grep` |
| 受け入れ 16（リーク・UB なし） | 所有権を `ffi.rs` に隔離、`unsafe` はここだけ | `leaks --atExit` = **0 leaks**、`leaks` が検出した初版の漏れは修正済み |
| 受け入れ 17（既存を壊さない） | `enum Commands` 不変、CLI 面変更ゼロ | 既存テスト 450 が無変更で緑、`--help` / `doctor` の実機確認 |

## 2. 敵対的検証（実際に試したこと）

| 試したこと | 結果 |
|---|---|
| 不正 JSON / 空 request_id / 未知 version / null コンテキスト | 即時拒否（2 / 2 / 3 / 1）でコールバック 0 回 |
| 受理後に cancel | コールバックは 1 回のまま（二重完了なし） |
| 生成中に destroy | destroy 後 200ms 観測してもコールバックなし（実測 56-120ms で復帰） |
| タイムアウト（ZERO）後に遅延結果が到着 | クラッシュなし。delivery レジストリが回収（`leaks` 0） |
| 同一コンテキストへの並行 generate | 受理と busy(4) のいずれかで、受理数 = コールバック数（実機で busy を実測） |
| 応答の request_id 不一致 / 未知 status / 未知 error kind | `InvalidResult`（対象スキップ）で安全 |
| 応答の型違い（title が数値） | 欠落として扱い、**捏造しない** |
| スタブビルド（`no-sdk` / `no-toolchain`） | 同一テストが全緑、理由をそのまま明示 |
| 実推論（Apple Intelligence 有効） | title/category を返した（初回 10.7-11.1 秒、ウォーム 1.6-2.4 秒） |

## 3. 指摘事項

### R1 🟡（実装中に発見・修正）swiftc の直接呼び出しで全ビルドが黙ってスタブ化する

**何が問題か**: 最初の `build.rs` は `xcrun --find swiftc` で得たパスを直接実行していた。この起動方法では Swift が標準ライブラリを見つけられず（`unable to load standard library for target ...`）プローブが失敗し、**ブリッジが一度もビルドされないまま「SDK なし」スタブに縮退**していた（ローカルテストはすべて緑のままなので気づきにくい）。
**証拠**: `$SWIFTC -typecheck probe.swift` = 失敗、`xcrun swiftc -typecheck probe.swift` = 成功（同じファイル・同じ SDK）。
**対応**: コンパイラは常に `xcrun swiftc` 経由で起動する（xcrun が無い場合のみ直接呼び出しにフォールバックし警告）。修正後は `BRIDGE_KIND == "swift"` で実推論まで到達することを実測。

### R2 🟡（実装中に発見・修正）deployment target 11.0 で dyld が起動時に abort する

**何が問題か**: リンクの deployment target が 11.0 だと、Swift concurrency ランタイムが `@rpath/libswift_Concurrency.dylib` 参照になり、Xcode の無いマシンでは **Task を使った瞬間にプロセスが abort** する（`Library not loaded ... no LC_RPATH's found`）。これは受け入れ 15（安全に終了）を大きく損なう。
**証拠**: 修正前のバイナリを実行して再現（dyld の abort 出力）。`otool -L` が `@rpath/...` を示す。
**対応**: Swift オブジェクトを `-target *-apple-macosx12.0`、Rust のリンクに `-mmacosx-version-min=12.0` を渡し、ロードを OS パス（`/usr/lib/swift/libswiftConcurrency.dylib`）に変えた。**副作用としてバイナリの最小 macOS が 11.0 -> 12.0 に上がる**（R5 として記録）。

### R3 🟡（実装中に発見・修正）タイムアウト後にプロセスが終了すると 112 bytes 漏れる

**何が問題か**: 初版は `Box<oneshot::Sender>` をリークしたポインタを `user_data` に渡し、コールバックが回収する設計だった。タイムアウトして受信側が消えた後、コールバックが届く前にプロセスが終了すると Box が漏れる。
**証拠**: `leaks --atExit -- <bridge test binary>` が `2 leaks for 112 total leaked bytes`（スタックは `Box::into_raw` を指した）。
**対応**: プロセス全体の delivery レジストリ（`Mutex<HashMap<u64, Sender>>`）に変更。タイムアウト・即時拒否時に呼び出し側がエントリを削除するため、**コールバックが永遠に来なくても漏れない**。修正後はブリッジテスト・実推論スモークとも `0 leaks for 0 total leaked bytes`。

### R4 🟢 スタブは Swift と同じリクエスト検証を Rust で再実装している

**内容**: `ffi_stub.rs` は JSON 検証（オブジェクト / version / request_id）を Swift と同順で再実装する。ドリフトすると「CI では通るがローカルで落ちる」があり得る。
**判断**: 意図的なトレードオフ。スタブを Swift で二重化するより、**同一テストを両実装で走らせる**方がドリフト検出に強い（ローカル = Swift / `KASEIFU_FM_FORCE_STUB` = スタブで、同じテストファイルが緑）。非阻害。

### R5 🟢 バイナリの最小 macOS が 12.0 になる

**内容**: R2 の対策による意図的な変更。macOS 11（Big Sur、2023 に EOL）では起動しなくなる。
**判断**: 代替案（11.0 を保ち `-rpath /usr/lib/swift` を足し、macOS < 12 では Task を使わない分岐にする）は、macOS 11 上で concurrency シンボルに触れた時の挙動を**この環境で検証できない**（未検証のクラッシュリスクを残す）。検証可能な確実性を優先し、README と計画書に明示した。非阻害（要ユーザー了解事項として報告）。

### R6 🟢 destroy の待機は最悪 5 秒ブロックしうる

**内容**: `destroy` は「破棄後のコールバック禁止」のため完了を待つ（上限 5 秒）。実測ではキャンセルが即時（56-120ms）のため実害はないが、モデルがキャンセルに応じない場合はその上限までブロックする。
**判断**: 契約（use-after-free を作らない）を優先。実測値を計画書に記載。非阻害。

### R7 🟢 プロンプトの切り詰めは末尾（データ）から切れる

**内容**: `kfMaxPromptChars = 2000` の切り詰めはプロンプト全体に適用される。
**判断**: 指示行（データを命令として扱わない旨を含む）を先頭に置いているため、切れるのはデータ行のみ。設計意図として計画書に記載済み。非阻害。

## 4. 仕様適合の確認（抜粋）

- 原典 §3.1 の禁止事項（外部プロセス / JSON Lines / XPC）は使っていない（同一プロセス内の C ABI のみ）。
- Swift 側にファイル操作・SQLite・Journal はない（`grep` で確認。Foundation / FoundationModels のみ）。
- CLI 面は不変（`src/main.rs` 未変更）。隠しコマンドも追加していない（スモークは `examples/`）。
- 秘密情報なし（`grep` で API キー・トークン参照なし）。

## 5. 判定

**APPROVE**。blocking 0。R1-R3 は実装中に発見し修正済み（各々実測証跡あり）。R5 はユーザー了解事項として報告する。
