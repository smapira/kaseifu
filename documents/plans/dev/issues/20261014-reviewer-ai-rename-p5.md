# reviewer 検収: AIリネーム P5（`20261014j-ai-rename-p5.md` §11 を含む完成形）

レビュー方式: `/reviewer` スキル不在のため手順をなぞって実施（承認済み計画との照合・差分の実読・制約検査・BDD 網羅確認）。
実測のみに基づく。commit / push / tag はしない。

## 判定: 承認（指摘なし。QA へ進める）

## 1. 照合対象

- 計画: `documents/plans/dev/20261014j-ai-rename-p5.md`（§1〜§10＋§11 引き継ぎ更新）
- 差分: `src/rename/image.rs`（新規）、`src/rename/rules.rs`、`src/rename/service.rs`、
  `src/main.rs`、`src/rename/mod.rs`、`swift/kaseifu_fm_bridge.swift`、
  `tests/v093_ai_rename_p5_test.rs`（新規）、`tests/v092_ai_rename_p34_test.rs`
- ベースライン: `cargo test` 663 passed / 0 failed、`quality_check.sh` OVERALL PASS（いずれも本レビュー前に再実測）。

## 2. 差分の実読結果

| 箇所 | 結果 |
|---|---|
| `swift/kaseifu_fm_bridge.swift`（人物方針） | OK。追加は `target_type == "image"` ゲート内の 3 行＋Instructions 画像分岐のみ。文書プロンプトの基底文字列に差分なし（`git diff` で確認）。禁止属性（氏名・性別・年齢・人種・身体・服装・髪型・続柄・役割・人数）、非人物要素の命名、人物のみ時の保留、断定禁止が文言に入る。全て ASCII、em dash なし |
| `src/rename/image.rs` | OK。`is_image_file` 再利用で拡張子リストの二重持ちなし。デコードなし。単体テスト 3 件 |
| `src/rename/rules.rs` | OK。`parse_datetime`（RFC 3339＋zone 無し 2 形、zone 推測なし）＋`resolve_image_date`（EXIF 先頭、文書内日付不使用）。文書の `resolve_date` の順序不変 |
| `src/rename/service.rs` | OK。`plan_image_rename`（本文読みなし・ファイル名 injection 検査のみ）と `plan_document_rename` に分割。`finish_plan`・`execute_plan`・journal・undo は共用で無変更。`T:` 信号は文書経路のみ |
| `src/main.rs` | OK。画像を先に通し、文書判定の既存メッセージ・文面を変更しない。`document.rs` の Phase 5 向け文面は到達不能のまま API として残る |
| `tests/v093`（21 件） | OK。I0〜I9＋契約固定（v1・未知無視）＋人物方針 3 件（プロンプト文言・facts キー・`T:` ゼロ）。全て Fake・一時 dir・HOME 隔離。A6 の相関方式で flake なし |
| `tests/v092` 更新 | OK。画像→Phase 5・動画→Phase 6 の fail-fast 更新のみ。既存の保護内容は維持 |

## 3. BDD 網羅（I0〜I11・N1〜N4）

- I0 分類・I1 ファイル名 injection・I2 dry-run・I3 execute/undo・I4 EXIF 日時・I5 P11 device／保留・
  I6 必須不足・未知変数・I7 検証一式・I8 Unreadable／破損継続・I9 利用不可: `v093`＋単体テストで網羅。
- I10 プロンプト文言: 方針テストで 5 文言＋画像ゲート 2 箇所を固定。I11 実機目視: §11.4 の記録（合成素材の限界注記あり）。
- N1 663/0・N2 fmt/clippy/CC・N3 ASCII・JSON キー不変・discover 隠蔽（差分なし）・N4 隔離（実写真・実 Keychain・実補完 dir 不使用）: 全て確認。

## 4. 制約検査

- 新規 crate なし（`Cargo.toml`／`Cargo.lock` 無変更）。新規設定・DB・キャッシュ・外部プロセスなし。
- 既存コマンド挙動不変（organize 等の差分なし。rename 文書系は実機で再確認）。
- 新規 CC>=16 なし・最大 CC 15 維持（complexity 表で確認）。
- 命名規則維持（dry-run 既定・捏造なし保留・拡張子不変・80 字・上書き禁止・Journal 先行）。
- 秘密情報の混入なし（差分を目視。`.env` 非表示）。

## 5. plan-architect 記録との整合（差し戻し不要の確認）

- A1〜A7 は全て実装に反映済み（§11.5 の対応表どおり）。人物記述方針は A5 の内側の文言拡張であり、
  契約・順序・隔離・CC・OCR 送りの判断を変更しない。矛盾なしのため差し戻し不要と判断する。
- 残る注意 R1（小関数分割・CC）は遵守、Q1（実機記録）は §11.4 で充足する。

## 6. 残リスクの承認（QA・報告への引き継ぎ）

- 真の視覚理解に対する方針の効き目は未検証（画像バイトを送らないため）。Attachment 配線後の次回に持越し（§11.4）。
- live の `{date}` は Spotlight capture 優先であり、EXIF 日時の勝ちは単体・Fake テストで固定する（A6 設計）。
- `date_source: exif` のラベルは Spotlight capture を含む従来語彙であり、今回変えない。
