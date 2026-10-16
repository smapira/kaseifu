# Homebrew対応 開発指示書（`brew install routeflags/tap/kaseifu`）

最終更新: 2026-10-16。

目的: `brew install routeflags/tap/kaseifu` で kaseifu を導入できるようにする。
成果物は tap 側の Formula と、それを維持する手順である。本書は指示書であり、実装自体は含まない。

## 現状（実測、2026-10-16。tap 側エージェントの実測を含む）

- tap リポジトリ (`github.com/routeflags/homebrew-tap`) は **public かつ空**である。ref が 1 つもなく `main` も未作成。初回 push は fast-forward ではなく **branch 作成**になる。public なので内部メモを置かない。
- Homebrew は本機で **7.0.9**。7 系では `brew audit <path>` が無効 ("Calling `brew audit [path ...]` is disabled!")。tap を brew に見せて **名前ベース**で実行する必要がある。path からの `brew install` / `livecheck` も拒否される。
- Homebrew 6.0 以降、非公式 tap は既定で untrusted。回避は `brew trust --formula <tap>/<name>` か、最初から fully-qualified 名で実行するかのいずれか。
- kaseifu 側に Homebrew 用資産はない。CI (`.github/workflows/rust-quality.yml`) は品質ゲートのみで、リリース artifact を作らない。
- リリースは annotated タグのみ (`v0.12.0` まで到達)。GitHub Release やバイナリ tarball の添付はない。ただし GitHub はタグごとにソース tarball を自動提供する (`.../archive/refs/tags/vX.Y.Z.tar.gz`)。
- 配布物は 2 バイナリ: `kaseifu` と `kaseifu-bench` (`Cargo.toml` の `[[bin]]`)。
- ビルドは Rust だけでは完結しない。`build.rs` が `xcrun --find swiftc` で Swift コンパイラを探し、`swift/kaseifu_fm_bridge.swift` をコンパイルしてリンクする。
- **ソースビルドには Xcode 本体が必要で、Command Line Tools では不可**（実測で確定）: `@Generable` のマクロ plugin `libFoundationModelsMacros.dylib` は Xcode.app 側にのみ存在し、CLT 配下には無い。CLT 選択時は `cargo install` が build 失敗する（本機実測: Xcode 26.6 選択で EXIT=0、CLT 26.6 選択で EXIT=101）。
- 落とし穴（ツール側の実装課題）: `build.rs` の SDK probe は `-typecheck` の `import FoundationModels` しか見ないため、CLT では probe をすり抜けて stub に降格せず **hard failure** になる。別課題として起票する（`20261016-open-build-rs-sdk-probe.md`）。
- tarball に `Cargo.lock` は同梱される（`--locked` が成立）。`LICENSE` ファイルは同梱されない（`Cargo.toml` に `license = "MIT"` はある）。
- 組織名の不一致に注意: 本体は `smapira/kaseifu`、tap は `routeflags/homebrew-tap`。Formula の `url` は本体タグ tarball を指す。

## 方針（推奨）

ソース tarball 方式を Phase 1 とする。理由:

- 新しいリリース基盤 (CI artifact、bottle 用ビルド行列) が不要。タグを打つだけで配布物 (自動 tarball) が生まれる。
- Formula は `url` + `sha256` + `depends_on "rust" => :build` + `depends_on xcode:` で完結する。
- bottle (ビルド済みバイナリ配布) は Phase 2 以降に回す。bottle には CI での release ビルド、notarization 相当の検討、OS/arch 行列が必要で、今回の最小目標を超える。

制約として記録すること: ソースビルド方式では利用者側に **Xcode 本体（Command Line Tools ではない。Xcode 26 以降、実測点は 26.6）** が必要になる。README の導入手順に明記する。**「CLT のみで足りるなら緩和する」という保留は実測により却下で確定**（上の plugin 欠落が根拠）。

追加の実測（導入コスト）: `depends_on "rust" => :build` により `openssl@4` と `llvm` の install が発生する。既存 rust がある Mac では `rust`/`sqlite`/`z3`/`libgit2`/`libssh2`/`pkgconf` の upgrade を誘発した。**「軽量に導入できる」とは書かない**。

## 作業範囲

### T1 Formula を作る (tap 側)

作業依頼は tap リポジトリの issue として起票済み（<https://github.com/routeflags/homebrew-tap/issues/1>、2026-10-16）。版や実測値が変わったら両方を合わせる。

- 場所: tap リポジトリ直下の `Formula/kaseifu.rb` (tap の規約どおり。`brew` が認識する配置)。
- 内容:
  - `desc` は一文の ASCII (例: `Semantic file organization for macOS`)。
  - `homepage "https://github.com/smapira/kaseifu"`。
  - `url` は最新リリースタグの自動 tarball。`sha256` は `curl -L` で取得した実測値を転記 (推測で書かない)。
  - `license "MIT"`。
  - `depends_on "rust" => :build`。
  - `depends_on xcode: ["26.0", :build]`。`XcodeRequirement` が比較するのは **Xcode.app のバージョン**であって SDK バージョンではないので、「macOS 26 SDK を含む条件」とは書かない。実測点は Xcode 26.6。
  - `install` は `cargo install --locked --path .` 相当 (lock 厳密。再現性のため `--locked` を使う)。両バイナリが入ることを確認する。
  - `livecheck` ブロックは **書かない**（既定の Git 戦略がタグを追う。明示するなら `strategy :git`）。GitHub Releases が 0 件なので `GithubLatest` は使えない。
  - `depends_on macos:` は付けない（バイナリは FoundationModels を weak link するため macOS < 26 でも起動する設計）。
  - `test do` は副作用なしで終わるものだけ: `kaseifu --version`、引数なし `kaseifu` のガイダンス終了コード、`label --help` の ASCII 性。journal/Keychain/実ファイルに触れない。
- 検証値の記録: v0.12.3 の自動 tarball sha256 =（下記、タグ push 後に実測して転記）。v0.12.0 は `LICENSE` を含まず、v0.12.1 は Homebrew の `SDKROOT` で、v0.12.2 は sandbox 内の二重 `sandbox-exec` でビルドに失敗するため、**v0.12.3 以降を pin する**。版上げのたびに実測し直す。
- 禁止: force push、tap 履歴の書き換え。初回 push 先は tap の `origin` のみ（**branch 作成**になる）。

### T2 導入手順を書く (kaseifu 側 README)

- README の導入節に、trust を明示した手順を足す（何が起きるかが読み手に分かる形）:

  ```sh
  brew tap routeflags/tap
  brew trust --formula routeflags/tap/kaseifu
  brew install kaseifu
  ```

  併記として簡略形 `brew install routeflags/tap/kaseifu` を挙げる（trust 不要だが理由が読み取りにくい）。
- 前提を同じ節に明記する: **Xcode 26 以降（Command Line Tools では不可。理由は Foundation Models の Swift マクロ plugin が Xcode 同梱のため。欠けると install 時に build 失敗として現れる）**、Rust は Formula の build 依存として brew が入れる（ただし `llvm` 等の大きめの依存が入る）。
- 対応 macOS: `depends_on macos:` を付けない旨と、動作確認は macOS 26 系である旨を記録する。
- `cargo install` 手順は残す (両立)。

### T3 版上げ手順を固定する

- kaseifu のリリース (`chore: release vX.Y.Z`) が終わったら、Formula の `url`/`sha256`/`version` を更新する手順を本書または README に残す。
- 手順: tarball を取得 → `shasum -a 256` → Formula 更新 (`url`/`sha256`/`version`) → **brew に tap を見せる**（これが無いと audit/test/livecheck が回らない）→ 検証 → tap へ push。
- tap の可視化: `ln -s "$PWD" "$(brew --repository)/Library/Taps/routeflags/homebrew-tap"` を張ってから名前ベースで実行する。
- livecheck がタグを追う運用なので、「GitHub Release を作らない」現在のリリース方式と整合する。
- 自動化 (livecheck bot、bump PR) は Phase 2。手動でも確実に回る手順を先に作る。

### T4 検証する（ツール側で先に実測済みの部分）

Formula が実行する install コマンド自体は Homebrew を介さずに検証済み（2026-10-16、本機）:

- v0.12.0 の自動 tarball を展開し、`cargo install --locked --path . --root <tmp>` を実行 -> exit 0（実測 58 秒、warm な registry）
- 入ったバイナリ: `kaseifu` と `kaseifu-bench` の 2 つ
- `kaseifu --version` -> `kaseifu 0.12.0`
- 引数なし `kaseifu` -> guidance を出して exit 0（test block の前提）
- `kaseifu label --help` -> 非 ASCII 行 0（ASCII 性の前提）
- 隔離 HOME にファイルを1つも作らない（config / journal / completion の書き込みなし）

残るのは Homebrew 経由の検証（下記）。
### T4 検証する

**受け入れテストは GitHub 経由で行う**（2026-10-16 のユーザー方針）。tap の push 後に:

```sh
brew tap routeflags/tap
brew trust --formula routeflags/tap/kaseifu
brew install kaseifu
kaseifu --version
```

これは T3（版上げ手順）の実地検証を兼ねる。push 前のローカル smoke としてのみ、tap を `ln -s` で見せて名前ベースの `--build-from-source` を回してよい（path 形式は Homebrew 7 で無効）。

- install / test / audit / livecheck が通ること（audit 警告は理由付きで記録か修正）。
- 導入後の `kaseifu --version` がタグと一致すること。
- 実ファイルの移動・journal・Keychain に触れないこと（test block 側でも `KASEIFU_KEY_SOURCE=env` と `KASEIFU_NO_COMPLETION_UPDATE=1` を設定する）。
- install は prefix の変更（`rust`/`llvm`/`openssl@4` の install、既存 formula の upgrade）を伴うため、**実行前に確認を取る**。

#### 受け入れテストは CI で回す（2026-10-16 追加、v0.12.2/v0.12.3 で 2 件の実バグを検出）

- `.github/workflows/homebrew-acceptance.yml`: `macos-26`（使い捨て）で tap → trust → install → `--version` 照合 → `brew test` → 依存の記録。手動 (`workflow_dispatch`) + 毎週。失敗時は `~/Library/Logs/Homebrew` を artifact に回収する。
- ローカル prefix を変えないため、**Docker の代わりの隔離実行**としてこれを使う。Linux コンテナではこのプロジェクトは動かない（Swift ブリッジが Foundation Models を、ツール本体が Spotlight を必要とする）。
- **1 件目**: `SDKROOT` が CLT SDK を指すため `brew install` が `plugin for module 'FoundationModelsMacros' not found` で失敗（exit 101）。Xcode は選択済みで plugin も存在した。修正は v0.12.2（`build.rs` が Swift コンパイラ呼び出しで `SDKROOT` を外す）。
- **2 件目**: v0.12.2 でも `brew install` が失敗。今度は `swift-plugin-server produced malformed response` で、cargo log の一行目に `sandbox-exec: sandbox_apply: Operation not permitted` があった。原因は Swift driver がサブプロセス（plugin server 含む）を sandbox 化すること。Homebrew の build sandbox 内ではその二重 `sandbox-exec` が macOS に拒否される。修正は v0.12.3（`build.rs` が `-disable-sandbox` を渡す。外側の Homebrew sandbox は残る）。
- 再現環境のないローカルでも CI と同じ失敗を再現できたのは、Homebrew の `superenv` が `SDKROOT` を export するため（`Library/Homebrew/extend/ENV/shared.rb` の `SANITIZED_VARS` と `shims/mac/super/bin/xcrun`）。sandbox 側に opt-out は無い（`formula_installer.rb` に該当フラグなし）ため、修正はツール側で行う。

#### 実測済み（この worktree、2026-10-16）

- **Xcode 26.6 選択**: v0.12.1 tarball に対して `cargo install --locked --path . --root <tmp>` = exit 0（58 秒）。2 バイナリが入り、`--version` がタグ一致、引数なし実行は exit 0、`label --help` は ASCII、隔離 HOME に書き込みなし。
- **CLT 26.6 選択**（`DEVELOPER_DIR=/Library/Developer/CommandLineTools`、Xcode はインストール済みのまま強制）: **exit 101 で hard failure**。`swift/kaseifu_fm_bridge.swift:478` で `external macro implementation type 'FoundationModelsMacros.GenerableMacro' could not be found ...; plugin for module 'FoundationModelsMacros' not found` が出て、stub 降格せずにビルドが失敗する。tap 側の実測（同 EXIT=101）を独立に再現した。
- **Homebrew 経由の静的検証（prefix 変更なし）**: tap は公開済みで `brew tap routeflags/tap` が成功（1 formula）。`brew audit --strict --new` = exit 0（指摘なし）、`brew style` = no offenses、`brew livecheck` = `kaseifu: 0.12.1 ==> 0.12.1`。
- 未実施: 実マシンでの Xcode 無し確認（このマシンには Xcode があり、`DEVELOPER_DIR` の強制で代替した）、**`brew install` と `brew test`**（後者は実測済みの `cargo install --locked` + test block の前提確認で大半を代替できるが、brew の orchestration 自体は未観測）。

注記: 当初 T4 に「push 前の smoke check」として `--build-from-source` を含めていたが、bottle が無いためそれは受け入れテスト（同じソースビルド）と重複する。静的な audit / style / livecheck だけを軽い確認として残し、install は「どのマシンで prefix を変えるか」の判断に属する。

未実施項目は [`../issues/waiting-user/20261016-waiting-clean-mac-verification.md`](../issues/waiting-user/20261016-waiting-clean-mac-verification.md) に記録した。

## 受け入れ条件

- Xcode 26 以降を入れた macOS で、**GitHub 経由**で以下が通る（T3 の実地検証を兼ねる）:

  ```sh
  brew tap routeflags/tap
  brew trust --formula routeflags/tap/kaseifu
  brew install kaseifu
  kaseifu --version
  ```

  （trust を含まない `brew install kaseifu` は untrusted tap として失敗するため、受け入れ条件から外す。）
- Formula の `sha256` が実測転記であり、再取得で一致する。
- 版上げ手順書どおりに次バージョンへ追従できる (手順の正しさは上の検証と同じ手順で確認)。
- Xcode 無し / CLT のみのマシンでの挙動は別途確認する（未実施。実測済みなのは `DEVELOPER_DIR` 強制による hard failure まで）。

## 制約

- tap リポジトリはこのスレッドの Delta worktree ではない。編集前に attach してから触る。読み取りは端末経由でよい。
- tap 側の初回 push は **branch 作成**（ref が 1 つも無いため）。以降は fast-forward のみ。履歴書き換えなし。
- install 検証は prefix の変更を伴うため、実行前に確認を取る（§1.8 の依存解決結果を提示する）。
- kaseifu 本体の `Cargo.toml`/`Cargo.lock` は変えない (Formula は外部資産)。
- Formula の Ruby にも秘密情報を書かない。API キー・トークンは扱わない。
- テストは隔離環境のみ。実ユーザーのファイル・Keychain・journal を読み書きしない。

## 対象外 (Phase 2 以降)

- bottle (CI ビルド済みバイナリ) の配布。**2026-10-16 に「一旦保留」と判断**（Phase 1 の完成を優先）。実測済みの材料と受け入れ条件は [`../issues/waiting-user/20261016-waiting-bottle-distribution.md`](../issues/waiting-user/20261016-waiting-bottle-distribution.md) に保存した。bottle が無い限り、利用者には llvm を含む build 依存と Xcode 要件が残る。
- homebrew-core への移管。
- 版上げ bot / 自動 bump PR。
- zsh completion の Formula 組み込み (kaseifu 自体が `completions` を持つため、要否だけ記録しておく)。
