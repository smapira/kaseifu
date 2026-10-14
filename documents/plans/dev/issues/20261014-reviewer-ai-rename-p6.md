# reviewer 検収: AIリネーム P6（`20261014l-ai-rename-p6.md` の実装）

レビュー方式: 本環境に `/reviewer` スキルが無いため手順をなぞって実施（計画との照合・実測の再確認・退行検査・差し戻し判定）。根拠はすべて `read_file` / `grep` / `terminal` の実測。

## 判定: 承認（差し戻しなし。R1〜R3 の観点は下記のとおり確認済み）

## 確認済み項目

| 観点 | 実測結果 |
|---|---|
| 計画の T1〜T21 がすべて着地 | OK。`VideoInfo` / `src/{context,rename,semantic}/video.rs` / ffi + stub / bridge / fake / provider / rules / service / main / Swift / schema / tests / README が揃う。`git status --short` で変更 21・新規 7（+ S 済みフィクスチャ 3） |
| 優先順位 1（AVFoundation でメタデータと代表フレーム） | OK。実機で 30 秒 -> 4 枚 / 130 秒 -> 5 枚、メタデータと作成日時を実測（計画 §9.2） |
| 既存挙動の不変 | OK。画像プロンプトのバイト一致を HEAD との文字列比較で確認（`visual_labels` / `picture's content` / `Write the title ... that content` / 画像 gate の 4 箇所が完全一致。差分は字下げと `target_type == "video"` の追加行のみ）。文書・画像の経路は `video_perceive` を呼ばない（v095 W2 + CLI 実機で `video:` 行なし） |
| 契約の add-only | OK。`FmInput` の完全リテラルは 5 箇所だけ（`contract.rs` x2 / `provider.rs` x1 / `v093` x2）、既存キー削除なし。JSON 実機出力で既存 9 キー + `video`、`perception` は画像と同じ形 |
| 失敗分类（原典 §12） | OK。音声のみ（`video-no-video-track`）・破損（`video-unreadable`）・抽出/分類不能（`video-frames-unavailable`）・知覚不可（`unavailable`）・契約不正（`malformed`）の 5 分岐が Held + 理由になり、モデルは呼ばれない（v095 W4 + CLI 実機 0.08〜0.11 秒） |
| FFI の所有権・例外 | OK。戻り値は既存 `kaseifu_fm_free_result` で解放（ソース検査で `@_cdecl` は 1 つのまま）。Swift は `try?` / `catch` で全失敗を JSON 化し、エラー詳細は domain + code に丸めて改行なし（実機で確認） |
| 読み取り専用 | OK。Swift に `AVAssetWriter` / `CGImageDestination` / `FileManager` / `writeToFile` / `AVAssetReader` は存在しない（v095 S4 のソース検査）。フレームはメモリ上のままで書き出さない |
| 層構成・再利用 | OK。知覚の入り口は `FmBridge` のままで、ラベル整形は `kfLabels` を画像とフレームで共有（`kfLabels(request)` 2 箇所）。Rust 側の bound は既存 `perception::bound_labels` を再利用 |
| 複雑度・品質ゲート | OK。`bash scripts/quality_check.sh` = OVERALL PASS、**CC>=16 = 0**、最大 CC 15（ベースライン同）、baseline-gate PASS。新規最大は `video_hold_reason` 10 / `plan_video_rename` 9 |
| テスト | OK。729 passed / 0 failed（ベースライン 686 + 43）。stub ビルドでも 729 / 0。テストは隔離 HOME・一時 dir・注入経路のみ（実ユーザーの動画・`~/Movies/` には触れない） |
| 依存・設定 | OK。`Cargo.toml` / `Cargo.lock` 無変更、新規設定ファイル・別 DB・独自キャッシュ・外部プロセスなし |
| 出力規定 | OK。非 TTY / JSON で `label: value` 行、TTY で既存 ASCII パネル。em dash なし・内部 Phase 語なし（v092/v095 のテスト + 実機）。`--format json` は追加のみ |
| フィクスチャ | OK。合成 2 件（2,457 B / 8,580 B）に出自とサイズを `tests/fixtures/video/README.md` で明記。`git add` 済み（未追跡のままにしない） |
| 秘密情報 | OK。計画書・コード・テスト・JSON・ログに鍵・トークン・個人データなし。`.env` の中身は表示していない |

## 残る注意（QA・次ラウンドへの引き継ぎ）

- R1（QA へ）: `KASEIFU_FM_FORCE_STUB=no-sdk` のスイート（729 / 0）と、実 AV 経路 3 テストの「実経路 or 明示 unavailable」二択が機能していることを確認する。
- R2（QA へ）: 隔離 HOME・一時 dir の検証で実ユーザーのディレクトリを読み書きしていないこと、フィクスチャが読み取りのみであることを確認する。
- 既知の限界（差し戻し対象ではない。§9・§10 に記録済み）: (a) Held の動画は理由だけで、抽出したフレーム数・メタデータを表示しない（Held はレポートを持たない既存設計に合わせた）、(b) `video_hold_reason` は `PerceptionError::Failed{detail}` の理由 prefix を見る文字列マッチ（既存の Failed の表象に合わせた。単体ではなく統合テストで固定）、(c) webm / mkv / avi は対象外（AVFoundation が読まないため、未知型として fail-fast）、(d) 動画知覚は同期 FFI で Rust 側 timeout を持たない（フレーム上限・tolerance・maximumSize で有界。実測 0.03〜2.0 秒）。
