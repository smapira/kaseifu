# Plan Architect レビュー: v0.4.2 Discovery 候補品質改善計画

## 優先度
🟡 中（記録必須の指摘 3 件、確認事項 3 件。計画は承認）

## 対象
- 計画書: `documents/plans/dev/20261014b-v042-discovery-photo-scope.md`
- 関連: `src/discovery/similarity.rs`, `src/discovery/mod.rs`, `src/platform/macos/spotlight.rs`, `src/context/image_metadata.rs`, `tests/phase4_context_test.rs`, `tests/phase4_full_pipeline_test.rs`, `tests/v0401_discovery_fix_test.rs`, `tests/v042_discovery_target_scope_test.rs`

## 検証した項目（コードベース実査）

| 観点 | 結果 |
|---|---|
| 規約・パターン整合 | ✅ クエリは `similarity.rs` で 1 本生成し `search(&query, limit)` に渡す現行形を維持。定数は `scoring.rs` の `W_FILENAME` 等と同じ `pub const` + doc コメント形式 |
| 既存ユーティリティの再利用 | ✅ `crate::context::image_metadata::is_image_file`（`src/context/image_metadata.rs:52-60`、拡張子判定・公開）を再利用。画像拡張子リストの重複定義を作らない |
| レイヤー分離・依存方向 | ✅ 追加依存は `discovery → context` の 1 方向のみ。`normalize → context` が既に同型の依存を持ち（`src/normalize/mod.rs` の `context::image_metadata::collect`）、循環なし（`grep: context/*.rs` に `discovery` 参照 0） |
| 編集範囲 | ✅ src 変更は 2 ファイル + 新規テスト 1。`spotlight.rs` の境界（`Fn(&str, usize)` / `take(limit)`）は不変。無関係なリファクタリングなし |
| テスト計画 | ✅ 実 Spotlight 非依存（注入 search のみ）。T1 クエリ形 / T2 切り捨て順序 / T3 PDF 回帰 / 候補上限。既存 252 件への影響は要検証（M1） |
| セキュリティ | ✅ クエリに埋める語はメタ文字を除外（`'` `"` `*` `?` `|` `&` 等）。位置情報・秘密情報はクエリにもログにも出さない（`crate::debug!` は既存の query 出力のみで、撮影日時・GPS を含まない） |
| パフォーマンス | ✅ 上限を名前付き定数化。計測手順を T5 に明記 |
| 公開 JSON | ✅ `CandidateDestination` のスキーマ・`discover` の `hide = true` は不変 |

## 指摘事項

### 🟡 M1. 既存 phase4 テストは PNG ターゲットのスタブが「クエリ文字列に `CBX` を含む」ことに依存（要検証）

`tests/phase4_context_test.rs:125` / `tests/phase4_full_pipeline_test.rs:93` のスタブは `query.contains("CBX")` のときだけ候補を返す。ターゲットは `CBX（カンナビノイド）とは？｜NATURECAN.png`（＝**画像**）なので、変更1 のスコープ付きクエリに切り替わる。

実査: 当該フィクスチャの検索語は `X:CBX`（xattr）と `F:<ステム全体>` 由来で、ファイル名に `CBX` が含まれるため **`*CBX*` 条件は残る**（ステム全体の語は `？`・`｜` を含みメタ文字除外で落ちる）。よって `contains("CBX")` は成立する見込み。

**対応（承認条件）**: 実装時に `cargo test` で phase4 系（context / full_pipeline）が緑であることを確認し、結果をレビュー・QA 記録に残すこと。もし赤になった場合は、クエリ形を見直す（スタブ側は変更しない）。

### 🟡 M2. `discover --all` の表示件数が上限 50 になる（意図的表示差分）

変更2 で「表示/上位Nの制限」を導入するため、`discover --all` は修正前の「全候補（写真ケースで 35 件）」から「ランキング上位 50 件」に変わる（写真ケースは候補 1000 件超になるため必須の制限）。`classify` が Jev へ送る `DecisionContext.candidates` も同じ上限に従う。

**対応**: 意図的差分として計画書 §T2/T5・完了報告・QA 記録に明記する。

### 🟡 M3. アプリ内画像ストア（Eagle 等）が上位に残る（残余・スコープ外）

PoC 実測（§3 Q7 の実データ）で、`~/Pictures/Eagle.library/images/<hash>.info/IMG_xxxx.jpg` が名前類似度 0.68–0.71 で上位に来る（Eagle は 1 写真 = 1 ディレクトリのため、`IMG_50xx` のような兄弟写真名が高く出る）。これは「写真を含むディレクトリ」の定義を満たすが、ユーザの写真フォルダではない。

**対応**: パス除外（`.library` 等）の追加は本リクエストのスコープ外（ユーザ指示「スコープは画像に限定」「他種別の述語を証拠なしに追加しない」に従い、変更は 2 項目に限定）。残余として修正後レポートに記録する。

### 🟢 C1. 上限値の根拠を実測で裏付けること

`MAX_SPOTLIGHT_HITS = 10_000` / `MAX_CANDIDATE_DESTINATIONS = 50` は初期値。T5 の実測（3545 件規模の real / max RSS、1 件あたりの採点コスト）で矛盾が出た場合は値を調整し、doc コメントに数値を反映する。

### 🟢 C2. CHANGELOG

`[Unreleased]` に Fixed を 2 項目（画像ターゲットの種別スコープ / 集約後切り捨て）追加。バージョン番号は本書では確定しない（親が判断）。

### 🟢 C3. baseline.json

新規関数は CC16+ を作らない見込み（クエリ生成は小さな純関数 + 文字列整形）。意図的な複雑度変化がない限り `reports/quality/baseline.json` は更新しない。

## 判定

**APPROVE（M1 は承認条件、M2/M3 は記録必須）**

- 承認条件: (1) phase4 系を含む既存テスト全緑、(2) クエリが実機で有効（`mdfind` 終了コード 0）であることを実測で示す、(3) 非画像クエリ不変（PDF 回帰）
- 差し戻し不要。Implementer へ進んでよい。

## 備考

- 本レビューは plan-architect スキル未導入環境のため、`.github/agents/plan-architect.agent.md` の手順（コンテキスト把握 → 検証 → issue 化 → 報告）を本会話内でエミュレートして実施した。
