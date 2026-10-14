# plan-architect レビュー: AIリネーム P6 計画書（`20261014l-ai-rename-p6.md`）

レビュー方式: 本環境に `/plan-architect` スキルが無いため手順をなぞって実施（コード照合・再利用確認・制約検査・差し戻し判定）。照合は `read_file` / `grep` / `terminal` の実測のみに基づく。

## 判定: 条件付き承認（A1〜A3 を計画書へ反映済み。反映後の再レビューは不要）

初版計画書は、AVFoundation のメタデータ・代表フレームの実測（3 枚 / 5 枚、130 秒動画で 0.2 秒台、音声のみ 0.027 秒、破損 0.008 秒）、新 C ABI + Rust 側統合の層構成、add-only 契約、失敗の保留分类までを押さえていた。下記 3 点を検出し、計画書 §2・§4・§6・§10 へ反映した。

## 確認済み項目

| 観点 | 結果 |
|---|---|
| AVFoundation 実機 | OK。メタデータ（同期 0.006〜0.009 秒、`commonMetadata` の文字列で `+0900` を保持）、代表フレーム（2 秒 3 枚 / 130 秒 5 枚、位置は 10%..90% で原典 §5.3 の例と一致）、`AVAssetImageGenerator` + tolerance 1 秒 + maximumSize 2048、autolink `-framework AVFoundation`。ffmpeg は本機で壊れている（libx265 欠落）ことを実測し、フィクスチャは AVAssetWriter / afconvert で生成する方針にした |
| ABI の形 | OK。`kaseifu_fm_available` / `kaseifu_vision_perceive` と同じ「同期・文字列返却・`kaseifu_fm_free_result` で解放」規約を再利用。フレームをファイルに書かないので読み取り専用原則を守れる（一時ファイル経由の既存 ABI 再利用は不採用が妥当） |
| 再利用 | OK。ラベル整形は Swift の `kfPerceive` から `kfClassify` に切り出して画像・動画で共用（二重実装なし）。Rust の統合は `perception::bound_labels`（降順・床 0.01・重複除去・12 件・64 文字）をそのまま通す。失敗マッピングも `perception` 側の共通化で再利用（T3） |
| 契約の互換性 | OK。追加は任意の `input.video` のみで既存キーの削除・型変更なし。統合ラベルは既存 `input.perception.labels` に載せ、プロンプトの `visual_labels` 経路を 1 本に保つ（画像プロンプトのバイト不変条件を守れる） |
| 失敗分类（原典 §12） | OK。フレーム抽出失敗・デコード不能・音声のみ・視覚ラベル 0 件のいずれもモデルを呼ばずに `Held` + 理由。部分失敗は失敗フレームだけスキップして継続（§11.4 の「安全にスキップ」） |
| テスト隔離 | OK。Fake 注入 + 一時 dir + 隔離 HOME。実経路はコミット済み合成フィクスチャ 2 点（2,457 B / 8,580 B、出自明記）で CI でも通し、stub ビルドでは明示 unavailable を許容（v094 と同じ二択パターン） |
| 既存テストへの影響 | OK。`FmInput` の完全リテラル 4 箇所（`contract.rs:498,550` / `provider.rs:221` / `v093:193,611`）と `NormalizedFileContext` の完全リテラル 3 箇所（`normalize/mod.rs:117` / `search_terms.rs:117,145`）にだけフィールドを足す。assert は不変 |
| 複雑度 | 条件付き。`plan_video_rename` / `parse` / `merge_frames` は小関数に分割し、新規 CC>=16 を作らないこと（R1） |

## 指摘（計画書へ反映済み）

- A1 **`target_kind` の未配線**: `src/semantic/provider.rs:245-251` は画像以外をすべて `Document` にする。このままだと動画要求が `target_type: document` で送られ、プロンプトの動画分岐（S1）が発火しない。-> T9 に `TargetKind::Video` 分岐を明記（`contract.rs:44-60` の variant は既存）。
- A2 **predicate の層逆転**: `semantic::provider` の防御は `crate::context::image_metadata::is_image_file` を参照しており、`src/semantic/` は `crate::rename` を **0 箇所** しか参照していない（実測）。動画 predicate を `rename::video` に置いて provider から使うと新しい層逆転になる。-> `src/context/video.rs` に置き、`rename::video` と `semantic::provider` の両方が使う（T10 / §4-D1）。
- A3 **虚偽になるメッセージ**: `src/rename/document.rs:73-75` の「video rename is not available yet」は P6 着地後は虚偽。v092 の該当テストも同時更新が必要（タスク指示済み）。-> T15 と §2 に明記。未知拡張子のメッセージはスコープ外で変更しない（最小差分）。
## 残る注意（実装・レビュー工程への引き継ぎ）

- R1（reviewer へ）: `scripts/quality_check.sh` の complexity 表で新規 CC>=16 なし・既存最大 CC 不悪化を確認すること。
- R2（reviewer へ）: 画像プロンプトのバイト不変（動画分岐が `target_type == "video"` にだけ発火）と、文書・画像経路で `video_perceive` が呼ばれないことを確認すること。
- R3（reviewer へ）: 出力 ASCII・em dash なし・`--format json` の既存キー保持（追加のみ）を確認すること。
- Q1（QA へ）: 実機検証（合成動画の dry-run / `--execute` / `undo`、配線前後の提案名比較、所要時間、実ラベル、音声のみ、長時間）を §9 に実測記録すること。
- Q2（QA へ）: `KASEIFU_FM_FORCE_STUB=no-sdk` のスイートで動画経路の縮退（全スイート pass）を確認すること。
- Q3（QA へ）: テストが実ユーザーの動画（`~/Movies/` 等）・実 Keychain・実補完 dir を読み書きしないこと、`Cargo.toml` / `Cargo.lock` が無変更であることを確認すること。