# zsh 補完実装の QA 検証

- 日付: 2026-10-15
- 対象: `src/completion/`（新規）、`src/main.rs`、`src/doctor.rs`、`tests/completion_test.rs`（新規）、ドキュメント 3 件
- 検証方法: `bash scripts/quality_check.sh`、実バイナリ実行、実 zsh（zpty）、#5 のパス・リンク確認

## 1. 品質ゲート

```
rustfmt                PASS   violations 0
clippy                 PASS   warnings 0
cargo-test             PASS   passed 297, failed 0
complexity             PASS   analyzer 0.0.25
baseline-gate          PASS   no new/worsened CC>=16
report-tests           PASS   complexity_report self-tests green
coverage               PASS   report-only (line 75.81%)
OVERALL: PASS
```

- テストは既存 279 + 新規 18 = **297 passed / 0 failed**（既存テストの変更は doctor checks 件数の 1 行のみ）。
- 複雑度: 新規関数 30 個の最大 CC は `skip_case_arm` の 9、全体の CC ≥ 16 は 0 件（ベースライン更新不要で `reports/quality/baseline.json` は未変更）。
- 依存追加は `clap_complete` 4.6.11（`Cargo.lock`）のみ。

## 2. 受け入れ基準の実測

| AC | 実測結果 |
|---|---|
| 1 生成と構文 | `kaseifu completions zsh` → 24,306 bytes、`zsh -n` OK。1 行目 `#compdef kaseifu`、2 行目印 |
| 2 インストールと冪等性 | 実機 `/opt/homebrew/share/zsh/site-functions/_kaseifu` を作成。再実行で `mtime`・サイズ不変（1791897588 / 24,306） |
| 3 自動更新 | 印 `v0.4.2` → `v0.0.1` 改竄後、`inspect` 実行で `v0.4.2` に戻る。書込不可ディレクトリ（0o555）・欠損は無変化で exit/stdout/stderr 差分ゼロ。`KASEIFU_NO_COMPLETION_UPDATE=1` で改竄が残る |
| 4 コマンド網羅と隠し | 全 11 トップレベル（inspect/scan/organize/undo/classify/watch/setup/auth/config/doctor/completions）とネスト（set-key/status/remove-key/show/set）を含む。`discover` は 0 件 |
| 5 doctor | `ok Completion: zsh completions installed (<dir>)` / `FAIL Completion: zsh completions stale (<dir>; v0.3.0 -> v0.4.2); …` / `FAIL … missing …`。JSON の `checks[]` にも `"name":"Completion"` |
| 6 実 zsh | `zsh -f` + fpath 明示 + `compinit -u -D` で `_comps[kaseifu]=_kaseifu`。さらに zpty で実補完を確認: `kaseifu ins<TAB>` → `kaseifu inspect`、`kaseifu <TAB>` の候補一覧に `discover` なし、`kaseifu auth <TAB>` → set-key/status/remove-key |
| 7 品質ゲート | `OVERALL: PASS`、297 passed、新規 CC ≥ 16 なし、新規依存は clap_complete のみ |
| 8 `.zshrc` | `shasum -a 256 ~/.zshrc` = `bb535b0d…84ceae`（作業前後で同一） |

### JSON バイト不変の証跡

- `inspect --format json`（fixture）: ベースライン binary（HEAD から `git archive` でビルド）と**バイト同一**。
- `classify --format json`: モック Jev（`choice=NONE`）で成功パスを再現し、ベースラインと**バイト同一**（89 bytes）。
- `discover --format json`: `score` は時刻依存で同一 binary の連続実行でも差が出るため、スコア行を除いた構造がバイト同一であることを確認。
- 3 コマンドすべてで、自動更新 ON（改竄→再生成）と OFF の stdout/stderr がバイト同一。

## 3. パス・リンク・構造の一貫性

- README に `### Shell completion` を追加し、検出順・無効化 env・doctor の見方・他シェルの制限を記載。Project structure に `completion/` を追加。
- `documents/development.md` に env 2 件と開発者向け注意（テストは実 site-functions に書かない）を記載。
- CHANGELOG は `[Unreleased] → ### Added` のみ（バージョン・タグは親が実施）。
- 相対リンク・参照パスの破損なし（README 内の相対パス記載は既存のまま）。
- 作業ツリーの diff にデバッグ出力・一時ファイル・無関係な整形変更・秘密情報なし。

## 4. テストの安全性

- 新規テストはすべて `KASEIFU_COMPLETION_DIR` を一時ディレクトリへ向けるか、`RefreshPlan` を直接注入する。
- 実ユーザーの Homebrew ディレクトリへの書込テストは存在しない（`grep -n "/opt/homebrew" tests/completion_test.rs` の 2 件は、いずれも「書かない」旨を述べた doc コメントと検出順の期待値で、書込先ではない）。
- 実機検証（AC2 のインストール）はテストではなく手動検証として実施し、成果物を残した。

## 5. 結論

受け入れ基準 1-8 を満たすことを実測で確認。**QA 承認**。
