# 残タスク: TREC 本評価（コーパス入手が前提）

- status: blocked（**ユーザー作業待ち**: NIST のコーパス合意手続き）
- 出典: `documents/plans/user/20261014b.md`、`documents/plans/dev/20261014g-trec-benchmark-d2d.md`
- 優先度: 低（評価基盤は着地済み。実評価だけが残っている）

## 現状

- `kaseifu-bench` の 8 サブコマンドは実装済み。qrels は公開分を使う（`https://trec.nist.gov/data/robust/qrels.robust2004.txt`、6,543,541 B、sha256 `f8f2c972...ddbf4c3`）。実測: 311,410 行 / topics 249 / positive judgments 17,412 / positive docs 15,940。
- **コーパス（TREC Disks 4&5）は NIST の合意が必要**。現行案内は `https://trec.nist.gov/data/cd45/index.html`（旧 `cdrom2.html` は 404）。Organizational agreement（署名して `trec-data-admin@nist.gov` へ、件名 `request for TREC disks 4 and 5`、目安 7 営業日）+ Individual agreement。納品は download URL / login / password のメール。
- 作業領域は `~/Documents/kaseifu-benchmark/`（`~/Library/Application Support/kaseifu/` は kaseifu 自身の除外規則に落ちるため不可、と実測判断済み）。

## ユーザーの判断が要る論点（実装以前）

- Discovery は**文書の本文を検索語にしない**（ファイル名 stem 由来の `F:` と Spotlight メタデータのみ）。TREC コーパスでは coverage が 0 に近くなる可能性がある。本文を使うかは設計判断。
- フォルダランキング（L3）は D2D スコアの代用にしない、と決めている。

## やること

- （ユーザー）NIST へ合意書を送り、コーパスを受け取って `~/Documents/kaseifu-benchmark/` に置く。
- （実装）`validate` → `prepare` → `check-index` → `scatter` → `run` → `compare` を実コーパスで通し、Hit@1 を記録する。
- 事前に「本文を使うか」の結論を出す（使わないなら、この評価が何を測っているのかを計画書に明記する）。

## 受け入れ条件

- `kaseifu-bench run --system kaseifu` の結果が計画書に記録され、qrels のハッシュと入力数が再現可能に書かれている。
- コーパスは読み取りのみ（このツールはコーパスを変更しない）。

## 検証

- 再実行時に同じ入力数・同じスコアになること（seed 固定）。
