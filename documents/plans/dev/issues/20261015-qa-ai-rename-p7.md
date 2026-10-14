# QA 検収: AIリネーム P7（品質・構造・隔離の検査）

QA 方式: 本環境に `/qa` スキルが無いため、`.github/agents/qa.agent.md` の手順（スコープ確認 -> 静的チェック -> 実行動作確認 -> レポート作成）をなぞって実施。すべて実測（`terminal` / `grep` / `read_file`）。

## 判定: 承認（品質ゲート PASS、構造・隔離の逸脱なし）

## 実測記録

| 検査 | コマンド | 結果 |
|---|---|---|
| フルスイート | `cargo test --all-targets --all-features` | **774 passed / 0 failed / 4 ignored**（exit 0。ベースライン 729 + 45） |
| stub スイート | `KASEIFU_FM_FORCE_STUB=no-sdk cargo test --all-targets --all-features` | **774 passed / 0 failed / 4 ignored**（exit 0。Swift 不在ビルドでも同じスイートが通る） |
| フォーマット | `cargo fmt --all -- --check` | 違反 0 |
| 静的解析 | `cargo clippy --all-targets --all-features -- -D warnings` | 警告 0（exit 0） |
| 品質ゲート | `bash scripts/quality_check.sh` | **OVERALL: PASS**（exit 0）。complexity: 1322 関数、CC<=10 1310（99.1%）、CC 11-15 = 12、**CC>=16 = 0**、最大 15（ベースライン同）、baseline-gate PASS、coverage 84.69%（report-only） |
| 依存の不変 | `git diff --stat Cargo.toml Cargo.lock` | 空（新規 crate なし） |
| schema 変更なし | `git diff --stat src/rename/store.rs src/storage/history.rs` | 空（新しいテーブル・カラム・別 DB を作っていない） |
| 新モジュールの副作用 | `grep -cE "fs::(write|remove_file|remove_dir|rename|copy)|File::create"` | `src/context/directory.rs` の実装部 0（全 12 件は `#[cfg(test)]` 内）、`src/rename/directory.rs` の実装部 0（全 3 件はテスト内） |
| 追加行の書き込み呼び出し | `git diff -U0` の追加行を目視 | すべて `src/rename/{service,validate}.rs` の `#[cfg(test)]` 内。実装部に新しい書き込みなし |
| テストの隔離 | v096 / 新規単体の書き込み先を `grep` | `std::env::temp_dir()` 配下（`work_dir(tag)`）と CLI の隔離 `HOME` のみ。`OperationHistory::open_in_memory` 21 箇所、実 journal を使うテスト 0 |
| 実ユーザーパス | 追加行・新規ファイルで `/Users/bookair18`、`/home/` を `grep` | 0 件（拒否規則のテストにある `/Users/test/...` は文字列であり、実パスを読まない） |
| 出力の ASCII / em dash | 追加行と新規ソースで `—` を `grep` | 0 件。CLI の guidance / help は ASCII（v096 の CLI テスト + 既存 v070 の全 help 走査が PASS） |
| 内部語の漏れ | `cargo test --test v070_zero_config_p1_help_test` | PASS（`rename --help` を含む全 help 面を自動走査。`Phase` / `TargetKind` / `DirectoryFacts` などの漏れなし） |
| JSON の互換性 | 実機 `--format json` + v096 | 既存 10 キーを保持し `directory` を追加（null を含めてキー集合は常に 11）。削除・型変更なし |
| README と実装の一致 | README のフォルダ記述と定数を照合 | 「at most 200 entries」= `MAX_DIRECTORY_ENTRIES` 200、「12 file names and 8 folder names」= `MAX_FILES` 12 / `MAX_FOLDERS` 8、「up to three document children」= `MAX_CONTENT_CHILDREN` 3、「never recurses into a child folder」= 深さ 1 固定。「no frame is written to disk」/「read-only」の記述は新モジュールの副作用 0 と一致 |
| 一時ディレクトリ | `/tmp/kb-p7/{e2e,e2e2,e2e3,e2e4,pre,probe,probe2}` | すべて検証用の合成ツリーと配線前バイナリ。repo 内は不変（`git status` に `target/` などの混入なし） |
| 秘密情報 | 計画書・issues・コード・テスト・実機出力の目視 | 鍵・トークン・個人データなし。`.env` の中身は未表示 |

## QA からの補足（残るリスク。差し戻しなし）

- **PDF の子の抜粋は Phase 4 のバイト走査の限界を引き継ぐ**（非圧縮 PDF では辞書・xref が抜粋に混ざる。実測は計画書 §9.3/§9.5）。名前と種別の分布が主な証拠になるケースが実在するので、実 PDF でのタイトル品質は Phase 9 の評価対象とする。
- **モデルの退化出力**（自身の型名 `KfSemanticPayload` など）を実測した。子の抜粋の選別（長い順に 4 件）と指示行の追加で本ラウンドの再現条件（PDF のみのフォルダ）では 0 回になったが、一般的な防御（出力の妥当性検査）は未実装。
- **推論失敗は exit 1**（例: モデルの guardrail violation `skipped: InferenceFailed(...)`）。既存 Phase 4 以降と同じ振る舞いで、ファイルは動かない。フォルダ経路で新たに導入した挙動ではない。
- 新規ファイル（計画書・issue 2・`src/{context,rename}/directory.rs`・`tests/v096_*`）は commit 前は未追跡になるため、情報が失われないよう `git add` でステージ済み（**commit / push / tag はしていない**。親スレッドの作業）。
