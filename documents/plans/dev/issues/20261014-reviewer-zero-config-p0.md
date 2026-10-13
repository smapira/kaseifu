# Reviewer 検収: ゼロコンフィグ P0（ZC-1 / ZC-2 / ZC-3 仕上げ / ZC-4）

- 日付: 2026-10-14
- 対象: 計画 `documents/plans/dev/20261014e-zero-config-p0.md`、差分 `src/main.rs`, `src/auth/{mod,acquire}.rs`,
  `src/doctor.rs`, `tests/v052_zero_config_{key,cli}_test.rs`, `tests/v051_journal_path_test.rs`, `README.md`,
  `CHANGELOG.md`, `documents/development.md`
- 判定: **承認**（重大な指摘なし。注意点 3 件を記録）

## 確認したこと（受け入れ基準との対応）

| # | 基準 | 確認方法 | 判定 |
|---|---|---|---|
| 1 | TTY: 対話入力→保存→判定表示 | 実 PTY テスト（`libc::openpty`）で `Jev API key` プロンプト→`.env` 0600 保存→`POLICY`/`LEAVE` 表示・exit 0。手実測（`pty_run.py`）でも同一 | OK |
| 1' | `env` モードで Keychain 0 回 | 注入 `CountingStore` で読み書き 0 回を固定（`v052_env_mode_writes_dotenv_with_0600_and_zero_store_calls`） | OK |
| 2 | 非TTY でハングせず非0 | `classify` / `watch` で exit 1、案内に `kaseifu setup` / `inspect` / `doctor`、`.env` も `data/` も作らない | OK |
| 3 | 鍵が出力・ログ・JSON に出ない | PTY テストで `grep 0 件`、doctor JSON にもキー無し、エラー文にもキー無し（`v052_errors_never_contain_the_key`） | OK |
| 4 | 引数なし exit 0・副作用なし | `v052_no_args_prints_guidance_without_side_effects`（`data/` 非生成・完了ファイル未更新・stderr 空）、`--help`/`-h` 互換 | OK |
| 5 | FAIL 行に次アクション、全 ok で exit 0、env 即時完了 | `v052_doctor_fail_rows_carry_next_commands` / `v052_doctor_is_all_ok_in_a_ready_environment` / `v052_doctor_json_keeps_existing_keys_and_adds_the_hint` | OK |
| 6 | 旧ジャーナル 0 操作の案内・ファイル不変 | 手実測（0 行 = `safe to delete` + `rm ...` ヒント、N>0 = 保持案内、読取不可 = `could not read it`）で sha256/size/mode が前後同一 | OK |
| 7 | 品質ゲート | fmt / clippy / test 385 / complexity（CC16+ 0）/ baseline-gate すべて PASS | OK |

## コードレビュー所見

- **秘密の扱い**: `acquire.rs` は値を戻り値として返すのみでログ・エラーに埋めない。`render_value` は `dotenvy` のパース規則
  （単引用符は非展開、`$`/`\`/空白/`#` は要引用符）に合わせて往復一致をテストで固定。`.env` は 0600 + 一時ファイル rename。
- **モードの意味不変**: 分岐は既存 `KeySourceMode::keychain_allowed()` のみ。`env` は保存も再解決も Keychain 非接触。
- **保存後の再解決**: メモリ上の値を直接使わず `resolve_for` をやり直すため、経路の権威が 1 つに保たれる。
- **`--format json`**: 既存キーの削除なし（doctor は `next`/`ready`/`failing` の追加のみ）。ZC-1 の保存通知は stderr に出し、
  stdout の機械可読性を追加で壊さない。プロンプト自体が stdout に出るのは既存 `auth::prompt` の挙動で、本バッチで変更しない（再利用優先）。
- **doctor**: 検査の `ok` 判定・名称・既存 detail 語は不変。	`legacy_journal_result` は読み取り専用 `inspect_journal` の行数のみを使い、
  旧ジャーナルへの書き込み・移動・削除・隣接ファイル生成はしない（既存テストで固定）。
- **`watch`**: 鍵取得は `fsevents::spawn` より前、ループ内にプロンプト経路なし（`grep prompt::` で確認）。

## 注意点（ブロッカーではない）

1. **次アクションが「コマンド」でない FAIL が 1 つだけある**: 旧ジャーナルに N>0 行あるときの `(next: nothing to run: keep ...)`。
   自動で直す手段が存在しない（`doctor` は読み取り専用、削除はユーザーの判断）ため意図的に行動文にした。他の FAIL は実行可能なコマンドを含む。
2. **`auto` で空の `JEV_API_KEY=` が `.env` にあるケース**: 空値は Keychain フォールバックを抑止する既存仕様
   （resolver のコメント：`.env.example` のコピーで承認ダイアログを出さない）ため、対話保存後の再解決が失敗し得る。
   この場合は「保存できたが解決できない。空の `JEV_API_KEY` がマスクしているかも」と `doctor` を案内する（実装済み）。実 Keychain を使う自動テストは
   不可（制約）のため、この分岐はコード上のメッセージ確認のみ。
3. **release `auto` の素の `kaseifu`** は鍵状態表示のために Keychain を**読む**（書き込みなし）。doctor と同じ読み取りであり、`env` モードは非接触。

## 差し戻し

なし。Implementer の成果物を承認する。
