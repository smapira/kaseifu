# QA: AIリネーム P5b（Vision 知覚層の品質チェック）

実施方式: `/qa` スキル不在のため手順をなぞって実施（コード品質・パス検証・構造的一貫性・テスト実行）。実測のみに基づく。commit / push / tag はしない。

## 判定: 承認（問題なし。検収可能）

## 1. 実行した検証コマンドと実際の出力（2026-10-14）

- `cargo test --all-targets --all-features`: **686 passed / 0 failed**（ベースライン 664＋新規 22: v094 統合 17（`#[ignore]` 1 を含む）＋perception 単体 4＋contract 2）。`v094_ai_rename_p5b_vision_test` は 16 passed / 0 failed / 1 ignored。
- `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --all-targets --all-features`: **686 passed / 0 failed**（Swift ブリッジなし・Foundation Models SDK なしの縮退。知覚は `foundation-models-sdk-missing` の明示理由で unavailable になり、プランは Phase 5 と同じ提案に戻る）。
- `cargo fmt --all -- --check`: 違反 0。`cargo clippy --all-targets --all-features -- -D warnings`: 警告 0（実行中に `too_many_arguments` 1 件を検出し `PlanFacts` 導入で解消、`assertions_on_constants` 1 件を検出し実測 println に置換）。
- `bash scripts/quality_check.sh`: **OVERALL: PASS**。
  complexity 表: Functions 1201、CC<=10 が 1190（99.1% PASS）、CC 11-15 が 11、**CC>=16 が 0**、最大 CC 15（従来と同じ）、平均 2.85、baseline-gate PASS（新規・悪化の CC>=16 なし）。rustfmt PASS、clippy PASS、cargo-test PASS（passed 686, failed 0）、report-tests PASS、coverage 82.80%（report-only）。
- `cargo test --test v094_ai_rename_p5b_vision_test -- --ignored --nocapture`（実機 Vision）: 合成 PNG 1024x768 / 2,360,312 byte で **12 ラベル**（`outdoor 0.129, celestial_body 0.129, moon 0.129, recreation 0.039, sport 0.039, ping_pong 0.037, sky 0.028, sports_equipment 0.026, ball 0.026, sunset_sunrise 0.026, daytime 0.021, sun 0.021`）、**0.155〜0.625 秒**、降順・重複なし・64 文字以内。
- 実機 CLI（隔離 HOME=/tmp/kb-verify/home、一時 dir、合成のみ）:
  - 配線前バイナリ（HEAD `b9b7e68`）同一ファイル P02 → `2026-10-14_IMG_1234.png`（エコー、1.19 秒）。
  - 本実装 P02 → `2026-10-14_月.png`（14.91 秒 / 17.22 秒）、`2026-10-14_月のアウトドア風景.png`（8.01 秒）。知覚行 `perception: outdoor(0.129) ... (+6 more)` が dry-run text に出る。
  - `--format json`: キーは `current_name, date_source, operation_id, pattern, perception, proposal, reason, status, target`（既存キー全保持、追加は `perception` のみ）。文書は `perception: null`。
  - `--execute` → `月夜のボール投げ.png`、`operation_id: op-92374-1971978514555710000`、`status: executed`（journal `move` success、4.61 秒）。`undo --last 1` で `EXEC_0001.png` に復帰、二重 undo は `nothing left to undo`。
  - 縮退の実機確認: 画像でないバイト列（`BROKEN_0001.png`）→ `perception: unavailable (failed: image-undecodable: ...)` ＋ `2026-10-14_BROKEN_0001.png`（Phase 5 と同じエコー）。
  - 非退行: 文書（`note.txt`）は知覚行なしで提案、動画（`.mp4`）は `Error: .mp4 is a video; video rename arrives with Phase 6`、EXIF カメラの無い JPEG の P11 は `reason: missing: device` で保留。

## 2. 構造的一貫性

- 配置: 知覚層は既存の層（contract / ffi / bridge / provider）の隣に `src/semantic/perception.rs` を追加し、契約型は `contract.rs`、観測モデルは `models/file_context.rs`（`image_metadata` と同じ位置づけ）に置いた。新規テストは `tests/v094_ai_rename_p5b_vision_test.rs`（`v093` 使用済みの次番。版数衝突なし）。
- 差分範囲: 変更 15 ファイル＋新規 4（計画と issues を含む）。`Cargo.toml` / `Cargo.lock` 無変更。無関係変更・デバッグ残骸・秘密情報なし（`git status`・差分を目視）。
- 出力規約: 追加した text 行・JSON キーは ASCII（ラベルは Vision の識別子）。em dash の追加なし（既存コメントの em dash はそのまま）。非 TTY / NO_COLOR 経路は変更していない。
- テスト隔離: 実ユーザーの写真・`~/Pictures/`・`~/Desktop/`・実 Keychain・実補完 dir の読み書きなし。実ファイル移動は一時 dir 内の合成ファイルのみ（この検証用 `/tmp/kb-verify` は検証後に削除済み）。

## 3. 受け入れ基準（原典 §15 の本ラウンド分）

- 3（画像のリネーム）: dry-run／`--execute`／Journal／`undo` の一巡を Fake と実機の両方で確認。
- 4/5/6（命名規則・日付・テンプレート）: 画像でも同一エンジン（P01/P02/P11 を実測）。拡張子不変・80 字・単語境界は v094 で固定。
- 10/11/12/13（dry-run 既定・上書き禁止・Journal・Undo）: 計画どおり。
- 17（既存機能を壊さない）: 686/0（通常＋stub）＋OVERALL PASS＋文書・動画の実機挙動で確認。
- §4.3 / §5.2（画像内容理解とタイトル生成）: 知覚（Vision）→ `input.perception` → Foundation Models の日本語タイトル（`月` / `月のアウトドア風景`）で実測。元画像は不変（2048 thumbnail の read-only 読み）。

## 4. 検証できなかったことと残るリスク

- 実写真での命名品質は未検証（合成画像のみ。合成夕景は分類器にも曖昧で moon/ping_pong/ball が同居する）。ラベル→日本語タイトルの経路自体は実機で確認済み。
- 生成 wall time は 8.0〜17.2 秒で 20 秒 timeout 未超過だがマージンは薄い（プロンプト調整中の 1 回のみ wall 20.2 秒、生成は完走）。超過を観測した時点で timeout を見直す条件付き繰り越し。
- OCR（`VNRecognizeTextRequest`）は次回送り（動くことは実測済み: 0.397 秒で `OPEN` / `9:00 - 18:00`）。画像内文字とファイル名は同種の注入面なので、需要と対策をセットで設計する。
- Vision のラベル名は OS のリビジョンで揺れるため、テストは件数・順序・長さのみを固定する（ラベル名の断言はしない）。
- commit / push / tag はしていない（親スレッドが実施）。
