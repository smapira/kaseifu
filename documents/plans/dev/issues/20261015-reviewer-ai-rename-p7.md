# reviewer 検収: AIリネーム P7（`20261015c-ai-rename-p7.md` の実装）

レビュー方式: 本環境に `/reviewer` スキルが無いため、`.github/agents/reviewer.agent.md` の手順（受入条件の確定 -> 周辺コードの確認 -> 仕様->実装のトレーサビリティ -> 敵対的レビュー -> 自動検証 -> 判定）をなぞって実施。根拠は `read_file` / `grep` / `terminal` の実測と、実機 CLI（隔離 HOME・合成ツリーのみ）の実行結果。

## 判定: 承認（REQUEST CHANGES なし。下記 R1〜R6 の観点は実測で確認、残る注意は QA と次ラウンドへ）

## 受入条件 -> 実装のトレーサビリティ

| 受入条件（原典） | 実装 | 実測の証跡 |
|---|---|---|
| §5.4 子ファイル名・子フォルダ名・種別の分布・代表の内容・ディレクトリ構造・親フォルダ名 | `context::directory::read_directory_facts` + `service::read_child_text` + 既存の `parent_directory` | 統合テスト v096 dir01 / dir05、実機 `--format json` の `directory` オブジェクト |
| §5.4 大規模フォルダのサンプリング | 読み 201 件（1 件超過で truncated 検出）・提示 12/8 件・種別 8 件 | v096 dir04（260 件 -> `at least 200 entries`）、実機 J-large（5.5 秒） |
| §5.4 無制限の再帰は禁止 | 深さ 1 固定（孫は名前も読まない） | v096 dir05（孫の名前が request に現れない）+ 実機 `2024-2025` |
| §5.4 空フォルダは保留 | モデル前に `Held` | v096 dir02、実機 F-empty / G-noise-only（request 0 件） |
| §5.4 内容が混在したら共通テーマを作らない | プロンプトで title 空を許容、空 title は保留 | v096 dir03、実機（title 空）、プロンプト源の文字列検査 |
| §5.4 子要素のパス変更を考慮 | D7（新しい順 undo + ディレクトリ undo + 正直な拒否） | v096 dir07/dir08（両順序）/ stale、実機 E（journal 直接確認、§9.4） |
| §9.1 検証項目 | `check_directory_name`（空・`.`/`..`・区切り・NUL・長さ）+ 既存の衝突/No-op | v096 の単体テスト（ドットを含む名前を許すことも固定） |
| §9.2 TOCTOU | dev/ino の同一性 + 種別 + 書込可 + destination 空き + 親 | v096 dir_w10（子の追加は許可、別フォルダへの入れ替えは `SourceChanged`） |
| §9.3 Journal / §10 Undo | 既存 `move` 行、record -> rename -> mark、`undone_at` | 実機 D / E（`sqlite3` で直接確認）、v096 dir07 |
| §11.2 Sampling / Lazy / Early Exit | 上限付き読み・子 3 件だけ読む・モデル前に確定 | v096 dir02/dir04、`read_child_text` の `take(3)` |
| §11.3 画像最適化 | フォルダ経路は ImageIO / AVFoundation を呼ばない | `grep -c "is_image\|perceive" src/context/directory.rs` = 0、v096（画像の子は開かない） |
| §12 エラー処理 | 保留/スキップ/実行禁止/モデル不可の分類 | v096 dir02/dir03/dir_w10、実機（guardrail violation で exit 1・何も動かない） |
| §13.5 DIR01〜DIR08 | v096 の 25 テスト + 単体 20 | §9.1 の 774 passed |
| §15 #3 | フォルダの dry-run / `--execute` / `undo` | 実機 D / E、v096 の CLI 3 テスト |
| §15 #14 | 親 rename 後も子履歴が解決 | 実機 E（絶対パスで記録・新しい順で解決）、v096 dir08 両順序 + journal 絶対パステスト |

## 敵対的レビューで試したこと

| 試行 | 実測結果 |
|---|---|
| 作業ディレクトリ自身（とその祖先）を対象にする | `refuse_reason` が `WorkingDirectory`、CLI は exit 1 で何も動かない（実機 M） |
| `Example.app` / `.config` を対象にする | それぞれ Bundle / Hidden で拒否（実機 K / L）。判定は canonical パスで行われる |
| journal の記録パスを手で改名してから undo | `SourceMissing` で拒否、メッセージが記録パスを名指しして「何も変えていない」と述べる（`UndoError` の Display、v096 の単体テスト） |
| undo 先に別のフォルダを作る | `Collision` で拒否、上書きしない（v096 dir-undo-safety、実機相当） |
| 二重 undo | `undone_at` により `NothingToUndo`（実機 E の 3 回目） |
| 提案の後でフォルダを入れ替える | `SourceChanged`（v096 dir_w10） |
| 提案の後で読み取り専用にする | `ReadOnly`（`check_directory_stable`。実装は単体テストで固定） |
| 存在しない・ファイルである対象をフォルダとして渡す | `classify_directory` が理由付きで拒否（v096 単体、実機なし） |
| 名前が `.png` のフォルダ | フォルダとして扱う（`target_kind` は filesystem 種別優先。v096 の契約テスト） |
| 子に命令文を混ぜる | モデルを呼ばず `Held`（実機 H、v096 の子抽出テスト） |
| 子フォルダだけのフォルダ / 空 / ノイズだけ | それぞれ提案（`2024-2025`）/ 保留 / 保留 |
| 相対パスで実行 | journal は canonical パス（v096 の絶対パステスト、実機 E） |
| `--format json` の既存キー | 実機で 11 キー（既存 10 + `directory`）、削除・型変更なし |
| 文書/画像/動画の経路 | 実機で `directory` 行・キーが出ない。既存 v092/v093/v094/v095 の assert は不変 |

## 確認済み項目（主要なもの）

- **層構成と再利用**: 名前形状規則（bundle / Library / system area / cache-temp / hidden / system tree 名）は `src/context/directory.rs` に移り、`src/discovery/destinations.rs` がそれを呼ぶ（規則の二重実装なし）。destination 側の既存テストは 1 件も変えずに通る（判定不変）。`src/semantic/` から `crate::rename` への新規依存は無い（実測）。
- **第三の summary を作っていない**: 集計は `normalize::noise::DirectorySummary::new` を再利用し、`DirectoryFacts` は観測（サンプル + 分布 + truncated）の入れ物。`models::file_context::DirectorySummary`（親ディレクトリ用）の意味は不変（`inspect` などの既存出力が変わらない）。
- **契約の add-only**: `FmInput` の完全リテラル 6 箇所に1 フィールドを追加しただけで、既存キーの削除・型変更なし。schema は `directory` を任意で文書化（required に入れていない）。`$id` は `fm-rename-v1` のまま。
- **プロンプトのバイト不変（要確認→確認済み）**: フォルダ分岐は `target_type == "directory"` のみで発火する。`filename:` 行は `else` 分岐にあり、文書・画像・動画のプロンプト文字列は変わらない（v093/v095 のバイト検査テストが無変更で通る）。
- **読み取り専用**: `src/context/directory.rs` は `read_dir` 以外の書き込み・削除・rename を呼ばない。Swift 側も新規の書き込み API を使っていない（`@_cdecl("kaseifu_fm_free_result")` は 1 つのまま）。
- **テストの隔離**: v096 の書き込み先は `std::env::temp_dir()` 配下のみ（`work_dir` / `isolated home`）。実ユーザーのフォルダ・実 Keychain・実補完 dir には触れない。フィクスチャはリポジトリに追加していない。
- **依存の不変**: `Cargo.toml` / `Cargo.lock` 無変更（`git diff --stat` 空）。新規設定ファイル・別 DB・独自キャッシュ・外部プロセスなし。
- **複雑度・品質ゲート**: `bash scripts/quality_check.sh` = OVERALL PASS、新規 CC>=16 = 0、既存最大 CC 15 を悪化させず（baseline-gate PASS）、新規最大は `refuse_reason` 11 / `plan_directory_rename` 8。

## 実装中に計画を超えて直した点（計画書 §9.6 に記録済み、差し戻しではなく改善）

- **フォルダ名の漏れ（計画 A1 の修正範囲を拡大）**: 実測で `kMDItemTitle` はフォルダではその名前であり、`M:`/`K:` シグナル経由で名前がプロンプトに届いていた（これで `status: noop` になる事例を実測）。フォルダは観測由来のシグナルをすべて落とし、子から読んだ `T:` だけを載せる。統合テストで「シグナルは `T:` のみ」を固定。
- **子の抜粋の選別**: 非圧縮 PDF の子でコンテナ語彙がタイトルを引きずり、モデルが自身の型名（`KfSemanticPayload`）を出力する事例を実測（4 回中 3 回）。子ごとに長い抜粋を上位 4 件に限る規則で 0 回になった（単体テストで固定）。シグナルを空にすると逆に悪化する（3 回中 3 回）ことも実測し、選別方式の根拠とした。
- **journal の絶対パス**: 相対パスで実行すると journal にも相対パスが入り、別の作業ディレクトリからの `undo` で履歴が解決できなかった。実行対象の `source`/`destination` のみ canonical 化して記録する（分類・観測・レポートは従来どおり）。**既存テストの assert は 1 件も変えていない**。

## 残る注意（QA・次ラウンドへの引き継ぎ）

- R1（QA へ）: `KASEIFU_FM_FORCE_STUB=no-sdk` のスイート（774 / 0）と、実機 3 種（空・注入・拒否）がモデル呼び出し 0 件で保留・拒否になることを確認する。
- R2（QA へ）: 隔離 HOME・一時 dir の検証で実ユーザーのフォルダを読み書きしていないこと、`git status` に無関係な変更が混ざっていないことを確認する。
- R3（次ラウンド / Phase 9 へ）: PDF の子の抜粋は Phase 4 のバイト走査の限界をそのまま引き継ぐ（非圧縮 PDF では辞書が抜粋に混ざる）。実 PDF のフォルダでのタイトル品質は Phase 9 の評価対象。
- R4（次ラウンドへ）: モデルの出力が稀に自身の型名になる事例を実測した（指示行の追加と抜粋の選別で本ラウンドの範囲では解消）。同種の退化を一般に防ぐ仕組み（出力の妥当性検査など）は Phase 9 で検討する。
- R5（実装の限界として記録）: フォルダでは `input.filename` をワイヤには載せるがプロンプトには描画しない（契約は add-only のため既存キーを削れない）。テストで「prompt に `filename:` が 1 箇所（非フォルダ分岐）だけ」を固定している。
- R6（既知の限界）: 拒否規則は destination の部分木規則と意図的に異なる（rename は根そのものだけを拒否し、`/tmp` や `$TMPDIR` の中のユーザーフォルダは許す）。`/Users` のような OS ルート自身は `SystemRoot`、ホーム自身は `Home` として拒否される。
