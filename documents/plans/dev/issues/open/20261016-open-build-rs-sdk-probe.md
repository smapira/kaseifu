# 残タスク: `build.rs` の SDK probe が CLT で hard failure になる

- status: open
- milestone: M4 - Measurement and release hygiene
- next owner: implementer
- blocker: actionable
- 出典: Homebrew 対応の実測（tap 側 `20261016f-tool-side-agent-brief.md` §1.3 / §2-B、2026-10-16）
- 優先度: 中（Homebrew 経由の導入失敗が「分かりにくいビルドエラー」として現れる）

## 現状（実測）

- `src/build.rs` の `sdk_has_foundation_models` は `-typecheck` で `import FoundationModels` が通るかだけを見る。
- CLT 選択時（`DEVELOPER_DIR=/Library/Developer/CommandLineTools`）が実測で失敗する。SDK は 26.5 で `import FoundationModels` の typecheck は通ってしまうため、**probe をすり抜けて** `compile_and_link` が `swift/kaseifu_fm_bridge.swift` をコンパイルし、`@Generable` のマクロ plugin 欠落で panic する。

  ```
  error: external macro implementation type
  'FoundationModelsMacros.GenerableMacro' could not be found for macro 'Generable(description:)'
  plugin for module 'FoundationModelsMacros' not found
  ```

- 根因: `libFoundationModelsMacros.dylib` は Xcode.app 配下にしか存在しない（本機実測: `/Applications/Xcode.app/.../host/plugins/` にあり、`/Library/Developer/CommandLineTools/usr/lib/swift/host/plugins/` には無い）。
- 実測結果: Xcode 26.6 選択で `cargo install --locked --path .` は EXIT=0、CLT 26.6 選択で EXIT=101。
- 文書側は [Homebrew 対応開発指書](20261016d-homebrew-support.md) で「CLT 不可」を前提に確定済み。

## やること（候補）

- probe を強化して、マクロ plugin の所在を見る（例: `swiftc -print-target-info` や plugins ディレクトリの存在確認）。通らなければ既存の `no-sdk` stub に降格し、警告の文言を「Xcode が必要」まで具体化する。
- 降格にした場合の帰結を明記する: CLT だけの Mac では AI rename が使えないスタブバイナリができ上がる（ビルドは成功する）。それが望ましいか、build 失敗のままにするかは製品判断。
- どちらを採るかは判断材料を添えて記録する。

## 受け入れ条件

- CLT 選択時の振る舞いが意図どおり（stub 降格、または理由の明確な失敗）で、テストまたは実測で固定されている。
- README / 導入手順の記述と矛盾しない。

## 制約

- CC 16 以上を作らない。新規 crate 依存なし。
- 検証は隔離 HOME と一時ディレクトリのみ。
