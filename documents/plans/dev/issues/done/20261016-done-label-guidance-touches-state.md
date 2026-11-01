# `label` のガイダンス表示が journal DB を作る - done

- status: done
- completed: 2026-11-01
- milestone: M1 - Reliability and CLI clarity
- next owner: none (verify at merge time)
- blocker: none
- 出典: Homebrew Formula の test block 検証（tap 側 Issue #1 のコメント、2026-10-16）
- 優先度: 低（実害は「何もしないコマンドが状態ファイルを作る」こと）

## 原因（実測: v0.12.1 で報告、2026-11-01 に再現）

- `handle_rename` が先頭で `RenameStore::open(journal_path())` を呼び、その後で「ターゲット無しなら guidance」を返す順序だった。`RenameStore::open` は SQLite DB と親ディレクトリを作る。
- 再現（隔離 HOME、`KASEIFU_NO_COMPLETION_UPDATE=1`、stdin closed）: `kaseifu label` の実行後に `Library/Application Support/kaseifu/kaseifu.db` が存在した。
- 対照: トップレベルの `kaseifu`（サブコマンド無し）は従来から何も作らない（ZC-2）。

## 変更

- `src/main.rs::handle_rename`: 順序を「判定 → store」に入れ替えた。
  - 管理フラグ + ターゲットの組み合わせは、**store を開く前**に拒否する
  - 管理フラグ無し + ターゲット無し（guidance）は、**store を開かない**
  - store を開くのは管理フラグ経路（`--patterns` / presets / `--show-default`）と、実際の計画・実行経路だけ
- `tests/v115_label_guidance_state_test.rs`（新規3件）: guidance が journal もそのディレクトリも作らない、管理フラグは従来どおり答える、管理フラグ + ターゲットは状態を作る前に拒否される。

## 受け入れ条件（満たした）

- `kaseifu label`（引数なし）が隔離 HOME にファイルを作らない
- `label --patterns` / `--list-presets` / `--show-default` / 保存系は従来どおり動く（store が必要な経路は開いたまま）
- 既存テストは無変更で緑

## 検証

- `cargo test --test v115_label_guidance_state_test`: 3 passed / 0 failed
- `cargo test --all-targets --all-features`: 965 passed / 0 failed（0.13.0 系の直前基準 962、+3）
- `bash scripts/quality_check.sh`: `OVERALL: PASS`（rustfmt 0、clippy 0、CC >= 16 = 0）
- 手動（隔離 HOME、stdin closed）: `kaseifu label` の後に `Library` 配下へ何も作られないこと、`label --patterns` が従来どおり一覧を返すことを確認
