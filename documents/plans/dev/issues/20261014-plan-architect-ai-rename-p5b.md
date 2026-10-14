# plan-architect レビュー: AIリネーム P5b 計画書（`20261014k-ai-rename-p5b-vision.md`）

レビュー方式: 本環境に `/plan-architect` スキルが無いため手順をなぞって実施（コード照合・再利用確認・制約検査・差し戻し判定）。照合は `read_file` / `grep` / `terminal` の実測のみに基づく。

## 判定: 条件付き承認（A1〜A5 を計画書へ反映済み。反映後の再レビューは不要）

初版計画書は、知覚の層構成（新 C ABI + bridge 注入）・互換拡張・縮小レシピ・テスト分離までを抑えていたが、下記 5 点を本レビューで検出した。A1〜A3 は実装を壊す/実測を濁す指摘であり、計画書 §4・§6・§10 へ反映した。

## 確認済み項目

| 観点 | 結果 |
|---|---|
| 知覚 API の実測 | OK。`import Vision` + `VNClassifyImageRequest` + ImageIO thumbnail は typecheck 成功、実ラベル取得も成功（合成夕景で 0.078〜0.385 秒、確信度最大 0.106）。`import Vision` の object に `-framework Vision` の autolink が入ることを `otool -l` で実測。SDK に Vision 無しの場合は `canImport(Vision)` で明示エラーに縮退する設計は妥当 |
| FFI 追加の整合 | OK。`kaseifu_fm_available`（同期・文字列返却・`kaseifu_fm_free_result` で解放）と同じ規約を再利用するため、新規 unsafe は `ffi.rs` に閉じ、`ffi_stub.rs` に同一 ABI を足す既存パターンで縮退できる |
| 層構成 | OK。contract（型と bound）→ ffi（唯一の unsafe）→ bridge（`FmBridge::perceive`）→ provider（`SemanticProvider::perceive` と `build_request` の写像）に乗る。`image_facts`（provider.rs:235-267）と同じ写像パターンで、二重実装なし |
| 契約の互換性 | OK。`input.perception` は任意フィールドで既存キー削除なし。`contract.rs:9-12`（未知フィールド無視）と `tests/v093` の未知フィールドテストで前方互換が既に固定されている。`schema_version` 据え置きは正当 |
| 非破壊・縮退 | OK。知覚失敗を `Unavailable(reason)` の縮退にして提案を継続する設計は原典 §12（デコード失敗スキップ・読み取り失敗スキップ）と整合。stub ビルド（`KASEIFU_FM_FORCE_STUB`）で Phase 5 と同じ提案に戻ることも計画の検証手段に含まれている |
| テスト隔離 | OK。Fake 注入＋一時 dir＋隔離 HOME。実ラベルは `#[ignore]` に分離し、assert は件数・順序・長さに限定。合成画像はテスト内 PNG ライタで決定的に生成（新規 crate なし） |
| 既存テストへの影響 | OK。`NormalizedFileContext` の完全リテラルは `src/normalize/mod.rs:117` と `src/discovery/search_terms.rs:117,144` の 3 箇所のみ（他は `..Default::default()`）。`SemanticInfo` を変更しない選択（知覚は `NormalizedFileContext` 側に載せる）により v090/v092/v093 の構造体リテラルは壊れない |
| 複雑度 | 条件付き。`plan_image_rename` への挿入は小関数（知覚呼び出し・ラベル丸写し判定）に分割し、新規 CC>=16 を作らないこと（R1） |
| OCR 送り | OK。実測（0.397 秒で `OPEN` / `9:00 - 18:00`）を示した上で、原典の「必要時」・注入面の拡大・優先順位を根拠に次回送りとしている |

## 指摘（計画書へ反映済み）

- A1 **プロンプト行の位置**: `kfBuildPrompt`（`swift/kaseifu_fm_bridge.swift:182-231`）は最後に `kfTrim(..., kfMaxPromptChars=2000)` で切り詰める。`signals:` 行は最大 20x120 文字になり得るため、`visual_labels` 行を signals の後ろに置くと切り捨てられ得る（知覚が届かない退行）。→ 信号行より前に置くこと。計画 §6-T9 に明記し、§5-S1 のテストもその位置を固定する。
- A2 **非画像パスの防御**: `SemanticProvider::perceive` は service の画像分岐からのみ呼ばれるが、provider 側で `is_image` を確認し、非画像は `Failed("not an image")` を返して ImageIO に渡さない（PDF 等を誤って分類しない）。→ §6-T8 に明記。
- A3 **モデル利用不可時の無駄知覚**: 知覚は `analyze` より前に必要なため（ラベルを request に載せる）、モデルが unavailable の環境でも知覚 0.1〜0.4 秒を使う。既存の `availability()` 事前確認は行わず、この順序と限界を §10（リスク）に明記する（振る舞いは analyze の失敗と同じ `ModelUnavailable`）。→ §10 に追記した。
- A4 **ラベル丸写し判定の正規化**: 比較は title と各ラベルを「trim・小文字化・`_`/空白連続を 1 つの空白」に正規化して等値で行う（部分一致・同義は見ない。誤検出を避ける）。→ §5-W5 に明記した。
- A5 **表示の上限**: 知覚ラベルは最大 12 件なので、text 出力は最大 6 件＋`(+N more)` に丸めて 1 行を bounded に保つ（JSON は全件）。→ §6-T11 に明記した。

## 残る注意（実装・レビュー工程への引き継ぎ）

- R1（reviewer へ）: `service.rs` の追加は小関数に分割し、新規 CC>=16 を作らないこと。実装後 `scripts/quality_check.sh` の complexity 表で確認する。
- R2（reviewer へ）: 文書プロンプトのバイト不変を `git diff` と既存テスト（画像 gate 数 2）で確認すること。
- Q1（QA へ）: 実機検証（旧バイナリ `b9b7e68` との同一入力比較・dry-run/execute/undo・所要時間・実ラベル・`--format json`）を §9 に実測記録すること。
- Q2（QA へ）: `KASEIFU_FM_FORCE_STUB=no-sdk` のスイートで縮退（知覚 unavailable でも提案が成立）を確認すること。
