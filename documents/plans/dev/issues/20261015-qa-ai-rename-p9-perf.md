# QA 検収: AIリネーム P9（Performance / Quality）

すべて親スレッドが本機（macOS、Apple Intelligence 有効）で実測した。素材は合成の文書・画像・動画と `/tmp` の一時ツリーのみで、実ユーザーのファイルは読み書きしていない。

## 実行した検証

| 検証 | コマンド | 結果 |
|---|---|---|
| 全テスト | `cargo test --all-targets --all-features` | **795 passed / 0 failed / 4 ignored**（Phase 8 の 791 + `benchmark::perf` の単体 4） |
| 品質ゲート | `bash scripts/quality_check.sh` | OVERALL PASS（rustfmt 0 / clippy 0 / CC>=16 = 0 / 最大 CC 15） |
| 依存の不変 | `git diff --stat Cargo.toml Cargo.lock` | 空（`libc` は既存依存） |

## 測定の生値（転記）

- FFI: `available` 200 回 median 588〜610 µs（`SystemLanguageModel.default.availability` を含む）、存在しない画像パスへの `vision_perceive` 200 回 median 18.2 µs。
- 生成（製品経路 / n=5）: min 3882 / median 3938 / mean 4714 / max 7905 ms。
- 生成（セッション再利用 / n=5）: min 3940 / median 7986 / mean 7920 / max 11347 ms。
  - n=3 では両方向に逆転する（再利用 median 7222 / 製品 9960 ms）→ 一貫した差なし。
- 混入検査: 6 回すべて `false`（語・ファイル名）。ただし答えがファイル名音写に寄るため弱い証拠。
- 画像（1024x768 PNG）: 3 回 min 18.3 / median 22.5 / max 124.4 ms。
- 動画 2 秒（3 フレーム）: median 39.6〜43.7 ms。動画 600 秒（5 フレーム）: min 76.0 / median 77.5 / max 336.7 ms。
- フォルダ読み: 260 entry → 200 で打ち切り、398.2 / 544.7 / 532.7 µs。
- メモリ: `kaseifu-bench perf` 26 MB、単一改名（実生成）29.6 MB、50 項目バッチ 20.5 MB。

## 異常終了（SAFE10）の実測

- 計画中に `kill -9`（開始 4 秒後）: ファイル無変更、ジャーナルに行なし。
- 記録と成否書き込みの間の状態を直接作って検証: `undo` は `warning: 1 pending journal row (1 move, 0 undo); interrupted moves may already be at the destination` を出して **pending を選択しない**（`nothing left to undo`）。ファイルは無損。
- `doctor`: `Journal: ... (1 operation(s); 1 pending, 0 possibly interrupted; pending file state: move: source absent, destination present)`。

## 検証できなかったこと

- 実ユーザーのファイル（設計上、合成のみ）。
- コールドスタートの分離（最大値にウォームアップが混ざる）。
- 上限 50 のフォルダを実際に計画する時間（1 項目 1 生成のため数分かかる）。
- セッション再利用のメモリ・課金等、生成時間以外の効果。
- 本機の ffmpeg/ffprobe は動かない（libx265 欠落）ため、コンテナの検証は AVFoundation の読み値のみ。
