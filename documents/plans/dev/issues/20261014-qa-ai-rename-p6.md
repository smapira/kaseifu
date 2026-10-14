# QA 検収: AIリネーム P6（品質・構造・隔離の検査）

QA 方式: 本環境に `/qa` スキルが無いため手順をなぞって実施。すべて実測（`terminal` / `grep` / `read_file`）。

## 判定: 承認（品質ゲート PASS、構造・隔離の逸脱なし）

## 実測記録

| 検査 | コマンド | 結果 |
|---|---|---|
| フルスイート | `cargo test --all-targets --all-features` | **729 passed / 0 failed / 4 ignored**（exit 0） |
| stub スイート | `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --all-targets --all-features` | **729 passed / 0 failed / 4 ignored**（exit 0。実 AV 経路の 3 テストは `unavailable` + 理由を受容） |
| フォーマット | `cargo fmt --all -- --check` | 違反 0 |
| 静的解析 | `cargo clippy --all-targets --all-features -- -D warnings` | 警告 0（exit 0） |
| 品質ゲート | `bash scripts/quality_check.sh` | **OVERALL: PASS**。complexity: 1260 関数、CC<=10 1249（99.1%）、CC 11-15 = 11、**CC>=16 = 0**、最大 15（ベースライン同）、baseline-gate PASS、coverage 83.76%（report-only） |
| 依存の不変 | `git diff --stat Cargo.toml Cargo.lock` | 空（新規 crate なし） |
| フィクスチャの完全性 | テスト前後で `shasum -a 256 -c` | OK（テストが書き換えない） |
| テストの隔離 | 新規テストの書き込みを `grep` で列挙 | すべて `std::env::temp_dir()`（`work_dir` / `isolated_home`）のみ。実 HOME・実動画・`~/Movies/`・実 Keychain・実補完 dir は不明 |
| 新規ソースの副作用 | `grep -cE "std::fs::(write|remove_file|rename)"` | `semantic/video.rs` 0、`context/video.rs` 0、`rename/video.rs` 0（Rust は動画を開かない） |
| ハードコードパス | `/Users/`、`/Movies`、`Desktop` を新規ファイルで検索 | 0 件 |
| JSON の互換性 | 実機 `--format json` + v095/v092 のテスト | 既存キー 9 個を保持し `video` を追加。削除・型変更なし |
| 出力の ASCII / NO_COLOR | 非 TTY 実行の目視 + テスト | `label: value` 行、TUI なし、em dash なし、内部 Phase 語なし |
| ヘルプ / guidance | `rename` / `rename --help` | 型の列挙が実態と一致（mp4, mov, m4v, 3gp, 3g2）。"arrive later" / "not available yet" は消失 |
| 秘密情報 | 計画書・issues・コード・テスト・JSON の目視 | 鍵・トークン・個人データなし。`.env` の中身は未表示 |
| 一時ディレクトリ | `/tmp/kb-verify`、`/tmp/kb-pre`、`/tmp/kb-probe` | 検証用の素材・旧バイナリ・プローブのみ。repo 内は変わらない（`/tmp/kb-pre` は `git archive` から展開、.git に痕跡なし） |

## QA からの補足（残るリスク。差し戻しなし）

- 実動画・実写真での品質評価は本ラウンドの対象外（合成素材のみ。§9.4）。
- 動画 dry-run の wall は 8.1〜17.9 秒で `DEFAULT_FM_TIMEOUT` 20 秒以内だが余裕は薄い。timeout 超過を観測した時点で根拠付きに見直す（計画 D6）。
- モデルが動作を示唆する題名を出すことがある（計画 §9.5 に 3 版の実測）。フレームだけでは動作を正確に理解できないという原典 §5.3 の制限であり、プロンプトで抑制している。
- 新規ファイルの一部（計画書・issues・`src/*/video.rs`・`tests/v095_*`）は commit 前の段階では未追跡になるため、情報が失われないよう `git add` でステージ済み（commit / push / tag は親スレッドの作業）。
