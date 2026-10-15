# 未実施: Xcode 無し / CLT のみのマシンでの検証

- status: waiting-user
- milestone: M4 - Measurement and release hygiene
- waiting on: Xcode を入れていない Mac（または VM）
- 出典: Homebrew 対応の実測（[開発指書](20261016d-homebrew-support.md) T4、2026-10-16）
- 優先度: 中（失敗モードの利用者体験が未確認。hard failure するなら導入前に気づけるようにしたい）

## なぜ未実施か

- 手元の Mac には Xcode が入っている。Xcode 無しを模すために `DEVELOPER_DIR=/Library/Developer/CommandLineTools` を強制して hard failure を再現したが、これは「Xcode が存在するが選択されていない」状態であり、**Xcode が本当に入っていないマシン**とは環境が違う（`xcrun --find swiftc` の結果、最初から CLT を選ぶ系など）。
- clean Mac（rust / llvm が未導入）での `brew install` は prefix への大規模な導入を伴うため、この worktree では実行しない。

## やること

- Xcode 無しの Mac（CLT のみ）で `brew install kaseifu` を実行し、失敗の見え方を記録する:
  - build が失敗するか、stub に降格して**成功**するか（`build.rs` の probe 次第。関連: [`../open/20261016-open-build-rs-sdk-probe.md`](../open/20261016-open-build-rs-sdk-probe.md)）
  - 失敗する場合、利用者に何が出るか（`brew` のエラー + `cargo` のログ）。README の「Xcode 26 以降が必要」で足りるかの確認
- clean Mac（Xcode あり / rust 未導入）で `brew install` を通し、所要時間と導入された依存（`llvm` など）を記録する。
- 結果を README の前提記述に反映する。

## 受け入れ条件

- Xcode 無し環境での挙動が実測で記録されている（成功なら stub で動く旨、失敗なら理由が読める旨）。
- clean Mac での導入時間と依存が実測されている。

## 制約

- 検証用マシンの prefix を変更する操作なので、実行はユーザーの管理下で行う。
- 実ユーザーのファイル・Keychain・journal には触れない。
