# plan-architect レビュー: AIリネーム P7 計画書（`20261015c-ai-rename-p7.md`）

レビュー方式: 本環境に `/plan-architect` スキルが無いため、`.github/agents/plan-architect.agent.md` の手順（コンテキスト把握 -> コードベース照合 -> 再利用確認 -> フィードバック -> 差し戻し判定）をなぞって実施。根拠はすべて `read_file` / `grep` / `terminal` の実測で、計画書 §1〜§4 の記述と実コードを突き合わせた。

## 判定: 条件付き承認（A1〜A6 を計画書へ反映済み。反映後の再レビューは不要）

計画の骨格（フォルダ事実を context 層に置く、子の上限付きサンプリング、既存の bounded document reader の再利用、`input.directory` の add-only、新しい順 undo の不変条件で親子整合を説明し解決できない場合だけ拒否）は、原典 §5.4・§9・§10・§11.2・§12・§13.5 と実コードの両方に整合していた。一方で「実装すると計画どおりには動かない」点を 6 件検出し、計画書 §4（D2/D4/D8）・§5（F1/S1）・§6（T1/T5/T12）へ反映した。

## 確認済み項目

| 観点 | 実測結果 |
|---|---|
| 土台の実在（file:line） | OK。`TargetKind::Directory`（`src/semantic/contract.rs:41-61`）、`read_directory(path, max)`（`src/platform/macos/filesystem.rs:7`）、`read_bounded` / `extract_signals` / `contains_prompt_injection`（`src/rename/document.rs:83-106,163-202,334-339`）、`normalize::noise::{DirectorySummary,is_noise_file}`（`src/normalize/noise.rs:4-68`）、`move` 行と `undone_at`（`src/storage/history.rs:110-181`）。計画書 §2 の引用は全件一致 |
| 第三の summary を作らない | OK。`noise::DirectorySummary` は `noise.rs` の外に呼び出し元が無く（実測: `is_noise_file` / `noise_filter` / `directory_summary` の参照は同ファイル内のみ。`src/benchmark/scatter.rs:161` は同名の別関数）、再利用先として適切。`models::file_context::DirectorySummary`（親ディレクトリ用）の意味を変えない方針も妥当 |
| 完全リテラルの影響範囲 | OK。`NormalizedFileContext` の完全リテラルは **3 箇所**（`src/normalize/mod.rs:117`、`src/discovery/search_terms.rs:117,146`）で、他は `..Default::default()` を用いている（`src/semantic/provider.rs:393`、`tests/v042:52,76`、`tests/v31:79`、`tests/v090*`）ことを実測。`FmInput` の完全リテラルは 6 箇所（`src/semantic/provider.rs:242`、`src/semantic/contract.rs:537,590,646`、`tests/v093:193,613`） |
| 層の向き | OK。`src/discovery/` の import は `crate::models` と `crate::discovery` のみ（`crate::context` 参照 0）。`is_excluded_destination` は既に `src/operations/plan.rs:109`・`src/benchmark/{gate,cli}.rs` から呼ばれており、規則の実装を `src/context/directory.rs` へ移して `destinations` がそれを呼ぶ形は循環も逆転も作らない |
| ディレクトリ undo の現状 | OK（計画の主張どおり）。`src/operations/undo.rs:66` が `is_file()` を要求し、ディレクトリは `SourceNotAFile` で undo 不可。`last_successful_move` は `rowid DESC LIMIT 1`（`src/storage/history.rs:168-181`）で、CLI は `0..last` のループ（`src/main.rs:872-877`）= **undo は常に新しい順**という D7 の不変条件の根拠も実測で確認 |
| 虚偽メッセージ | OK。`src/rename/document.rs:64-67` の extension 無し文言は現行バイナリで実際にこれが出ることを確認（`$ kaseifu rename <dir>` -> `Error: <dir> has no extension; rename needs txt, md, pdf or html`、exit 1）。文言を assert する既存テストは 0 件（`grep "has no extension" tests/` が空） |
| 既存ヘルプ走査 | OK。`tests/v070_zero_config_p1_help_test.rs:168-227` が全 help 面（`rename --help` を含む）を自動走査し、禁止語と ASCII を検査する。計画 T13 の文言追加はこの走査と同じ制約に従う |
| 拒否規則の共有範囲 | OK。`src/discovery/destinations.rs` の 7 規則のうち bundle / Library / system area / cache-temp / hidden は `&str` / `&[&str]` を取る純関数で、テストは全て公開関数 `is_excluded_destination` 経由（同ファイル `mod tests`）。移動しても既存 14 テストの判定は不変 |
| 複雑度ゲートの基準 | 条件付き。`reports/quality/baseline.json` は 278 関数・最大 CC 12 の古い記録で、現行コードの最大 CC は別途 `bash scripts/quality_check.sh` の実行結果（`reports/quality/complexity.json`）で確認する必要がある（計画 §7 がその手段を書いている）。新規は CC>=16 を作らず、既存最大を悪化させない（R1） |

## 指摘（計画書へ反映済み）

- A1 **フォルダ自身の名前が request に漏れる（重大）**: `src/normalize/mod.rs:54-68` は**常に** `F:{stem}` を `semantic_signals` に積む。計画の D4 は「`filename:` 行を出さない」としているが、そのままではフォルダ名が `signals:` 行として Swift に届き、「子の内容に追従する」という §5.4 の要求と before/after の証跡が崩れる。-> T5 に「フォルダ経路では `F:`（= target 自身の名前由来）の signal を落としてから request を組む」を明記し、W1 で「request に `F:` が無い」を固定する。**フォルダ名の prompt-injection 検査は残す**（検査は `context.target.filename` を使い、signals に依存しない）。
- A2 **truncated を検出できない**: `read_directory(path, 200)` は上限に当たっても「200 件」を返すだけで切れたことを教えない。計画 D2 の `truncated` はそのままでは常に false になる。-> `MAX_DIRECTORY_ENTRIES + 1` 件を読み、1 件超過したら `truncated = true` として 200 件に切り詰める（T1）。F1 に「250 件 -> total_entries 200 / truncated true」を固定。
- A3 **種別の分布をどこから数えるか未定義**: 名前サンプルを 12 件に絞ると、そのリストから拡張子分布を数えると 200 件フォルダで過少申告になる。-> 拡張子分布は**読み取った全エントリ**（上限 200 / `truncated` で明示）から数え、名前サンプルとは別の上限で切り出す（D2/T1）。フォルダの子は分布に含めない（種別の分布 = ファイルの拡張子）。
- A4 **プロンプトの 2000 文字予算と名前上限**: `swift/kaseifu_fm_bridge.swift:64` の `kfMaxPromptChars = 2000` でプロンプトは末尾から切られる。D2 の「ファイル 20 + フォルダ 10 件 x 64 文字」は名前だけで最大 1,920 文字になり、D4 で足す「混在時は title を空に」等の行が切られ得る（切られると §5.4 の要求がモデルに届かない）。-> 上限を **files 12 / folders 8 / 1 名 48 文字 / 種別 8** に下げ、**フォルダ向けの指示行は `folder_files:` より前**に置く（Phase 6 が video facts を `signals:` より前に置いたのと同じ理由）。S1 で「最大サンプルでも指示行と `folder_files:` が両方残る」を固定。
- A5 **相対パスと cwd / home の比較**: D8 の拒否規則 2・3 はパス比較なので、`rename tree/新しいフォルダ` のような相対パスでは絶対パスの cwd / home と一致しない。-> 拒否判定は `std::fs::canonicalize` したパスで行い（失敗時は与えられたパスに落とす）、判定材料（home / cwd）は `RefusalEnv` として明示的に渡す（純関数 `refuse_reason` は env を引数に取り、単体テストは固定 env で書ける）。T4/T5 に反映。
- A6 **`UndoError` の文言がディレクトリで虚偽になる**: 計画は「ファイルまたはディレクトリを受け入れる」としているが、`SourceNotAFile` の表示文言は「source is no longer a file」のままだと、ディレクトリに対する拒否理由として嘘になる。-> 変種名（公開 API）は変えず、`UndoError` に `Display` を追加して「記録されたパスがファイルでもフォルダでもない」と述べ、`apply_single_undo` の `{:?}` 出力を `{}` に切り替える（T12）。

## 残る注意（実装・レビュー工程への引き継ぎ）

- R1（implementer -> reviewer へ）: `bash scripts/quality_check.sh` の complexity 表で **新規 CC>=16 なし・既存最大 CC を悪化させない**ことを確認する。`assemble_plan` と `finish_plan` は既存関数なので、分岐を足すのではなく**ディレクトリ専用の関数を分ける**方針を守る（計画 T5/T6）。
- R2（reviewer へ）: 文書・画像・動画のプロンプトのバイト不変（新しい gate が `target_type == "directory"` にだけ発火すること）と、それらの経路で `input.directory` が出ないことを確認する（S2 / W12）。
- R3（reviewer へ）: `src/discovery/destinations.rs` の規則移動後も destination の判定が不変であること（既存テストと、`operations/plan.rs` 経由の拒否）を確認する。
- R4（reviewer へ）: 出力 ASCII・em dash なし・`--format json` の既存 10 キー保持（`directory` は追加のみ）を確認する。
- Q1（QA へ）: 実機検証（隔離 HOME・一時 dir・合成ツリーのみ）を §9 に実測記録すること。特に「フォルダ自身の名前ではなく子の内容に追従すること」の before/after と、D7 の親子履歴（execute -> undo、journal の `sqlite3` 直接確認）を証跡として残すこと。
- Q2（QA へ）: `KASEIFU_FM_FORCE_STUB=no-sdk` のスイートでフォルダ経路の縮退（全スイート pass）を確認すること。
- Q3（QA へ）: テストが実ユーザーのフォルダ（`~/Documents` / `~/Desktop` / `~/Library/Application Support/kaseifu/`）・実 Keychain・実補完 dir を読み書きしないこと、`Cargo.toml` / `Cargo.lock` が無変更であることを確認すること。
