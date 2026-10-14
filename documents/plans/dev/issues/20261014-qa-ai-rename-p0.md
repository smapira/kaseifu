# QA 検証: Apple Foundation Models 統合（AI リネーム基盤）Phase 0-2

- 日付: 2026-10-14
- 対象: `documents/plans/dev/20261014f-ai-rename-p0.md` の実装（未コミット差分 + 新規ソース / Swift / テスト / スキーマ / ドキュメント）
- 判定: **PASS**
- 実施コマンド: `bash scripts/quality_check.sh` / `cargo test --all-targets --all-features`（ブリッジ・スタブ両モード）/ `cargo fmt --all -- --check` / `cargo clippy --all-targets --all-features -- -D warnings`（両モード）/ `cargo build` / `cargo build --release` / `cargo run --example fm_bridge_smoke` / `leaks --atExit` / `otool -L`・`otool -l` / `cargo test -- --ignored`（実推論）

## 品質ゲート（最終実行、2026-10-14）

| 項目 | 結果 |
|---|---|
| rustfmt | PASS（violations 0） |
| clippy `-D warnings` | PASS（warnings 0。ブリッジ・スタブ両モード） |
| cargo test | **PASS: 506 passed / 0 failed（+ ignored 3）**（ベース 450 + 新規 59。減ったテストなし） |
| complexity | PASS（analyzer 0.0.25、829 関数） |
| baseline-gate | PASS（no new/worsened CC>=16） |
| report-tests | PASS |
| coverage | PASS（report-only、line **85.61%**。ベース 85.26% から微増） |
| **OVERALL** | **PASS** |

- CC 分布: CC<=10 = 825 / 829（**99.5%**）、CC 11-15 = 4（既存のみ）、CC>=16 = **0**、最大 CC **15**（悪化なし）。新規の `semantic` 系は小さな関数に分割されている。

## 新規テストの内訳

- `src/semantic/**` ユニット 25（contract 8 / ffi 9 / bridge 1 / provider 7）
- `tests/v090_fm_bridge_test.rs` = **14 passed + 3 ignored**（利用可能性契約 / 即時拒否 / 受理と exactly-once / タイムアウトと遅延結果破棄 / 並行 / destroy / スキーマ整合 / 弱リンク証跡 / 実推論（ignored）/ destroy 待機（ignored）/ busy（ignored）。計 17）
- `tests/v090_semantic_provider_test.rs` = **17 passed**（fake 注入のみ。モデル非依存）
- 既存テストの意図的変更: **なし**（450 件は無変更で緑。`v052_help_still_lists_every_command` などの列挙リストも不変）

## パス・構造チェック

- 新規ファイル配置: `build.rs`（root 慣例）、`swift/kaseifu_fm_bridge.swift`（非 Rust ソースの置き場）、`src/semantic/{mod,contract,ffi,ffi_stub,bridge,provider,fake}.rs`（既存モジュールと同層）、`examples/fm_bridge_smoke.rs`（開発者用）、`schemas/fm-rename-v1.json`（既存 3 本と同流儀）。
- ドキュメント: 計画書 1 + issues 3（Plan Architect / Reviewer / QA）。`documents/plans/user/**` は**無変更**（読み取りのみ）。
- `.gitignore`: 生成物は `$OUT_DIR`（`/target` 配下、既存の管理外）のみ。リポジトリ直下に Swift 中間ファイルなし（実測確認）。`reports/quality/` は既存どおり管理外。
- 未追跡のまま残したもの（親がコミット対象を選ぶ）: `build.rs` / `swift/` / `src/semantic/` / `schemas/fm-rename-v1.json` / `tests/v090_*.rs` / `examples/` / 計画書 + issues。`git add` は未実施。
- デバッグ残骸: 新規コードに `TODO` / `FIXME` / `dbg!` / `println!`（library 内）なし。em dash なし（新規テキストは ASCII）。

## 隔離・セキュリティ監査

- 新規テストは**ファイルシステムに触れない**（パスを一切開かない。合成 `NormalizedFileContext` と JSON 契約のみ）。`leaks` / スモークも同様。
- 実ユーザー領域への副作用なし: 実行した CLI は `--help` と `doctor`（隔離 HOME + `KASEIFU_KEY_SOURCE=env` + `KASEIFU_NO_COMPLETION_UPDATE=1`）のみ。実補完ディレクトリ `_kaseifu` の mtime は実行前のまま（04:59）。`cargo install` は未実行。
- 秘密情報: 新規コード・テスト・JSON・計画書に API キー・トークン・認証情報なし（`grep` で確認）。`.env` は読んでいない。
- ネットワーク: 新規テスト・スモークはネットワーク不要（Foundation Models はオンデバイス）。

## 互換性監査

| 項目 | 結果 |
|---|---|
| `enum Commands` / CLI 面 | 不変（`src/main.rs` 無変更。ヘルプ走査・補完・README 契約の既存テストが緑） |
| `discover` 非表示 | 不変（隠しコマンド追加なし。スモークは `examples/` で `cargo install` 対象外） |
| `--format json` | 既存キーの削除なし（CLI 出力に変更なし） |
| `KASEIFU_KEY_SOURCE` | 不変（`src/config` 未変更） |
| ドライラン既定 | 不変（rename は未実装。新規コマンドなし） |
| ビルド要件 | **変更あり（意図的）**: Swift ツールチェーンがあるとブリッジをビルド（なければ Rust スタブでビルド成功）。バイナリの最小 macOS が 12.0（README に記載） |
| 実推論 | 本ブリッジ経由で成功（title/category を取得。初回 10.7-11.1 秒、ウォーム 1.6-4.7 秒） |

## 実測エビデンス（抜粋）

```text
$ cargo run --example fm_bridge_smoke
bridge: swift
availability: available
semantic: title=Some("2026-Q4-budget-draft.pdf") category=Some("document") project=None (4721 ms)

$ KASEIFU_FM_FORCE_STUB=no-sdk cargo run --example fm_bridge_smoke
bridge: stub
stub reason: foundation-models-sdk-missing (no Swift bridge was built)
availability: unavailable: foundation-models-sdk-missing
semantic error: Unavailable(foundation-models-sdk-missing) (0 ms; skippable: false)

$ leaks --atExit -- <bridge test binary> --test-threads=1
Process 6377: 0 leaks for 0 total leaked bytes.

$ otool -L target/release/kaseifu | grep -iE "FoundationModels|swift_Concurrency"
  /System/Library/Frameworks/FoundationModels.framework/... (..., weak)
  /usr/lib/swift/libswift_Concurrency.dylib (...)
$ otool -l target/release/kaseifu | sed -n '/LC_BUILD_VERSION/,+4p'
    minos 12.0
```

## 残存事項（非阻害）

- 実推論は `--ignored` テストとスモークに分離（既定スイートはモデル非依存。CI はスタブで全緑）。
- スタブは Swift と同じリクエスト検証を Rust で再実装（Reviewer R4。同一テストを両実装で走らせてドリフト検出）。
- バイナリの最小 macOS 12.0（Reviewer R5。ユーザー了解事項）。
- `destroy` の待機上限 5 秒（実測は 56-120ms。Reviewer R6）。
- セッション / コンテキストの再利用は Phase 9 に繰り延べ（Phase 2 は 1 リクエスト 1 セッション）。
- Phase 3 以降（rename / 命名ルール / Journal / Undo）は未着手（計画どおり）。
