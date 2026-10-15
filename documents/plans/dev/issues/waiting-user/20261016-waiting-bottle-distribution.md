# 保留: bottle（CI ビルド済みバイナリ）の配布

- status: waiting-user
- milestone: M4 - Measurement and release hygiene
- waiting on: ユーザー判断（Phase 2 の開始。2026-10-16 に「一旦保留」と決定）
- 出典: Homebrew 対応の検討（[開発指書](20261016d-homebrew-support.md) の対象外節、tap 側 `20261016f-tool-side-agent-brief.md`）
- 優先度: 低（Phase 1 のソースビルド方式で導入は成立する。bottle は導入コストの削減策）

## なぜ保留か

- Phase 1（ソース tarball 方式）を先に完成させる方針が決まった（2026-10-16）。
- bottle は Phase 1 の置き換えではなく追加であり、Phase 1 の受け入れが済むまで着手しない。

## 問題意識（bottle が無い場合に残るもの）

- 利用者側に Xcode 26 以降が必要（CLT 不可。実測）。
- `depends_on "rust" => :build` により `llvm` を含むビルド依存と、`openssl@4` が入る（実測）。
- 導入に数分かかり、既存 formula の upgrade を誘発し得る。

## 再開時に使う実測済みの材料

- ランナー: GitHub は macOS 26 を GA 提供。arm64 は `macos-26`（`macos-latest` も現在 macOS 26）、Intel は `macos-26-intel` / `macos-26-large`。
- bottle 生成手順: `brew install --build-bottle <formula>` → `brew bottle --json <formula>` → `brew bottle --merge --write <json>`（`--write` で formula に bottle ブロックを書く）。
- bottle DSL: `bottle do` / `root_url "..."` / `sha256 arm64_tahoe: "..."`。本機は macOS 26.4.1 arm64 なのでセルは `arm64_tahoe`。
- 配布先の選択肢: (a) kaseifu リポジトリの GitHub Release assets（`root_url` を `https://github.com/smapira/kaseifu/releases/download/<tag>/` にする。CI の `GITHUB_TOKEN` だけで完結し、tap への書き込み権限が不要） / (b) tap リポジトリの Release（Homebrew の慣例に近いが、kaseifu CI から tap へ push する PAT が要る）。
- セル範囲の制約: `arm64_tahoe` の1セルでは macOS 26 arm64 のみが恩恵を受ける。Intel と古い macOS はソースビルドに戻る（ランナー側の Xcode/SDK 制約による。Phase 3 候補）。
- bottle が使われない条件（Homebrew 公式ドキュメント）: `--build-from-source`、オプション指定、checksum 不一致、cellar が relocatable でない、など。
- 検証の難しさ: bottle は「リリースしてから」でないと作れない（formula が tap に存在し、`--build-bottle` で install できる必要がある）。したがって検証には tag/branch の push が伴う。

## 受け入れ条件（再開時）

- `brew install kaseifu` が bottle を取得し、利用者側に Xcode も llvm も不要であること（実機または clean Mac で実測）。
- bottle の sha256 が実測転記で、再取得で一致すること。
- Phase 1 のソースビルド経路が壊れていないこと（Intel / 古い macOS のフォールバック）。

## 制約

- 新規 secret を使う場合は、その置き場所をユーザーに案内する（エージェントは secret を書かない）。
- tap の履歴を書き換えない。
