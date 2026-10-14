# 完了: 生成のタイムアウトを 1 回再試行し、文言を利用者向けにした

- status: **done**（2026-10-15 実装。利用者の選択は B）
- 出典: `documents/plans/dev/20261014l-ai-rename-p6.md` D6（「超過を観測したときだけ根拠付きで見直す」）、2026-10-15 の実機セッション

## 実測（2026-10-15、利用者の実写真。これが発火の根拠）

```text
$ kaseifu rename IMG_2741.jpeg
Error: skipped: Timeout          (exit 1)
$ kaseifu rename IMG_2741.jpeg   (すぐ再実行)
proposal: 2026-07-05_道路で自動車が走行する写真.jpeg / status: proposed
```

- 同じファイルに対する**再実行は成功**した（観測は 1 回、4 件中 1 件）。
- `DEFAULT_FM_TIMEOUT = 20 秒`（生成のみ）。参考実測: 合成入力の生成は 3.9〜11.3 秒、Phase 6 は 8.0〜17.9 秒。

## 問題は 2 つあった

1. **閾値**: 実写真で超える。再実行で通り、失敗は回復可能。
2. **メッセージ**: `skipped: Timeout` は内部語（`src/rename/service.rs` の `format!("skipped: {error}")`）。

## 実装したもの（B）

- [`analyze`](../../../../src/rename/service.rs) が `SemanticError::Timeout` のときに**1 回だけ再試行**する。ループしないので 1 対象の最悪ケースは生成 2 回。
- `RenameError::ModelTimeout` を追加し、文言を
  `the on-device model did not answer in time, even after a retry; the file was not changed`
  にした（内部語なし）。
- 同じ行にあった他の内部語も文章化した（`inference-failed` → `the model could not answer (...); the file was not changed`、`invalid-result` → `the model's answer did not match the expected shape (...)`、`bridge` → `the model boundary failed (...)`、`cancelled` → `the generation was cancelled; ...`）。詳細は空白を 1 行に畳んで 160 文字に制限する。
- 一括では同じ文章が `held` の理由として出る（1 件の失敗で全体は止まらない。既存の `§12` の扱いのまま）。
- テスト: `tests/v098_rename_timeout_test.rs`（再試行で成功 / 2 回目失敗で文章 + 内部語なし + 再試行は 1 回だけ / タイムアウト以外は再試行しない / 一括で 1 件だけ保留）。

## 実測（実装後、`f519188` 以降）

- `cargo test --all-targets --all-features` = **799 passed / 0 failed / 4 ignored**（+4）。
- `bash scripts/quality_check.sh` = OVERALL PASS（CC>=16 = 0、最大 CC 15）。
- 実機確認（タイムアウトを一時的に 1 ms にして同じ経路を通したもの）の出力は下の「検証」に転記。

## 残っていること

- 実写真で連続実行しての失敗率と所要時間の記録（3840x2160 級 5 枚、複数回）。実装は済んでいるが、数字は未取得。
- 最悪ケースの待ち時間は 20 秒 → **最大 40 秒**になった。体感が問題になるなら `--timeout` を検討する（未実装）。

## 検証

- `tests/v098_rename_timeout_test.rs`（stub provider、Apple Intelligence 不要）。
- 実機: タイムアウトを一時的に 1 ms にしたビルドで、`Error: the on-device model did not answer in time, even after a retry; the file was not changed` が出ること（内部語が消えていること）。

