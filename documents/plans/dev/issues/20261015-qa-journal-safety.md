# QA 品質チェック: ジャーナル安全性と設定の正直さ（20261015b）

## 判定

**PASS**（品質ゲート OVERALL PASS、パス・リンク・構造の不整合なし）

## 対象

計画書 `documents/plans/dev/20261015b-journal-safety-config-honesty.md` の全 9 タスク（T1–T9）。

## 1. テスト実行

```text
bash scripts/quality_check.sh
  rustfmt                PASS   violations 0
  clippy                 PASS   warnings 0
  cargo-test             PASS   passed 367, failed 0
  complexity             PASS   analyzer 0.0.25
  baseline-gate          PASS   no new/worsened CC>=16
  report-tests           PASS   complexity_report self-tests green
  coverage              PASS   report-only (line 80.66%)
OVERALL: PASS
```

- テスト数の内訳: 既存 346 + 新規振る舞いテスト 19 + destinations 単体 2 = **367 passed / 0 failed**。
- 新規 CC16+ なし（最大 CC 15 = 既存 `apply_setting`、今回の変更箇所は最大 CC 12 以下）。
- `reports/quality/baseline.json` は未更新（意図的な複雑度変化なし。ゲートも PASS）。
- カバレッジは report-only（80.66%、前回 ~71% から変化はテスト追加効果を含む）。

## 2. パス・リンク検証

- 新規文書の相互参照: 計画書 → `issues/20261015-plan-architect-journal-safety.md` /
  `issues/20261015-reviewer-journal-safety.md` / `issues/20261015-qa-journal-safety.md`（本ファイル）— 存在確認済み。
- 実ユーザーパス・API キーの漏洩: `grep` で変更ファイルに `bookair` / 実ホームパスの混入なし。テストは合成パス（`/Users/test/...`）と隔離 HOME のみ。
- README の記述（設定表・Undo・Doctor・project structure）が実装と一致することを CLI 実測で確認。

## 3. 構造的一貫性

| 検査 | 結果 |
|---|---|
| `config/default.toml` と `src/config/defaults.rs` / policy 定数の一致 | テストで固定（`v051_default_toml_documents_only_settings_that_exist`） |
| 旧セクション（`[scanner]` `[context]` `[storage]`）の残存参照 | default.toml から削除済み。他コードに読取参照なし（grep 0 件） |
| `documents/development.md` の解決順記述 | `src/config/defaults.rs` を指すよう更新済み |
| 旧設計文書（`20261014d-*`） | 実測日付きの歴史的記録として更新しない（変更は `20261015b` + CHANGELOG が正） |
| 生成物の ig管理 | `reports/quality/*` は git 管理外（logs/raw/complexity 等）。baseline.json のみ管理対象で未変更 |
| 新規ファイルの tracked 化 | 計画書・issue 2 件・テスト 4 件を `git add` 済み（未追跡ファイル 0） |
| `.gitignore` | 未変更（親管理。`/data/*.db` は旧ジャーナル検出用にそのまま有効） |

## 4. 振る舞いの実測（再現手順付き）

- **R8**: 新規テスト `v051_system_directory_rule_test`（3 件）+ `destinations.rs` 単体 2 件。旧判定の再現（python）で `~/var` 等が EXCL→keep、`/System` 等が EXCL のままを確認。
- **R5**: 隔離 HOME で `undo` を CWD 違いで 2 回実行 → 同一の絶対ジャーナルを作成。`doctor` が `ok Journal: <path> (no journal yet)` を表示、ファイル未作成。旧 `./data/kaseifu.db` 設置時は `FAIL Legacy Journal: found data/kaseifu.db (old CWD-relative journal; not read, not migrated; ...)` + ファイル不変。
- **R1/R2**: 合成 DB（3h 前 pending）→ `doctor` が `FAIL Journal: ... 1 pending, 1 possibly interrupted; pending file state: move: source absent, destination present`。`undo` が warning 行を stdout に出し、pending 行は実行後も pending、通常の success move のみ復元。
- **F-05/F-12**: `config show` が `dry_run = true (inactive; moves are controlled by --execute)` 等を表示。JSON に `inactive_settings` 追加・既存キー維持。`config set storage.database` は `unknown setting` で exit 1。

## 5. 改善提案（優先度低、任意）

1. 🟢 0 バイトジャーナル（クラッシュ境界）を「未初期化」と別扱いにする余地はあるが、誤警報のリスクより検出の見落としを避ける現状が安全側。
2. 🟢 `doctor` の Legacy 検出は CWD のみを見る。複数ディレクトリの旧ジャーナル探索は将来課題（設計書 §3.2 に記録済み）。

## 備考

判定は「検出して報告」の要求範囲（自動復旧なし）を満たし、実ユーザーデータの読み書きなし・新規依存なし・JSON 互換維持を確認した。
