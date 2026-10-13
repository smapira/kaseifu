# Plan Architect レビュー: v0.4.1 Discovery 汚染修正計画

## 優先度
🟡 中（非ブロッキングの指摘 2 件、確認事項 3 件。計画は承認）

## 対象
- 計画書: `documents/plans/dev/20261014a-v0401-discovery-terms-fix.md`
- 関連: `src/normalize/mod.rs`, `src/normalize/image.rs`, `src/classifier/jev.rs`, `src/models/decision.rs`, `tests/v32_image_metadata_test.rs`, `tests/v0401_discovery_fix_test.rs`

## 検証した項目（コードベース実査）

| 観点 | 結果 |
|---|---|
| 規約・パターン整合 | ✅ マーカー文字列は現在 `normalize/image.rs::signals` と `jev.rs::keep_signal` の 2 箇所のみ。移設後は `jev.rs` に一本化され「定義を二重に残さない」要件と整合 |
| 既存ユーティリティ再利用 | ✅ `ImageMetadata::has_capture_time()` / `has_location()`（`src/models/file_context.rs:150-158`）を再利用。新規ヘルパ追加は不要 |
| レイヤー分離・依存方向 | ✅ `classifier/jev.rs` は既に `ImageMetadata` を受けて `capture_date_for`/`camera_for`/`location_for`/`dimensions_for` を実装済み（v3.2）。アダプタがマーカーを生成しても依存方向は不変 |
| 編集範囲 | ✅ src 変更は 3 ファイルのみ。リファクタリング・無関係変更なし |
| テスト計画 | ✅ 受入基準 1/2 に対応する新規テストファイル + v32 テストの責務更新。mock server は std-only で新規依存なし。既存 247 件への影響は v32 の `signals` 参照のみ |
| セキュリティ | ✅ privacy ゲート（capture/gps）と「withheld leaves no trace」不変条件を維持。マーカーは可用性のみで座標・時刻を含まない |
| パフォーマンス | ✅ Vec の小さいフィルタ/合成のみ。I/O・検索回数に影響なし |
| 公開 JSON | ✅ `inspect --format json`=FileContext / `classify --format json`=decision。`semantic_signals` は非公開（`DecisionContext` の `skip_serializing` 確認済み） |

## 指摘事項

### 🟡 M1. 既存 v32 テストの wire 形状が「より本番忠実」に変化する（許容・記録必須）

`tests/v32_image_metadata_test.rs::image_context()` は手組み context で `semantic_signals` に capture/GPS マーカーを入れているが、camera マーカーは入れていない（滑入りな本番形状）。
単一情報源（strip + regenerate）方式では、`v32_jev_sends_gps_only_when_explicitly_enabled` の wire に `M:Camera available` が加わる。
これは本番の normalize 出力（camera があれば常に camera マーカーあり）と一致する方向の変化で、断言は壊れない（同テストは camera マーカーを断言していない）。

**対応**: 許容。ただし「production 経路の wire は不変 / 非本番形状の context は単一情報源ルールに従う」ことをレビュー・QA 記録に明記する。計画書 §3 の表を根拠とする。

### 🟡 M2. `classify --verbose` の stderr 診断も 3 行減る（意図的表示差分の追加記録）

`src/main.rs:536-538` は `DecisionContext.semantic_signals` を stderr に出す。修正後はマーカーがアダプタ合成になるため、この診断リストからマーカーが消える。公開 JSON ではないが表示差分として記録する。

**対応**: 計画書 §T8 と完了報告に追記（Rich `inspect` の差分と合わせて 2 箇所）。

### 🟢 C1. ゴールデン採取手順の明文化

計画書 §T5 に「同一テストコードを修正前後で実行」とあるが、採取コマンドと出力の保存先を完了報告に含めること（本レビュー時点で採取済み: 4 通りの `state` JSON）。

### 🟢 C2. CHANGELOG / jev-api.md の更新をタスクに含めること

`documents/jev-api.md:31` の「`semantic_signals` (verbatim)」は修正後「内容 signal verbatim + アダプタ生成の可用性マーカー」へ更新が必要。`CHANGELOG.md` `[Unreleased]` に Fixed を追加する（Keep a Changelog の慣例に適合）。

### 🟢 C3. 削除関数のベースライン扱い

`reports/quality/baseline.json` に `normalize::image::signals` / `keep_signal` のエントリは存在しない（baseline は v0.3.0 時点 cc512bd。`signals_lines` のみ）。削除は baseline-gate の note 扱い（失敗にならない）で問題なし。

## 判定

**APPROVE（指摘 M1/M2 は記録必須、C1–C3 は実施項目へ反映）**

- 承認条件: マーカー順序 capture→GPS→camera を normalize の旧追記位置（末尾）で維持すること。受信 signal からの stale マーカー除去を実装に含めること。
- 差し戻し不要。Implementer へ進んでよい。

## 備考

- 本レビューは plan-architect スキル未導入環境のため、`opencode/.github/agents/plan-architect.agent.md` の手順を本会話内でエミュレートして実施した（v3.1 計画と同じ運用）。
