# AIリネーム Phase 9（Performance / Quality）実装計画

## 0. 位置づけと進め方

- 原典: `documents/plans/user/20261014a.md` の §11.1〜§11.5（推論時間、FFI、セッション再利用、ディレクトリサンプリング、動画最適化、メモリ、異常終了）、§9.3、§13.6 SAFE10、§14 Phase 9、§15。
- 作業基点: `ad5b0a4`（Phase 8 着地、791 passed / gate PASS）。
- 本ラウンドも親スレッドが直接実施（委任は前ラウンドで2回連続失敗したため、測定は同機で行う必要がある作業でもある）。
- Phase 9 の成果物は**実測値と、それに基づく決定**である。§11.5 が求める「セッション再利用の効果をベンチマークで評価する」を満たすために、bridge に **opt-in のセッション再利用**（既定 off）を入れ、同じワークロードを両方の設定で測れるようにした。

## 1. ベースライン（実測）

- 実装前: 791 passed / gate PASS。実装後: **795 passed / 0 failed / 4 ignored**（新規 4 = `benchmark::perf` の単体テスト。既存 assert の変更なし）。
- `bash scripts/quality_check.sh` = OVERALL PASS（rustfmt 0、clippy 0、関数 1395、CC<=10 1382（99.1%）、CC 11-15 = 13、CC>=16 = 0、最大 CC 15）。
- `Cargo.toml` / `Cargo.lock` 無変更（`libc` は既存依存）。

## 2. 実測した数値（本機、Apple Intelligence 有効）

### 2.1 FFI オーバーヘッド

- `kaseifu_fm_available` 200 回: median **588〜610 µs**（min 565、max 1185〜3412）。中身は `SystemLanguageModel.default.availability` という**システムサービスへの問い合わせ**で、境界のマーシャリングではない。
- 同じ `kaseifu_vision_perceive` を**存在しないパス**で呼ぶ（ImageIO が開く前にエラー封筒で返る）200 回: median **18.2 µs**（min 17.5、max 319）。これが境界そのもののコストで、画像 1 枚 20 ms の 0.1% 未満。
- 結論: FFI 境界は律速ではない。律速は常にモデルと Vision/AVFoundation。

### 2.2 推論時間とセッション再利用（§11.5）

| 条件 | calls | min | median | mean | max |
|---|---|---|---|---|---|
| 呼び出しごとにセッション生成（製品経路） n=5 | 5 | 3882 ms | **3938 ms** | 4714 ms | 7905 ms |
| セッション再利用（opt-in） n=5 | 5 | 3940 ms | **7986 ms** | 7920 ms | 11347 ms |
| 呼び出しごとに生成 n=3 | 3 | 8625 ms | 9960 ms | 9516 ms | 9963 ms |
| セッション再利用 n=3 | 3 | 7160 ms | 7222 ms | 8440 ms | 10938 ms |

- **同じワークロードで再利用が速いという結果は再現しなかった**。4 回の実行の median は 3.9〜10.0 秒で、順序に一貫性がない（n=5 では再利用の方が遅い）。セッション生成の削減幅は Apple の生成時間のばらつき（同じ入力で 3.9〜11.3 秒）に埋もれて測定できない。
- 結論: **再利用を採用しない**（現行どおり 1 ファイル 1 セッション）。再利用は診断用に opt-in で残す（既定 off、製品経路は `reuse_session` を渡さない）。

### 2.3 履歴の混入（§11.5 の制約）

- 再利用コンテキストで、内容の全く違う 2 文書（`zebra` / `quasar` を含む）を続けて生成し、2 番目の応答に 1 番目の語とファイル名が現れるかを調べた。
- 6 回の実行で**混入は検出されなかった**（語・ファイル名とも false）。
- ただし 2 番目の答えは多くの回で**ファイル名の音写**で、内容を述べた答えではなかった。したがってこれは「混入しない証明」ではなく**弱い証拠**である。決定は「効果が測れない」という 2.2 の結果で正当化し、混入リスクは採らない理由の補強にとどめる。

### 2.4 知覚のコスト

- Apple Vision（1024x768 PNG）: 3 回、min 18.3 / median 22.5 / max 124.4 ms（初回にフレームワーク起動を含む）。
- 動画: 2 秒（3 フレーム）median **39.6〜43.7 ms**、600 秒（5 フレーム）median **77.5 ms**（min 76.0 / max 336.7）。**10 分の動画でも 2 秒の 2 倍程度**で、時間に比例しない。代表フレーム上限 5 + tolerance ±1 秒 + maximumSize 2048 が効いている（§11.4）。
- フォルダ読み: 260 entry の合成フォルダで **398〜545 µs**、200 entry で打ち切られ `truncated` が立つ（§11.2 Directory Sampling）。

### 2.5 メモリ

- `kaseifu-bench perf` プロセス: peak RSS **26 MB**。
- `kaseifu rename <file>`（実生成 1 回）: peak RSS **29.6 MB**。
- `kaseifu rename <folder> --recursive`（50 項目・モデル呼び出し 0 回）: peak RSS **20.5 MB**。
- オンデバイスモデルは本プロセスではなくシステム側で動くため、CLI 自身のフットプリントは 20〜30 MB で項目数に依存しない。

### 2.6 異常終了（§9.3 / SAFE10）

- **計画中の SIGKILL**: 3 項目のバッチを開始 4 秒後に `kill -9`。ファイルは 1 つも動かず、ジャーナルにも行が残らない（`operations` 表はまだ作られていない）。
- **記録と成否書き込みの間で落ちた場合**（その状態を直接作って検証）: `undo` は
  `warning: 1 pending journal row (1 move, 0 undo); interrupted moves may already be at the destination`
  と警告し、**pending 行を選択せず `nothing left to undo`** を返す。ファイルは 1 つも壊さない。
- `kaseifu doctor` は pending を状態付きで報告する（実測: `Journal: ... (1 operation(s); 1 pending, 0 possibly interrupted; pending file state: move: source absent, destination present)`）。
- 結論: 自動復旧はしない（現行どおり）。「実行中の 1 行」は可視化され、利用者が判断できる。

## 3. 設計判断

| 論点 | 採用 | 却下 |
|---|---|---|
| セッション再利用（§11.5） | 既定 off の opt-in（`kaseifu_fm_create` の `{"reuse_session":true}`）。製品経路は渡さない | 既定 on（効果が測れず、履歴混入のリスクだけを背負う）、実装しない（§11.5 の評価ができない） |
| 測定の場所 | 既存の研究バイナリ `kaseifu-bench` の `perf` | `kaseifu` 本体にモードを足す（ユーザーワークフローと help を汚す）、ドキュメントのみ（再現できない） |
| 再利用の粒度 | 指示文（instruction text）ごとに 1 セッション | ターゲット種別ごと（フォルダと画像で指示が違うため結局指示文が鍵）、常に 1 セッション（指示が違うと誤った指示で生成する） |
| メモリの測り方 | `getrusage` の `ru_maxrss`（自プロセス）+ `/usr/bin/time -l`（CLI） | 外部プロファイル（SSH/権限が要る）、推測 |
| 異常終了の検証 | 計画中 kill の実測 + 記録/成否間の状態を直接作って復旧挙動を検証 | 狭い窓（<1 ms）を狙っての kill（再現性がない） |
| 長尺動画の素材 | 開発時に Swift + AVAssetWriter で 600 秒（1 fps, 105 KB）を `/tmp` に生成し、`--video` で測定 | リポジトリに大きなフィクスチャを追加（README が「数 KB を保つ」と定めている）、ffmpeg（本機で壊れている） |

## 4. 変更したファイル

- `swift/kaseifu_fm_bridge.swift`: `reuse_session` オプションを解釈し、コンテキストに指示文ごとのセッションキャッシュを持たせる（既定は従来どおり毎回生成）。
- `src/semantic/ffi.rs`: `FmContext::create_with_options`（既存の `create` は null のまま）。
- `src/benchmark/perf.rs`（新規）: 測定本体とレポート（text / JSON）。
- `src/benchmark/{mod,cli}.rs`: `perf` サブコマンド。
- `README.md`: `kaseifu-bench perf` の節。

## 5. 受け入れ基準（原典のうち本ラウンド分）との対応

| 原典 | 実測の証跡 |
|---|---|
| §11.2 FFI オーバーヘッド | 2.1（境界 18 µs、可用性確認 0.6 ms） |
| §11.5 推論時間 / セッション再利用評価 / 履歴混入の防止 | 2.2、2.3、既定 off の判断 |
| §11.2 Directory Sampling | 2.4（260 entry を 200 で打ち切り、398〜545 µs） |
| §11.4 長時間動画 | 2.4（600 秒 → 77.5 ms、フレーム 5） |
| §11 メモリ使用量評価 | 2.5（20.5〜29.6 MB） |
| §9.3 / SAFE10 異常終了 | 2.6（pending の可視化と `doctor` の状態報告） |
| §14 Phase 9 の全項目 | 2.1〜2.6 |

## 6. 検収記録（実測）

- `cargo test --all-targets --all-features` = **795 passed / 0 failed / 4 ignored**。
- `KASEIFU_FM_FORCE_STUB=no-sdk` で同スイート（stub ビルドでは `perf` は使えないが、ライブラリのテストは全部通る）。
- `bash scripts/quality_check.sh` = OVERALL PASS（CC>=16 = 0、最大 CC 15、coverage 83.72% report-only）。
- 実機測定の生出力は §2 に転記（すべて本機で取得、製品経路の変更なし）。
- 非退行: Phase 8 までの挙動は不変（795 − 4 = 791 が無変更で通過）。

## 7. 検証できなかったこと

- 実ユーザーのファイル（測定はすべて合成入力と `/tmp`）。
- セッション再利用の「生成時間以外」の効果（メモリ・トークン課金等は本機から見えない）。
- 履歴混入の**証明**（6 回で検出されなかったが、答えがファイル名音写に寄るため弱い証拠）。
- コールドスタート（初回起動・モデル未ロード）の分離測定。実測の最大値にはフレームワークのウォームアップが混ざる。
- 大規模フォルダの**計画**コスト（1 項目 1 生成なので上限 50 で数分。待ち時間は §2.2 から推算できるが実測していない）。

## 8. リスクと対策

| リスク | 対策 |
|---|---|
| 再利用をうっかり既定にする | 製品経路は `FmContext::create()` のままでオプションを渡さない。テストとコードで固定 |
| 測定が製品経路と別物になる | `perf` は製品と同じ `build_request` を使い、生成以外の観測は計測ループの外で 1 回だけ行う |
| 測定が環境依存で再現しない | 同じコマンドを誰でも実行できる（`kaseifu-bench perf`）。ばらつきを隠さず min/median/mean/max を出す |
| 長尺動画の素材がリポジトリを太らせる | フィクスチャを追加せず、開発時に `/tmp` へ生成したものを使う（生成手順を計画に記載） |
| pending を自動復旧してしまい正しい状態を壊す | 現行の「可視化のみ」を維持（実測で確認） |

## 9. 追加測定（M4 `perf-measurement-gaps`、2026-10-15、本機）

`kaseifu-bench perf` に `--batch-plan N` を追加し、初回呼び出しを常時分離表示
（`generation_cold_first` / `ffi_available_cold_first`）にした。§7 の残りのうち
以下を実測で埋める。トークン課金はブリッジの応答に使用量が出ないため本機から
は測れない（記録のみ）。

- コールドスタート: `perf --generations 3` で初回生成 **8711 ms**、定常 2 回は
  min 3990 / median 3999 / max 4008 ms。モデルロード分は約 +4.7 s。初回
  availability **979 µs**、定常 median 595 µs（199 回: min 572 / max 1360）。
- 大規模計画: `perf --batch-plan 3`（合成 txt 3 件、各 1 実生成）で準備
  **193.93 ms**（walk・観測・要求構築、モデルなし）+ 生成 min 7279 /
  median 7411 / max 10348 ms。50 項目の投影は「準備 1〜3 s + 50 × 定常生成
  （4〜9 s）」= 4〜8 分。50 回の実生成は回していない（正直な内訳表示）。
- メモリ: 上記 batch 実行の peak RSS **54,607,872 bytes**（生成 4 回 + Vision
  1 回）。対して生成 3 回の base 実行は **26,918,912 bytes**、再利用 3 回は
  **26,034,176 bytes**。プロセス内 peak の実行間比較のみ（getrusage は peak
  専用のため、項目数依存の曲線は別実行の peak 比較で読む）。
- セッション再利用の生成時間以外: `perf --generations 3 --reuse-session` で定常
  min 7444 / median 9304 / max 11164 ms、peak RSS 26.0 MB。今回の実行では再利用
  に時間・メモリの利得は出ず（ばらつきが支配的）。製品の session-per-generation
  維持の根拠に変更なし。トークン面は測定不可（上記）。
- フォルダ sampling 5000 件: read 200 件で打切り（上限）、**377.7 µs**。打切り
  後の read 時間は件数に依存しない。
- Vision（OCR あり）: 合成 invoice 画像 1 回 **274.41 ms**。
- 同時実行（手動）: `perf --generations 1` を 2 プロセス同時起動。両方 exit 0、
  初回生成 14161 ms / 14336 ms（単独 cold 8.7〜11.1 s より遅いが失敗なし）、
  peak RSS 25.8 / 26.1 MB。`FmContext` の 1-ctx-1-gen 制約はプロセス内のみで、
  プロセス間は直列化も排他もなく動く（遅くなるだけ）。ハーネスは作っていない
  （製品制約の外部プロセス禁止に触れないよう手動実行・記録のみ）。
