# Plan Architect レビュー: v0.4.2c 移動先になれないディレクトリ排除計画

## 優先度
🟢 低（指摘 2 件は計画修正済み、確認事項 2 件。計画は承認）

## 対象
- 計画書: `documents/plans/dev/20261014c-v042c-discovery-destination-policy.md`
- 関連: `src/discovery/{similarity.rs,candidates.rs,mod.rs,scoring.rs}`, `src/classifier/jev.rs`, `src/operations/plan.rs`, `tests/*.rs`

## 検証した項目（コードベース実査）

| 観点 | 結果 |
|---|---|
| 規約・パターン整合 | ✅ 除外述語は既存 `is_system_directory(path: &str) -> bool`（`similarity.rs:176`）と同じ「文字列を受ける純関数」形式。定数・命名（`excluded_dirs`）は既存語彙に合わせる |
| 既存ユーティリティの再利用 | ✅ 機械生成名の判定に `scoring::split_stem`（`scoring.rs:51-57`、`pub`）を再利用し、拡張子除去ロジックを重複実装しない。正規表現クレートは導入せず `char` 判定（新規依存なし） |
| 配置の妥当性 | ✅ `src/policy/` は Phase 5 の意思決定閾値（`AUTO_MOVE_CONFIDENCE` 等）であり、パス規則の置き場ではない。`discovery` 内の新モジュールが依存方向として正しい（`discovery` は `models` のみに依存、新規の逆依存なし） |
| 単一情報源（重複防止） | ✅ 候補の消費者は discovery の 1 本（`classifier/pipeline.rs:212` → `DecisionContext::from_normalized` → Jev criteria / `operations/plan.rs` / 各 renderer）。集約段階での除外が criteria と表示の両方に効くことを確認 |
| 編集範囲 | ✅ src 変更は discovery 内 3 ファイル + 新モジュール 1。`spotlight.rs` の境界、`discover` の `hide = true`、JSON スキーマは不変 |
| テスト計画 | ✅ 実 Spotlight 非依存。規則表（除外・維持の境界）+ 集約 + 探索 + PDF 回帰 + 空リスト。既存フィクスチャのディレクトリ名（`CBX`/`Misc`/`Research`/`target`/`Camera Roll`/`State Of Mind Labs`/`roll-NNN`/`Documents`/`Reports`）を全数確認し、新規則 R1–R5 に抵触 0 件 |
| セキュリティ | ✅ 扱うのはパス文字列のみ。ログに追加するのは件数のみ（撮影日時・GPS・秘密情報を出力しない）。`env::var("HOME")` に依存せずパス形状で判定するため環境依存の挙動差がない |
| パフォーマンス | ✅ 判定は syscall なし（文字列のみ）。むしろヒット 2276/3526 件を採点前に落とすため I/O は減る（T3 で実測） |
| 過剰除外リスク | ✅ `$HOME`+`/Volumes` の 171,428 ディレクトリ実測で R5 の偽陽性 0 件。深さ 3 の `Library`・隠し祖先配下の可視フォルダは**維持**する設計を確認 |

## 指摘事項（計画修正済み）

### 🟡 A1. R5 の hex 条件が「16 文字以上」だけでは大文字 hex と通常の英数字名の境界が曖昧

**問題**: 計画初版は「16 文字以上の hex」とだけ書き、`[A-Z0-9]{12,}` 条件との関係（大文字 hex は両方に該当）が不明確だった。実装時にどちらの判定が先に走るかでテストの期待値が変わり得る。

**改善（適用済み）**: 計画 §3.1 の R5 を「① UUID、② 16 文字以上で全文字 hex（大小不問）、③ 12 文字以上で全文字 `A-Z0-9` かつ英字・数字を各 1 文字以上」の 3 形に明記。`LY2TLHNM2EO5K`（Eagle）は③、`e368edc30f1c4d7f88489c955fa6569a`（VSCode）は②、`20260925T175534` は③で捕捉される。

### 🟡 A2. R3（隠しディレクトリ）の適用範囲を受入条件から再確認

**問題**: ユーザ要求は「隠しディレクトリ（`.` 始まり）」を除外クラスの 1 つに挙げるが、ancestor まで適用すると本リポジトリのテストフィクスチャ（worktree が `.delta/**` 配下）や `~/.ghq/**`・`~/.local/share/**` の正当な作業ツリーが全滅し、受入基準の「過剰除外を避ける」に反する。

**改善（適用済み）**: basename 限定（候補ディレクトリ自身の名前）とし、根拠を計画 §3.1 R3・§3.2 に記録。危険ケース `~/.Trash`（basename が隠し）・`~/.cache/**`（構成要素 `*.cache`）は引き続き除外。配下の可視フォルダ（`~/.Trash/old-album`）は残余として記録（実測 0 件）。

## 確認事項

### 🟢 C1. PDF 候補件数が 17 → 16 に減る（意図的差分）

`~/Library/Application Support/Code/User/workspaceStorage/<hash>/chatSessions` は R2 で除外されるため、PDF ケースの候補件数が 1 件減る。**D1 `State Of Mind Labs` score 1.00 は不変**であり受入基準は満たす。件数差は実装レポート・QA 記録に明記すること。

### 🟢 C2. 上限 50 件に到達しないケースの期待値

画像 2 ケースは除外後に集約 128 ディレクトリ → 候補 50 件（上限）。PDF は 16 件。`MAX_CANDIDATE_DESTINATIONS` の truncate 位置（集約・ランキング後）は変更しない。

## 判定

**APPROVE**（A1/A2 は計画修正済みで実装可。C1/C2 は記録必須）

- 承認条件: (1) 既存 260 件全緑、(2) 実写真 2 件の before/after 候補一覧、(3) PDF 回帰（D1 1.00）実測、(4) 規則の境界テスト追加
- 差し戻し不要。Implementer へ進んでよい。

## 備考

- 本レビューは plan-architect スキル未導入環境のため、`.github/agents/plan-architect.agent.md` の手順（コンテキスト把握 → 検証 → issue 化 → 報告）を本会話内でエミュレートして実施した
- 実測根拠: `KASEIFU_DEBUG=1 discover` の `.debug.txt`（dir= 行 1363 件）解析、`tests/*.rs` のフィクスチャ全数 grep、`$HOME`+`/Volumes` ディレクトリ走査（171,428 件）
