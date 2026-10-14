# QA: AIリネーム P5（完成形の品質チェック）

実施方式: `/qa` スキル不在のため手順をなぞって実施（コード品質・パス検証・構造的一貫性・テスト実行）。
実測のみに基づく。commit / push / tag はしない。

## 判定: 承認（問題なし。検収可能）

## 1. 実行した検証コマンドと実際の出力（2026-10-14）

- `cargo test --all-targets --all-features`: **663 passed / 0 failed**
  （ベースライン 639＋回収分 21＋人物方針 3。`v093_ai_rename_p5_test` は 21/21）。
- `cargo fmt --all -- --check`: 違反 0。`cargo clippy --all-targets --all-features -- -D warnings`: 警告 0。
- `bash scripts/quality_check.sh`: **OVERALL: PASS**。
  complexity 表: Functions 1164、CC<=10 が 1153（99.1% PASS）、CC 11-15 が 11、**CC>=16 が 0**、
  最大 CC 15（更新前と同じ）、baseline-gate PASS（新規・悪化の CC>=16 なし）。
  rustfmt PASS、clippy PASS、cargo-test PASS（passed 663, failed 0）、report-tests PASS、coverage 82.45%（report-only）。
- `cargo check --all-targets --all-features`: 成功（約16秒）。Swift ブリッジ再コンパイル＋リンク成功。
- SDK 再検証: `Attachment(imageURL:)` プローブは `cannot find 'Attachment' in scope` で失敗（D1 維持）。
  swiftinterface の `Attachment` 5 件は全て `logFeedbackAttachment`（画像入力 API なし）。
  ImageIO thumbnail レシピ（2048）は typecheck 成功（D2 維持）。
- 実機（隔離 HOME・一時 dir・合成のみ）: PNG P02 dry-run 15.3 秒→`2026-10-14_shot.png`。
  EXIF JPEG P02 dry-run 10.9 秒→`2026-10-14_IMG_1234.jpg`。P11 dry-run 11.4 秒→
  `2026-10-14_TestMake_TestModel_IMG_1234.jpg`。PNG `--execute`→`executed`（journal `move` success）→
  `undo --last 1` で復帰→二重 undo は `nothing left to undo` で停止。人物示唆名 probe 5.3 秒で捏造なし。
  動画は Phase 6 エラー継続、文書は従来提案、`--format json` キー不変。全件 20 秒以内。

## 2. 構造的一貫性

- 配置: `src/rename/image.rs`（`document.rs` と同層）・`tests/v093_ai_rename_p5_test.rs`
  （`v090`・`v091`・`v092` 使用済みの次番）。版数衝突なし。
- 差分範囲: 変更 6 ファイル＋新規 4 ファイル（計画書・issues 2 件を含む）。`Cargo.toml`／`Cargo.lock` 無変更。
  無関係変更・秘密情報・デバッグ残骸なし（`git status`・差分を目視）。
- テスト隔離: 実ユーザーの写真・`~/Pictures/`・`~/Desktop/`・実 Keychain・実補完 dir の読み書きなし。
  実ファイルの移動・削除・改名は一時 dir 内の合成ファイルのみ。手動検証の一時 dir は掃除済み。

## 3. 受け入れ基準（原典 §15 の本ラウンド分）

- 3（一部）画像のリネームができる: dry-run／`--execute`／Journal／Undo の一巡を Fake と実機の両方で確認。
- 4/5/6 命名ルール・日付・テンプレート: 画像でも同一エンジン（P01/P02/P11 を実測）。
- 10/11/12/13 dry-run 既定・上書き禁止・Journal・Undo: 計画どおり（衝突・No-op・symlink はテストで固定）。
- 17 既存機能を壊さない: 663/0＋OVERALL PASS＋文書・動画の実機挙動で確認。
- 1/14/15/16 FFI・履歴整合・利用不可時終了・FFI 安全性: 既存実績＋I9 で維持。

## 4. 検証できなかったことと残るリスク

- 真の視覚理解に対する人物方針の効き目は未検証（画像バイトを送らないため。§11.4 に次回条件を記録）。
- live の `{date}` は Spotlight capture 優先であり、EXIF 日時の勝ちは単体・Fake テストで固定する。
- Unicode NFC の完全比較はできない（継承した残リスク。バイト一致＋衝突検査で対応）。
- commit / push / tag はしていない（親スレッドが実施）。
