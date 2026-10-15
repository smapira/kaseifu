# 残タスク: `LICENSE` ファイルがリポジトリに無い

- status: done
- milestone: M4 - Measurement and release hygiene
- completed: 2026-10-16 (Phase 1, tool side)
- 出典: Homebrew 対応の実測（tarball 内容の確認、2026-10-16）
- 優先度: 低（機能には影響しないが、配布物としての作法と監査の観点で望ましくない）

## 現状（実測）

- `Cargo.toml` に `license = "MIT"`、README 末尾に `## License` / `MIT` はある。
- しかしリポジトリに `LICENSE` ファイルが**無い**（`ls LICENSE*` が該当なし）。
- したがって GitHub の自動ソース tarball にも `LICENSE` が含まれない（v0.12.0 の tarball で実測確認）。
- Homebrew の Formula は `license "MIT"` を宣言するが、ライセンス本文は tarball 側に無い状態になる。

## やること

- `LICENSE`（MIT、著作権者名と年）をリポジトリ直下に追加する。
- `Cargo.toml` の `license` と表現が矛盾しないことを確認する。
- リリース手順（`chore: release vX.Y.Z`）で追加物が漏れないことを確認する。

## 受け入れ条件

- 次のタグの自動 tarball に `LICENSE` が含まれる。
- 次回リリースの Formula 監査（`brew audit`）でライセンス周りの指摘が出ない。

## Verification (2026-10-16)

- リポジトリ直下に MIT 本文の `LICENSE` を追加（著作権表示: `Copyright (c) 2026 smapira`）。
- 実測: v0.12.0 の自動 tarball には `LICENSE` が含まれない（`tar xzf` 後に `ls kaseifu-0.12.0/LICENSE` が該当なし）。つまり**次のタグで初めて同梱される**。
- `Cargo.toml` の `license = "MIT"` と README 末尾の `MIT` に矛盾なし。
- 著作権者名は暫定（`smapira`）。法人名义に変える場合はこのファイル1行の修正で済む。
- コード・依存には触れていない。

## 制約

- コード・依存に触れない（ドキュメントとライセンス本文のみ）。
