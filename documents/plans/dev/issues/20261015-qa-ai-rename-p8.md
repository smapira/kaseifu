# QA 検収: AIリネーム P8（Batch Rename）

検証はすべて親スレッドが実測した。実行環境: macOS、Apple Intelligence 有効、隔離 HOME `/tmp/p8-verify/home`、素材はすべて合成ツリー。

## 実行した検証と結果

| 検証 | コマンド | 結果 |
|---|---|---|
| 全テスト | `cargo test --all-targets --all-features` | **791 passed / 0 failed / 4 ignored**（ベースライン 774 + 17） |
| 縮退ビルド | `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --all-targets --all-features` | **791 / 0 / 4** |
| 品質ゲート | `bash scripts/quality_check.sh` | **OVERALL: PASS**（rustfmt 0 / clippy 0 / CC>=16 = 0 / 最大 CC 15 / baseline-gate PASS / coverage 84.91% report-only） |
| 依存の不変 | `git diff --stat Cargo.toml Cargo.lock` | 空（新規 crate なし） |

## 実機 CLI（合成ツリーのみ、隔離 HOME）

- **プレビュー**（`tree/` = `d/note.txt`、13.7 秒）: `proposals: 2` / `conflicts: 0` / `held: 0` / `status: proposed` / `mode: dry-run (pass --execute to rename)`。番号付きの項目行が2行。
- **衝突**（内容の同じ2ファイルに `--template reports`、12.0 秒）: `proposals: 0` / `conflicts: 2`、両項目 `[conflict: two or more items propose the same name (reports.txt); all are held]`、**ファイルは不変**。
- **実行**（`--execute`、stdin を `/dev/null`、13.4 秒）: `status: applied` / `order: applied children before parents (deepest first)` / `undo: kaseifu undo --last 2`。結果は `tree/<新名フォルダ>/<新名>.txt`（子が先に適用された証跡）。
- **一括 Undo**: `undo --last 2` -> `restored: .../tree/d`（フォルダ）-> `restored: .../tree/d/note.txt`（子）。ツリーは元に戻った。
- **上限**（55件、0.024 秒）: `held: 50` + `bounded: the folder holds more than 50 targets; only those were planned`。
- **JSON**: バッチは 10 キー + item 8 キー（単一ターゲットの形は不変）。
- **確認と非ブロッキング**: PTY では質問が出て入力待ちになる。`--yes` では出ない。`/dev/null`（非 TTY）では出ずに完走。
- **非退行**: `rename <file>` と `--recursive` 無しのフォルダ改名は Phase 7 のまま（v092-v096 が無変更で通過）。

## 確認した安全側の性質

- 検証に使ったのは `std::env::temp_dir()` 配下の合成ツリーと隔離 HOME だけで、実ユーザーのフォルダ・実ジャーナル・実 Keychain・実補完ディレクトリを読み書きしていない（`git status` にも無関係な変更なし。`.gitignore` は本ラウンドと無関係の既存差分）。
- 実ファイルを削除・移動したのは検証用ツリー内の合成ファイルのみ。

## 検証できなかったこと

- 実ユーザーのフォルダでのバッチ（設計上、合成のみ）。
- 深さの上限を実機 CLI で確かめること（単体テストで固定。実機は件数上限のみ）。
- 正規化差だけが異なる2提案の同時発生（適用時の再確認が上書きしないことは統合テストで固定）。
- 途中 SIGKILL の実測（1行ずつの `pending` -> `success` と `undo` の `pending` 無視はコードと既存テストの検証に留まる）。
- バッチの所要時間の一般化: 実測は 2件で 13.7 秒、55件（非対応のみ）で 0.024 秒。生成はモデルと内容に依存する。
