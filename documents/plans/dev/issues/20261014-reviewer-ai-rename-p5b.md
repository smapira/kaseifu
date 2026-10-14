# reviewer 検収: AIリネーム P5b（Vision 知覚層。`20261014k-ai-rename-p5b-vision.md` §9 を含む完成形）

レビュー方式: `/reviewer` スキル不在のため手順をなぞって実施（承認済み計画との照合・差分の実読・制約検査・BDD 網羅確認）。実測のみに基づく。commit / push / tag はしない。

## 判定: 承認（指摘なし。QA へ進める）

## 1. 照合対象

- 計画: `documents/plans/dev/20261014k-ai-rename-p5b-vision.md`（§0〜§10＋§9 実測記録）と plan-architect 記録 `issues/20261014-plan-architect-ai-rename-p5b.md`（A1〜A5 反映済み）。
- 差分: `src/semantic/perception.rs`（新規）、`src/semantic/{mod,contract,bridge,ffi,ffi_stub,fake,provider}.rs`、`src/models/file_context.rs`、`src/normalize/mod.rs`、`src/discovery/search_terms.rs`、`src/rename/service.rs`、`src/main.rs`、`swift/kaseifu_fm_bridge.swift`、`schemas/fm-rename-v1.json`、`tests/v094_ai_rename_p5b_vision_test.rs`（新規）、`tests/v093_ai_rename_p5_test.rs`（FmInput のフィールド 1 つ追加）。
- ベースライン: 664 passed / 0 failed（HEAD `b9b7e68`）。実装後 686 / 0（通常ビルドと `KASEIFU_FM_FORCE_STUB=no-sdk` の両方）。`quality_check.sh` OVERALL PASS。

## 2. 差分の実読結果

| 箇所 | 結果 |
|---|---|
| `swift/kaseifu_fm_bridge.swift` | OK。`kaseifu_vision_perceive` は同期・文字列返却で、ImageIO の read-only open（`kCGImageSourceShouldCache: false`）→ 長辺 2048 の thumbnail（アスペクト比保持・transform 有効）→ `VNClassifyImageRequest` → 降順・上限 12・床 0.01・1 ラベル 64 文字。`canImport(Vision)` gate 外は `vision-framework-missing` の明示エラー。書込み API（`CGImageDestination` / `writeToFile` / `FileManager`）は無し。ヘッダの「ファイルシステムに触れない」記述は「書かない／渡された画像を read-only で読む」に更新済み |
| プロンプト（同ファイル） | OK。追加は `input.perception.labels` が存在し空でない場合のみ発火する 3 行で、位置は `signals:` 行より前（A1 の切詰め対策）。文書経路ではデータが無いためバイト不変。既存の画像 gate 文字列は 2 箇所のままで、v093 の gate 数テストがそのまま通る |
| `src/semantic/perception.rs` | OK。envelope は availability と同じ規約（版不一致・未知 status・壊れた JSON・error に kind 無しはすべて明示エラー）。ラベル欠落は除外し捏造しない。bound は関数分割され CC は小さい |
| `src/semantic/contract.rs` | OK。`FmPerception` / `FmPerceptionLabel` は任意フィールド `input.perception` として追加。`FmImageFacts` のキー集合は不変（camera/capture_time/height/width。テストで固定）。wire 直前で再度 bound |
| `src/semantic/ffi.rs` / `ffi_stub.rs` | OK。新規 unsafe は raw 宣言とラッパ 1 関数のみ（NUL・非 UTF-8 を明示エラーに）。stub 側は同じ ABI を `unavailable`＋REASON で実装し、CI 経路が壊れない |
| `src/semantic/bridge.rs` / `provider.rs` / `fake.rs` | OK。`FmBridge::perceive` → `SemanticProvider::perceive` の 1 本の注入口。非画像パスは provider で拒否（A2）。`build_request` は `context.perception` を `image_facts` と同じ形で写像。Fake は既定で unavailable（テストが OS に触れない） |
| `src/models/file_context.rs` | OK。`PerceptionInfo` / `PerceptionLabel` は `image_metadata` と同じ流儀（`skip_serializing_if` + default）。完全リテラルの 3 箇所（normalize 本番 + search_terms の単体 2）のみ `None` を追加し、`..Default::default()` を使う既存テストは不変 |
| `src/rename/service.rs` | OK。知覚は画像分岐の注入検査の後・analyze の前に 1 回だけ。失敗は `PerceptionReport::Unavailable(reason)` の縮退で Held にしない。ラベル丸写しの保留は正規化等値のみ（部分一致を見ない）。`PlanFacts` で引数を 7 個以下に保ち、`finish_plan` / `execute_plan` / journal / undo は共用のまま |
| `src/main.rs` | OK。text は 1 行（最大 6 件＋残数）、JSON は `perception` 1 キー追加のみで既存キーは不変。Noop/Held と文書では出力しない |
| `schemas/fm-rename-v1.json` | OK。`perception` は required に入らず、`labels.maxItems = 12` の任意プロパティとして文書化。既存の schema テストは無修正で通る |
| `tests/v094`（17 件、うち `#[ignore]` 1） | OK。すべて Fake 注入・一時 dir・HOME 隔離。`#[ignore]` は実 Vision のみを呼び、assert は件数・順序・長さに限定。合成画像はテスト内 PNG ライタ（zlib stored・CRC32・adler32）で外部ツール・新規 crate を使わない |

## 3. BDD 網羅（P1〜P5・C1〜C3・W1〜W6・S1〜S3・N1〜N4）

- P1 正常/ P2 整列と bound / P3 失敗分類 / P5 縮退（stub スイート実測）: `perception.rs` 単体 4 件＋v094。P4 実機は `#[ignore]` で 12 件のラベルと 0.155〜0.625 秒を実測。
- C1〜C3 契約: v094 と contract 単体（既存キー不変・文書はキー無し・未知フィールド無視は v093 の既存テストが維持）。
- W1 ラベル送信と表示 / W2 文書は知覚しない / W3 注入名は推論も知覚もゼロ / W4 知覚失敗の縮退 / W5 ラベル丸写しの保留と通過 / W6 非破壊（execute・undo・拡張子・80 字・アンダースコア）: v094 で網羅。
- S1 プロンプト行と位置 / S2 壊れた画像の明示エラー（実機 `image-undecodable`）/ S3 解放規約（free 関数 1 つのみ）: ソース検査と実機で固定。
- N1〜N4: 686/0（両ビルド）・fmt/clippy/CC ゲート PASS・出力 ASCII（新規行は全て ASCII。em dash は追加していない）・JSON 追加のみ・隔離（実写真・実 Keychain・実補完 dir 不使用）。

## 4. 制約検査

- 新規 crate なし（`Cargo.toml` / `Cargo.lock` 無変更）。新規設定ファイル・別 DB・独自キャッシュ・外部プロセスなし（合成画像もテスト内で生成）。
- 既存コマンドの差分なし（organize / watch / classify / inspect / doctor / undo / uninstall / bench）。rename の文書系は実機で従来どおり提案することを確認（§9.3）。
- 新規 CC>=16 なし・最大 CC 15 維持（complexity 表と baseline-gate PASS で確認）。
- 命名規則維持（dry-run 既定・拡張子不変・80 字・上書き禁止・Journal 先行・undo）。
- 秘密情報の混入なし（差分を目視。`.env` は表示していない）。

## 5. plan-architect 記録との整合

- A1（プロンプト行の位置）: 実装時に**実際に `signals:` の後ろに置いてしまい**、v094 のソース検査が検出した（テストが機能した）。修正後は `signals:` の前で固定している。
- A2（非画像の拒否）/ A3（知覚は analyze より前・無駄コストを §10 に明記）/ A4（丸写し判定は正規化等値のみ）/ A5（表示は 6 件＋残数）: すべて反映済み。
- R1（小関数分割・CC）: `PlanFacts` 導入と関数分割で引数・CC とも制約内。R2（文書プロンプトのバイト不変）: `input.perception` がある場合のみ発火するブロックであり、文書経路に差分なし。v093 の文言テストが無修正で通ることを確認。

## 6. 残リスクの承認（QA・報告への引き継ぎ）

- 生成の wall time は 8.0〜17.2 秒（画像 1 件、知覚込み）。20 秒 timeout は未超過だがマージンは薄く、プロンプト調整中に 1 回 wall 20.2 秒（生成は完走）を観測した。超過を観測した時点で `DEFAULT_FM_TIMEOUT` を見直す条件付き繰り越しとする（§10）。
- ラベルの絶対確信度が低いため確信度はプロンプトに載せていない。実写真での命名品質は未検証（本ラウンドは合成画像のみ）。
- OCR は次回（D5）。知覚は分類器の識別子であり、画像内の文字（看板等）は対象外。
